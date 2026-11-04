//! End-to-end tests: run `tb attach` in a pseudo-terminal, with `alacritty_terminal` playing the user's
//! terminal, type keys and clicks into it, and read back what the screen shows.
//!
//! Every test uses its own socket folder, state folder and session name, so it never touches real sessions.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;

const TB: &str = env!("CARGO_BIN_EXE_tb");
const TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Default)]
struct Proxy {
    sender: Arc<Mutex<Option<EventLoopSender>>>,
    clipboard: Arc<Mutex<Vec<String>>>,
    exited: Arc<AtomicBool>,
}

impl EventListener for Proxy {
    fn send_event(&self, event: Event) {
        match event {
            // Answers to the client's terminal queries go back to it.
            Event::PtyWrite(text) => {
                if let Some(tx) = self.sender.lock().unwrap().as_ref() {
                    let _ = tx.send(Msg::Input(text.into_bytes().into()));
                }
            }
            Event::ClipboardStore(_, text) => self.clipboard.lock().unwrap().push(text),
            Event::ChildExit(_) | Event::Exit => self.exited.store(true, Ordering::SeqCst),
            _ => {}
        }
    }
}

struct Size(usize, usize);

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.1
    }
    fn screen_lines(&self) -> usize {
        self.1
    }
    fn columns(&self) -> usize {
        self.0
    }
}

/// An isolated tb setup: its own folders, environment and session name.
struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    sock: PathBuf,
    session: String,
    vars: Vec<(String, String)>,
}

impl Env {
    fn new(session: &str) -> Env {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        // Unix socket paths must be short, so sockets live directly under /tmp.
        let sock = PathBuf::from(format!("/tmp/tbt-{}-{session}", std::process::id()));
        let vars = vec![
            ("TB_SOCKET_DIR".to_owned(), sock.display().to_string()),
            ("TB_STATE_DIR".to_owned(), root.join("state").display().to_string()),
            ("XDG_DATA_HOME".to_owned(), root.join("data").display().to_string()),
            ("SHELL".to_owned(), "/bin/sh".to_owned()),
            ("PS1".to_owned(), "$ ".to_owned()),
            ("TB_EDITOR".to_owned(), String::new()),
        ];
        Env { _dir: dir, root, sock, session: session.to_owned(), vars }
    }

    fn set(&mut self, key: &str, value: &str) {
        self.vars.retain(|(k, _)| k != key);
        self.vars.push((key.to_owned(), value.to_owned()));
    }

