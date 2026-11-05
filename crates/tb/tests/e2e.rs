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
    /// The cell size in pixels that the test terminal reports.
    cell: (u16, u16),
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
        Env { _dir: dir, root, sock, session: session.to_owned(), vars, cell: (8, 16) }
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

    /// Runs `tb <args>` (no `-s`) outside any session and returns its output, failed or not.
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(TB)
            .args(args)
            .envs(self.vars.iter().map(|(k, v)| (k, v)))
            .env_remove("TB_SESSION")
            .output()
            .unwrap()
    }

    /// Like `run`, but panics if it fails, and returns stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
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
        // Any other session a test started in this socket folder.
        let _ = self.run(&["kill-all-sessions", "-y"]);
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
        let ws = WindowSize { num_lines: rows, num_cols: cols, cell_width: env.cell.0, cell_height: env.cell.1 };
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

/// A screen line without the pane frame's side lines.
fn unframe(line: &str) -> &str {
    line.trim().trim_matches('│').trim()
}

/// An SGR mouse report as a terminal sends it (1-based coordinates).
fn click(x: u16, y: u16) -> Vec<u8> {
    format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", x + 1, y + 1, x + 1, y + 1).into_bytes()
}

#[test]
fn attach_type_split_detach_reattach() {
    let env = Env::new("e2e-attach");
    let s = env.attach(100, 30);
    s.wait_for("<g> LOCK");
    s.wait_for("$");
    // Every pane has a frame.
    let text = s.wait_for("┌");
    assert!(text.lines().nth(1).unwrap().starts_with('┌'), "a frame at the top left:\n{text}");
    s.send(b"echo hi-there\r");
    s.wait_until("command output", |t| t.lines().any(|l| unframe(l) == "hi-there"));

    // Ctrl+P r opens a pane on the right; the new pane gets focus.
    s.send(b"\x10r");
    wait("two panes", || (env.panes().len() == 2).then_some(()));
    let text = s.wait_for("┐┌");
    assert!(text.lines().nth(5).unwrap().contains("││"), "two frames side by side:\n{text}");
    assert_eq!(env.focused(), 2);

    // Clicking the left pane focuses it.
    s.send(&click(5, 5));
    wait("pane 1 focused", || (env.focused() == 1).then_some(()));

    // Ctrl+O d detaches; the session keeps running.
    s.send(b"\x0fd");
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
    // Ctrl+T n opens a tab.
    s.send(b"\x14n");
    s.wait_for(" 2 ");
    let tab_of = |id: u64| env.panes().iter().find(|p| p["id"] == id).map(|p| p["tab"].as_u64().unwrap());
    assert_eq!(tab_of(env.focused()), Some(2));
    // Click the first tab in the tab bar.
    s.send(&click(1, 0));
    wait("tab 1 active", || (tab_of(env.focused()) == Some(1)).then_some(()));
    // Ctrl+T r renames it; the old name is cleared first, as in zellij.
    s.send(b"\x14r");
    s.send(b"work\r");
    s.wait_for(" 1 work ");
    // Ctrl+B is zellij's tmux mode: c opens a tab.
    s.send(b"\x02c");
    s.wait_for(" 3 ");
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
fn select_mode_search_and_osc52() {
    let env = Env::new("e2e-copy");
    let s = env.attach(100, 30);
    s.wait_for("$");
    s.send(b"echo alpha-beta-gamma\r");
    s.wait_until("output", |t| t.lines().any(|l| unframe(l) == "alpha-beta-gamma"));
    // Ctrl+S is scroll mode; v selects with vi keys.
    s.send(b"\x13v");
    s.wait_for("SELECT");
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
    s.wait_for("SCROLL: 9/");
    // Typing jumps back to the bottom.
    s.send(b"x");
    s.wait_until("back at the bottom", |t| !t.contains("SCROLL:"));
}

#[test]
fn programs_exiting_close_panes_and_the_session() {
    let env = Env::new("e2e-exit");
    env.tb(&["start"]);
    env.tb(&["action", "split", "down", "--", "/bin/sh", "-c", "sleep 0.3"]);
    assert_eq!(env.panes().len(), 2);
    wait("the split to close", || (env.panes().len() == 1).then_some(()));
    env.tb(&["action", "write", "--enter", "exit"]);
    // zellij's name for `ls` works too.
    wait("the session to end", || (!env.tb(&["list-sessions"]).contains("e2e-exit")).then_some(()));
    assert!(!Path::new(&env.sock).join("e2e-exit.sock").exists());
}

#[test]
fn drag_to_select_copies() {
    let env = Env::new("e2e-drag");
    let s = env.attach(80, 24);
    s.wait_for("$");
    s.send(b"echo one two three\r");
    let text = s.wait_until("output", |t| t.lines().any(|l| unframe(l) == "one two three"));
    let y = text.lines().position(|l| unframe(l) == "one two three").unwrap() as u16;
    // The frame shifts the text one column right. Press on "two", drag to the end of "three", release.
    s.send(format!("\x1b[<0;6;{}M", y + 1).as_bytes());
    s.send(format!("\x1b[<32;10;{}M", y + 1).as_bytes());
    s.send(format!("\x1b[<32;14;{}M", y + 1).as_bytes());
    s.send(format!("\x1b[<0;14;{}m", y + 1).as_bytes());
    let clip = wait("clipboard", || s.proxy.clipboard.lock().unwrap().last().cloned());
    assert_eq!(clip, "two three");
    // Double-click selects a word.
    s.send(format!("\x1b[<0;3;{y1}M\x1b[<0;3;{y1}m\x1b[<0;3;{y1}M\x1b[<0;3;{y1}m", y1 = y + 1).as_bytes());
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

#[test]
fn zellij_modes() {
    let env = Env::new("e2e-modes");
    let s = env.attach(100, 30);
    s.wait_for("<g> LOCK");
    s.wait_for("$");
    let x_of = |id: u64| env.panes().iter().find(|p| p["id"] == id).map(|p| p["x"].as_u64().unwrap()).unwrap();
    let cols_of = |id: u64| env.panes().iter().find(|p| p["id"] == id).map(|p| p["cols"].as_u64().unwrap()).unwrap();

    // Pane mode shows its keys; Esc goes back.
    s.send(b"\x10");
    s.wait_for(" PANE ");
    s.send(b"\x1b");
    s.wait_for("<g> LOCK");

    // Locked mode passes Ctrl+P to the program instead of switching mode.
    s.send(b"\x07");
    s.wait_for(" LOCKED ");
    s.send(b"\x10");
    sleep(Duration::from_millis(200));
    assert!(s.text().contains(" LOCKED "), "still locked:\n{}", s.text());
    s.send(b"\x07");
    s.wait_for("<g> LOCK");

    // Alt+N opens a pane where there's room: to the right on a wide screen.
    s.send(b"\x1bn");
    wait("two panes", || (env.panes().len() == 2).then_some(()));
    assert_eq!(env.focused(), 2);
    assert!(x_of(2) > x_of(1));

    // Move mode: h swaps the focused pane with its left neighbour.
    s.send(b"\x08h");
    wait("panes swapped", || (x_of(2) < x_of(1)).then_some(()));
    s.send(b"\x1b");

    // Resize mode: l grows the focused (now left) pane's right side.
    let before = cols_of(2);
    s.send(b"\x0el\x1b");
    wait("pane grew", || (cols_of(2) > before).then_some(()));

    // Ctrl+P z turns frames off and on.
    s.send(b"\x10z");
    s.wait_until("frames off", |t| !t.contains('┌'));
    s.send(b"\x10z");
    s.wait_for("┌");

    // Ctrl+T b breaks the focused pane out into its own tab.
    s.send(b"\x14b");
    s.wait_for(" 2 ");
    let tabs: Vec<u64> = env.panes().iter().map(|p| p["tab"].as_u64().unwrap()).collect();
    assert_eq!(tabs, vec![1, 2]);

    // Ctrl+P c renames the pane; the frame shows the name.
    s.send(b"\x10cbuild\r");
    s.wait_for("┌ build ");

    // Ctrl+P x closes the pane (and its tab) without asking, as in zellij.
    s.send(b"\x10x");
    wait("one pane", || (env.panes().len() == 1).then_some(()));

    // Ctrl+Q asks before ending the session.
    s.send(b"\x11");
    s.wait_for("QUIT?");
    s.send(b"n");
    s.wait_for("<g> LOCK");
    assert_eq!(env.panes().len(), 1);
}

#[test]
fn scroll_mode_search() {
    let env = Env::new("e2e-search");
    let s = env.attach(80, 24);
    s.wait_for("$");
    // The command line doesn't contain the word; only the output does.
    s.send(b"printf 'nee%s\\n' dle-one; seq 1 60\r");
    s.wait_for("60");
    // Ctrl+S s, type, Enter: search mode jumps to the match above and scrolls it into view.
    s.send(b"\x13s");
    s.wait_for("find:");
    s.send(b"needle\r");
    let text = s.wait_for("SCROLL:");
    assert!(text.lines().any(|l| unframe(l) == "needle-one"), "the match is on screen:\n{text}");
    assert!(text.contains(" SEARCH "), "{text}");
    // Ctrl+C goes back to the bottom and to normal mode.
    s.send(b"\x03");
    s.wait_until("back at the bottom", |t| !t.contains("SCROLL:") && t.contains("g lock"));
}

#[test]
fn sync_move_tab_and_edit_scrollback() {
    let mut env = Env::new("e2e-more");
    // A fake editor: shows the first line of the file it got, waits for a line, then exits.
    let editor = env.root.join("show-editor");
    fs::write(&editor, "#!/bin/sh\necho \"FIRST: $(head -n 1 \"$1\")\"\nread line\n").unwrap();
    Command::new("chmod").arg("755").arg(&editor).status().unwrap();
    env.set("TB_EDITOR", &editor.display().to_string());
    let s = env.attach(100, 30);
    s.wait_for("$");
    let screen = |id: u64| env.tb(&["dump-screen", "-p", &id.to_string()]);

    // Ctrl+T s syncs the tab: typing goes to both panes.
    s.send(b"\x10r");
    wait("two panes", || (env.panes().len() == 2).then_some(()));
    s.send(b"\x14s");
    s.wait_for("(SYNC)");
    s.send(b"echo both-$((1+1))\r");
    wait("both panes ran it", || (screen(1).contains("both-2") && screen(2).contains("both-2")).then_some(()));
    s.send(b"\x14s");
    s.wait_until("sync off", |t| !t.contains("(SYNC)"));

    // Alt+O moves the tab right (wrapping to the front), Ctrl+T Tab goes back to the last tab.
    s.send(b"\x14n");
    s.wait_for(" 2 ");
    s.send(b"\x1bo");
    let tab_of = |id: u64| env.panes().iter().find(|p| p["id"] == id).map(|p| p["tab"].as_u64().unwrap());
    wait("the new tab moved to the front", || (tab_of(3) == Some(1)).then_some(()));
    s.send(b"\x14\t");
    wait("back on the old tab", || (tab_of(env.focused()) == Some(2)).then_some(()));
    s.send(b"\x1b");

    // Ctrl+S e opens the focused pane's scrollback in the editor, in the pane's place; it comes back after.
    let focused = env.focused();
    s.send(b"\x13e");
    s.wait_for("FIRST: $ echo both-$((1+1))");
    s.wait_for("┌ scrollback ");
    s.send(b"\r");
    wait("the pane is back", || (env.focused() == focused).then_some(()));
    s.wait_until("editor gone", |t| !t.contains("FIRST:"));
}

#[test]
fn tabs_are_named_after_the_running_program() {
    let env = Env::new("e2e-names");
    let s = env.attach(100, 30);
    s.wait_for(" 1 sh ");
    // `cat` starts half a second after the last output and prints nothing, so the tab bar must update
    // without any output to trigger it.
    s.send(b"sleep 0.5; cat\r");
    s.wait_for(" 1 sleep ");
    s.wait_for(" 1 cat ");
    s.wait_for("┌ cat ");
    s.send(b"\x04");
    s.wait_for(" 1 sh ");
}

#[test]
fn sessions_come_back_after_kill() {
    let env = Env::new("e2e-persist");
    let proj = env.root.join("proj");
    let files = env.root.join("files");
    fs::create_dir_all(&proj).unwrap();
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("a.txt"), "a").unwrap();
    fs::write(files.join("b.txt"), "b").unwrap();
    let s = env.attach(100, 30);
    s.wait_for("$");
    // Tab 1: a shell in proj, and a pane running `sleep`; renamed "work".
    s.send(format!("cd {}\r", proj.display()).as_bytes());
    s.send(b"\x10r");
    wait("two panes", || (env.panes().len() == 2).then_some(()));
    s.send(b"sleep 300\r");
    s.wait_for("┌ sleep ");
    s.send(b"\x14rwork\r");
    // Tab 2: a finder with b.txt selected (opened as if from inside the session, so it doesn't attach).
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    s.wait_for("a.txt");
    s.send(b"j");
    s.wait_for("┌ b.txt ");

    env.tb(&["kill"]);
    s.wait_exit();
    let ls = env.tb(&["ls"]);
    assert!(ls.contains("e2e-persist (exited, attach to restore)"), "{ls}");

    // Attaching brings it back: tabs, names, folders, the finder's selection, and the command at the prompt.
    let s = env.attach(100, 30);
    s.wait_for(" 1 work ");
    s.wait_for(" 2 finder ");
    s.wait_for("┌ b.txt ");
    let panes = env.panes();
    assert_eq!(panes.len(), 3, "{panes:?}");
    assert_eq!(panes[0]["cwd"].as_str(), Some(proj.to_str().unwrap()));
    let second = panes[1]["id"].as_u64().unwrap().to_string();
    wait("sleep typed, not run", || env.tb(&["dump-screen", "-p", &second]).contains("$ sleep 300").then_some(()));
    assert_eq!(panes[1]["title"].as_str(), Some("sh"), "the shell, not sleep, is running");

    // `tb delete` refuses a running session without --force, then forgets it.
    let out = Command::new(TB)
        .args(["-s", &env.session, "delete"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(!out.status.success());
    env.tb(&["delete", "--force"]);
    s.wait_exit();
    wait("forgotten", || env.tb(&["ls"]).contains("no sessions").then_some(()));
}

/// Programs in a pane see the real terminal's cell size, so tools like chafa keep images in shape.
#[test]
fn panes_get_the_terminal_cell_size() {
    let mut env = Env::new("e2e-cell");
    env.set("TB_IMAGES", "blocks");
    env.cell = (10, 21);
    let s = env.attach(100, 30);
    s.wait_for("$");
    let pane = &env.panes()[0];
    let (cols, rows) = (pane["cols"].as_u64().unwrap(), pane["rows"].as_u64().unwrap());
    // `CSI 14 t` asks for the text area in pixels; the answer comes back as typed input, echoed as `^[[4;…t`.
    s.send(b"printf '\\033[14t'\r");
    s.wait_for(&format!("[4;{};{}t", rows * 21, cols * 10));
}

#[test]
fn zellij_session_commands() {
    let env = Env::new("e2e-zj");
    let ls = || env.ok(&["list-sessions"]);
    let short = |extra: &[&str]| -> Vec<String> {
        let args: Vec<&str> = ["ls", "-s"].iter().chain(extra).copied().collect();
        env.ok(&args).lines().map(str::to_owned).collect()
    };

    // `attach -b NAME` and `a --create-background NAME` start sessions without attaching.
    env.ok(&["attach", "-b", "zj-a"]);
    env.ok(&["a", "--create-background", "zj-b"]);
    assert_eq!(short(&[]), ["zj-a", "zj-b"]);
    assert_eq!(short(&["-r"]), ["zj-b", "zj-a"]);
    let out = Command::new(TB)
        .args(["ls"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", "zj-b")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("zj-b: 1 tabs, 0 attached (current)"));

    // Two tabs in zj-a; `kill-session NAME` keeps the layout, and `attach -b` brings both tabs back.
    env.ok(&["action", "-s", "zj-a", "new-tab"]);
    env.ok(&["kill-session", "zj-a"]);
    wait("zj-a exited", || ls().contains("zj-a (exited, attach to restore)").then_some(()));
    env.ok(&["attach", "-b", "zj-a"]);
    assert!(ls().contains("zj-a: 2 tabs"), "{}", ls());
    // `--forget` starts it fresh instead.
    env.ok(&["k", "zj-a"]);
    wait("zj-a exited", || ls().contains("zj-a (exited").then_some(()));
    env.ok(&["attach", "-b", "--forget", "zj-a"]);
    assert!(ls().contains("zj-a: 1 tabs"), "{}", ls());

    // `-f` runs a restored command instead of only typing it.
    env.ok(&["ac", "-s", "zj-b", "new-tab", "--", "sleep", "300"]);
    env.ok(&["kill-session", "zj-b"]);
    wait("zj-b exited", || ls().contains("zj-b (exited").then_some(()));
    env.ok(&["attach", "-b", "-f", "zj-b"]);
    wait("sleep running again", || {
        let panes: Vec<serde_json::Value> = serde_json::from_str(&env.ok(&["list-panes", "-s", "zj-b", "-j"])).unwrap();
        panes.iter().any(|p| p["title"] == "sleep").then_some(())
    });

    // `kill-all-sessions -y` ends both and keeps their layouts.
    env.ok(&["ka", "-y"]);
    wait("both exited", || (ls().matches("(exited").count() == 2).then_some(()));
    // Without `-y` it asks, and anything but y keeps the sessions (here stdin is empty: no).
    env.ok(&["attach", "-b", "zj-a"]);
    let out = env.ok(&["kill-all-sessions"]);
    assert!(out.contains("End zj-a"), "{out}");
    assert!(ls().contains("zj-a: "), "still running: {}", ls());

    // `delete-all-sessions -y` forgets exited sessions only; `-f` also ends and forgets running ones.
    let out = env.ok(&["da", "-y"]);
    assert!(out.contains("still running: zj-a"), "{out}");
    assert_eq!(short(&[]), ["zj-a"]);
    env.ok(&["delete-all-sessions", "-y", "-f"]);
    wait("all gone", || (ls().trim() == "no sessions").then_some(()));

    // `delete-session NAME` and `d -f NAME`.
    env.ok(&["attach", "-b", "zj-c"]);
    assert!(!env.run(&["delete-session", "zj-c"]).status.success(), "refuses a running session");
    env.ok(&["d", "-f", "zj-c"]);
    wait("zj-c gone", || (ls().trim() == "no sessions").then_some(()));
}

/// Opens a finder on a folder holding one picture, with images forced to `protocol`.
fn finder_with_picture(name: &str, protocol: &str) -> (Env, Screen) {
    let mut env = Env::new(name);
    env.set("TB_IMAGES", protocol);
    let files = env.root.join("pics");
    fs::create_dir_all(&files).unwrap();
    let img = image::RgbImage::from_fn(200, 100, |x, _| image::Rgb([if x < 100 { 255 } else { 0 }, 0, 128]));
    img.save(files.join("pic.png")).unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(100, 30);
    s.wait_for("200×100");
    (env, s)
}

#[test]
fn images_as_blocks_or_real() {
    // Blocks: the picture is drawn with half-block characters.
    let (_env, s) = finder_with_picture("e2e-blocks", "blocks");
    s.wait_for("▀");
    // kitty: blocks stand in while the picture is encoded, then the cells are left for the image.
    let (_env2, s2) = finder_with_picture("e2e-kitty", "kitty");
    s2.wait_until("the image placed instead of blocks", |t| !t.contains('▀'));
}

#[test]
fn csv_grid_editing() {
    let env = Env::new("e2e-grid");
    let files = env.root.join("sheet");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("data.csv"), "name,qty\napple,3\npear,10\n").unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(100, 30);
    s.wait_for("data.csv");
    // Enter opens the grid.
    s.send(b"\r");
    s.wait_for("▦ data.csv");
    s.wait_for("row 0/2");
    // Change apple's qty from 3 to 5.
    s.send(b"jle\x7f5\r");
    s.wait_for("● unsaved");
    // Add a row under apple and name it kiwi.
    s.send(b"a0ekiwi\r");
    // Alt+s saves; q closes.
    s.send(b"\x1bs");
    s.wait_for("saved ");
    assert_eq!(fs::read_to_string(files.join("data.csv")).unwrap(), "name,qty\napple,5\nkiwi,\npear,10\n");
    s.send(b"q");
    s.wait_until("grid closed", |t| !t.contains("▦ data.csv"));
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
    s.send(b"\x10r");
    s.send(b"echo right pane\r");
    s.send(b"\x10d");
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
    // The CSV grid.
    let sheet = env.root.join("sheet");
    fs::create_dir_all(&sheet).unwrap();
    fs::write(
        sheet.join("fruit.csv"),
        "name,qty,colour,notes\napple,3,red,crisp\npear,10,green,\"soft, sweet\"\nkiwi,7,brown,\n",
    )
    .unwrap();
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", sheet.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    s.wait_for("fruit.csv");
    s.send(b"\rjl");
    sleep(Duration::from_millis(300));
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
    s.wait_until("nvim closed", |t| !t.contains("Alt+s save"));
    assert_eq!(fs::read_to_string(files.join("a.md")).unwrap(), "# Hello\n");
    println!("{}", s.text());
}