    /// Runs `tb -s <session> <args>` and returns its output; panics if it fails.
    fn tb(&self, args: &[&str]) -> String {
        let out = Command::new(TB)
            .arg("-s")
            .arg(&self.session)
            .args(args)
            .envs(self.vars.iter().map(|(k, v)| (k, v)))
            .env_remove("TB_SESSION")
            .output()
            .unwrap();
        assert!(out.status.success(), "tb {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn panes(&self) -> Vec<serde_json::Value> {
        serde_json::from_str(&self.tb(&["list-panes", "-j"])).unwrap()
    }

    fn focused(&self) -> u64 {
        self.panes().iter().find(|p| p["focused"] == true).map(|p| p["id"].as_u64().unwrap()).unwrap()
    }

    fn attach(&self, cols: u16, rows: u16) -> Screen {
        Screen::spawn(self, &["-s", &self.session, "attach"], cols, rows)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ =
            Command::new(TB).args(["-s", &self.session, "kill"]).envs(self.vars.iter().map(|(k, v)| (k, v))).output();
        let _ = fs::remove_dir_all(&self.sock);
    }
}

/// A terminal window running a command.
struct Screen {
    term: Arc<FairMutex<Term<Proxy>>>,
    tx: EventLoopSender,
    proxy: Proxy,
}

impl Screen {
    fn spawn(env: &Env, args: &[&str], cols: u16, rows: u16) -> Screen {
        let proxy = Proxy::default();
        let size = Size(usize::from(cols), usize::from(rows));
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &size, proxy.clone())));
        let mut vars: HashMap<String, String> = env.vars.iter().cloned().collect();
        vars.insert("TERM".into(), "xterm-256color".into());
        let options = tty::Options {
            shell: Some(tty::Shell::new(TB.into(), args.iter().map(|s| s.to_string()).collect())),
            working_directory: Some(env.root.clone()),
            drain_on_exit: true,
            env: vars,
        };
        let ws = WindowSize { num_lines: rows, num_cols: cols, cell_width: 8, cell_height: 16 };
        let pty = tty::new(&options, ws, 0).unwrap();
        let event_loop = EventLoop::new(term.clone(), proxy.clone(), pty, true, false).unwrap();
        let tx = event_loop.channel();
        *proxy.sender.lock().unwrap() = Some(tx.clone());
        drop(event_loop.spawn());
        Screen { term, tx, proxy }
    }

    fn send(&self, bytes: &[u8]) {
        self.tx.send(Msg::Input(bytes.to_vec().into())).unwrap();
        sleep(Duration::from_millis(80));
    }

    fn text(&self) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let mut out = Vec::new();
        for line in 0..grid.screen_lines() as i32 {
            let row = &grid[Line(line)];
            let mut s = String::new();
            for col in 0..grid.columns() {
                let cell = &row[Column(col)];
                if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    s.push(cell.c);
                }
            }
            out.push(s.trim_end().to_owned());
        }
        out.join("\n")
    }

    fn wait_until(&self, what: &str, check: impl Fn(&str) -> bool) -> String {
        let start = Instant::now();
        loop {
            let text = self.text();
            if check(&text) {
                return text;
            }
            if start.elapsed() > TIMEOUT {
                panic!("timed out waiting for {what}; the screen shows:\n{text}");
            }
            sleep(Duration::from_millis(50));
        }
    }

    fn wait_for(&self, needle: &str) -> String {
        self.wait_until(needle, |t| t.contains(needle))
    }

    fn wait_exit(&self) {
        let start = Instant::now();
        while !self.proxy.exited.load(Ordering::SeqCst) {
            assert!(start.elapsed() < TIMEOUT, "the client didn't exit; the screen shows:\n{}", self.text());
            sleep(Duration::from_millis(50));
        }
    }
}

fn wait<T>(what: &str, f: impl Fn() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(start.elapsed() < TIMEOUT, "timed out waiting for {what}");
        sleep(Duration::from_millis(50));
    }
}

/// An SGR mouse report as a terminal sends it (1-based coordinates).
fn click(x: u16, y: u16) -> Vec<u8> {
    format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", x + 1, y + 1, x + 1, y + 1).into_bytes()
}

#[test]
fn attach_type_split_detach_reattach() {
    let env = Env::new("e2e-attach");
    let s = env.attach(100, 30);
    s.wait_for("Ctrl+B ? help");
    s.wait_for("$");
    s.send(b"echo hi-there\r");
    s.wait_until("command output", |t| t.lines().any(|l| l.trim() == "hi-there"));

    // Ctrl+B | splits right; the new pane gets focus.
    s.send(b"\x02|");
    wait("two panes", || (env.panes().len() == 2).then_some(()));
    let text = s.wait_for("│");
    assert!(text.lines().nth(5).unwrap().contains('│'), "a separator between the panes:\n{text}");
    assert_eq!(env.focused(), 2);

    // Clicking the left pane focuses it.
    s.send(&click(5, 5));
    wait("pane 1 focused", || (env.focused() == 1).then_some(()));

    // Ctrl+B d detaches; the session keeps running.
    s.send(b"\x02d");
    s.wait_exit();
    s.wait_for("[detached from session e2e-attach]");
    assert!(env.tb(&["ls"]).contains("e2e-attach: 1 tabs, 0 attached"));

    // Reattaching shows the same panes and their content.
    let s2 = env.attach(100, 30);
    s2.wait_until("old output", |t| t.lines().any(|l| l.contains("hi-there")));
    assert_eq!(env.panes().len(), 2);
}

#[test]
fn tabs_by_key_and_click() {
    let env = Env::new("e2e-tabs");
    let s = env.attach(100, 30);
    s.wait_for(" 1 ");
    s.send(b"\x02c");
    s.wait_for(" 2 ");
    let tab_of = |id: u64| env.panes().iter().find(|p| p["id"] == id).map(|p| p["tab"].as_u64().unwrap());
    assert_eq!(tab_of(env.focused()), Some(2));
    // Click the first tab in the tab bar.
    s.send(&click(1, 0));
    wait("tab 1 active", || (tab_of(env.focused()) == Some(1)).then_some(()));
    // Rename it with Ctrl+B ,
    s.send(b"\x02,");
    s.send(b"\x7f\x7f\x7f\x7f\x7f\x7fwork\r");
    s.wait_for(" 1 work ");
}

#[test]
fn finder_previews_and_edits() {
    let mut env = Env::new("e2e-finder");
    let files = env.root.join("files");
    fs::create_dir_all(files.join("sub")).unwrap();
    fs::write(files.join("notes.md"), "# Notes Title\n\nSome *body* text.\n\n- one\n- two\n").unwrap();
    fs::write(files.join("data.csv"), "name,qty\napple,3\npear,10\n").unwrap();
    fs::write(files.join("code.rs"), "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
    // A fake editor: shows which file it got, waits for a line, then appends to the file.
    let editor = env.root.join("fake-editor");
    fs::write(&editor, "#!/bin/sh\necho \"EDITING $1\"\nread line\necho \"$line\" >> \"$1\"\n").unwrap();
    std::os::unix::fs::PermissionsExt::set_mode(&mut fs::metadata(&editor).unwrap().permissions(), 0o755);
    Command::new("chmod").arg("755").arg(&editor).status().unwrap();
    env.set("TB_EDITOR", &editor.display().to_string());

    env.tb(&["start"]);
    // `tb finder` from inside the session opens a tab without attaching.
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let s = env.attach(120, 30);
    // Folders first, then files by name; the first entry (an empty folder) is previewed.
    let text = s.wait_for("(empty folder)");
    let order: Vec<usize> = ["sub/", "code.rs", "data.csv", "notes.md"].iter().map(|n| text.find(n).unwrap()).collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "listing order:\n{text}");

    s.send(b"j");
    s.wait_for("1 │ fn main() {");
    s.send(b"j");
    let text = s.wait_for("apple");
    assert!(text.contains("─┼─") && text.contains("pear"), "csv table:\n{text}");
    s.send(b"j");
    let text = s.wait_for("Notes Title");
    assert!(!text.contains("# Notes"), "headings are rendered, not raw:\n{text}");
    assert!(text.contains("• one"), "list bullets:\n{text}");

    // Enter opens the editor inside the finder; when it exits, the preview shows the change.
    s.send(b"\r");
    s.wait_for("EDITING");
    s.wait_for("✎ notes.md");
    s.send(b"added by the editor\r");
    s.wait_for("added by the editor");
    s.wait_until("editor closed", |t| !t.contains("EDITING"));
    assert!(fs::read_to_string(files.join("notes.md")).unwrap().ends_with("added by the editor\n"));

    // Filter: / then letters narrows the list.
    s.send(b"/csv");
    let text = s.wait_for("/csv");
    assert!(text.contains("data.csv") && !text.contains("code.rs"), "filtered list:\n{text}");
    let dump = env.tb(&["dump-screen"]);
    assert!(dump.contains("data.csv"), "{dump}");
}

#[test]
fn copy_mode_search_and_osc52() {
    let env = Env::new("e2e-copy");
    let s = env.attach(100, 30);
    s.wait_for("$");
    s.send(b"echo alpha-beta-gamma\r");
    s.wait_until("output", |t| t.lines().any(|l| l.trim() == "alpha-beta-gamma"));
    s.send(b"\x02[");
    s.wait_for("COPY");
    // Search backwards, select the word, copy it.
    s.send(b"?alpha\r");
    s.send(b"v");
    s.send(b"e");
    s.send(b"y");
    let clip = wait("clipboard", || s.proxy.clipboard.lock().unwrap().last().cloned());
    assert_eq!(clip, "alpha-beta-gamma");
    s.wait_for("copied 16 characters");
}

#[test]
fn scrollback_with_the_wheel() {
    let env = Env::new("e2e-scroll");
    let s = env.attach(80, 24);
    s.wait_for("$");
    s.send(b"seq 1 200\r");
    s.wait_for("200");
    // Three wheel-up reports scroll 9 lines back; the indicator shows the position.
    for _ in 0..3 {
        s.send(b"\x1b[<64;5;5M");
    }
    s.wait_for(" 9/");
    // Typing jumps back to the bottom.
    s.send(b"x");
    s.wait_until("back at the bottom", |t| !t.contains(" 9/"));
}

#[test]
fn programs_exiting_close_panes_and_the_session() {
    let env = Env::new("e2e-exit");
    env.tb(&["start"]);
    env.tb(&["action", "split", "down", "--", "/bin/sh", "-c", "sleep 0.3"]);
    assert_eq!(env.panes().len(), 2);
    wait("the split to close", || (env.panes().len() == 1).then_some(()));
    env.tb(&["action", "write", "--enter", "exit"]);
    wait("the session to end", || (!env.tb(&["ls"]).contains("e2e-exit")).then_some(()));
    assert!(!Path::new(&env.sock).join("e2e-exit.sock").exists());
}

#[test]
fn drag_to_select_copies() {
    let env = Env::new("e2e-drag");
    let s = env.attach(80, 24);
    s.wait_for("$");
    s.send(b"echo one two three\r");
    let text = s.wait_until("output", |t| t.lines().any(|l| l.trim() == "one two three"));
    let y = text.lines().position(|l| l.trim() == "one two three").unwrap() as u16;
    // Press on "two", drag to the end of "three", release.
    s.send(format!("\x1b[<0;5;{}M", y + 1).as_bytes());
    s.send(format!("\x1b[<32;9;{}M", y + 1).as_bytes());
    s.send(format!("\x1b[<32;13;{}M", y + 1).as_bytes());
    s.send(format!("\x1b[<0;13;{}m", y + 1).as_bytes());
    let clip = wait("clipboard", || s.proxy.clipboard.lock().unwrap().last().cloned());
    assert_eq!(clip, "two three");
    // Double-click selects a word.
    s.send(format!("\x1b[<0;2;{y1}M\x1b[<0;2;{y1}m\x1b[<0;2;{y1}M\x1b[<0;2;{y1}m", y1 = y + 1).as_bytes());
    wait("word copied", || (s.proxy.clipboard.lock().unwrap().last().map(String::as_str) == Some("one")).then_some(()));
}

#[test]
fn finder_refreshes_when_the_folder_changes() {
    let env = Env::new("e2e-watch");
    let files = env.root.join("w");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("first.txt"), "1").unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = env.attach(100, 24);
    s.wait_for("first.txt");
    fs::write(files.join("second.txt"), "2").unwrap();
    s.wait_for("second.txt");
}

/// Prints what a split session and the finder look like. Run with:
/// `cargo test -p tb --test e2e show_screens -- --ignored --nocapture`
#[test]
#[ignore]
fn show_screens() {
    let env = Env::new("e2e-show");
    let s = env.attach(110, 28);
    s.wait_for("$");
    s.send(b"echo left pane\r");
    s.send(b"\x02|");
    s.send(b"echo right pane\r");
    s.send(b"\x02-");
    s.wait_for("right pane");
    sleep(Duration::from_millis(300));
    println!("{}\n{}", "=".repeat(110), s.text());
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plans").canonicalize().unwrap();
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", docs.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    s.send(b"j");
    sleep(Duration::from_millis(500));
    println!("{}\n{}", "=".repeat(110), s.text());
}

/// Opens a file in the real Neovim (needs `nvim`), types, and closes without saving. Run with:
/// `cargo test -p tb --test e2e real_neovim -- --ignored --nocapture`
#[test]
#[ignore]
fn real_neovim() {
    let mut env = Env::new("e2e-nvim");
    env.vars.retain(|(k, _)| k != "TB_EDITOR");
    let files = env.root.join("n");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("a.md"), "# Hello\n").unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = env.attach(110, 26);
    s.wait_for("a.md");
    s.send(b"\r");
    s.wait_for("Alt+s save");
    s.send(b"typed in nvim");
    s.wait_for("typed in nvim");
    println!("{}", s.text());
    // Alt+q asks to save; answer No.
    s.send(b"\x1bq");
    s.wait_for("Save changes");
    s.send(b"n");
    s.wait_until("nvim closed", |t| !t.contains("Alt+s save · Alt+q close  ·"));
    assert_eq!(fs::read_to_string(files.join("a.md")).unwrap(), "# Hello\n");
    println!("{}", s.text());
}
