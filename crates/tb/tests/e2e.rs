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
    assert!(env.tb(&["ls"]).contains("e2e-attach [1 tabs, 0 attached]"));

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
    env.tb(&["action", "write-chars", "--enter", "exit"]);
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
    assert!(ls.contains("e2e-persist (EXITED - attach to restore)"), "{ls}");

    // Attaching brings it back: tabs, names, folders, the finder's selection, and the command at the prompt.
    let s = env.attach(100, 30);
    s.wait_for(" 1 work ");
    s.wait_for(" 2 finder ");
    s.wait_for("┌ b.txt ");
    let panes = env.panes();
    // The finder is two panes, its file list and its preview, linked again.
    let kinds: Vec<_> = panes.iter().map(|p| p["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["terminal", "terminal", "finder", "preview"], "{panes:?}");
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
    assert!(String::from_utf8_lossy(&out.stdout).contains("zj-b [1 tabs, 0 attached] (current)"));

    // Two tabs in zj-a; `kill-session NAME` keeps the layout, and `attach -b` brings both tabs back.
    env.ok(&["action", "-s", "zj-a", "new-tab"]);
    env.ok(&["kill-session", "zj-a"]);
    wait("zj-a exited", || ls().contains("zj-a (EXITED - attach to restore)").then_some(()));
    env.ok(&["attach", "-b", "zj-a"]);
    assert!(ls().contains("zj-a [2 tabs"), "{}", ls());
    // `--forget` starts it fresh instead.
    env.ok(&["k", "zj-a"]);
    wait("zj-a exited", || ls().contains("zj-a (EXITED").then_some(()));
    env.ok(&["attach", "-b", "--forget", "zj-a"]);
    assert!(ls().contains("zj-a [1 tabs"), "{}", ls());

    // `-f` runs a restored command instead of only typing it.
    env.ok(&["ac", "-s", "zj-b", "new-tab", "--", "sleep", "300"]);
    env.ok(&["kill-session", "zj-b"]);
    wait("zj-b exited", || ls().contains("zj-b (EXITED").then_some(()));
    env.ok(&["attach", "-b", "-f", "zj-b"]);
    wait("sleep running again", || {
        let panes: Vec<serde_json::Value> = serde_json::from_str(&env.ok(&["list-panes", "-s", "zj-b", "-j"])).unwrap();
        panes.iter().any(|p| p["title"] == "sleep").then_some(())
    });

    // `kill-all-sessions -y` ends both and keeps their layouts.
    env.ok(&["ka", "-y"]);
    wait("both exited", || (ls().matches("(EXITED").count() == 2).then_some(()));
    // Without `-y` it asks, and anything but y keeps the sessions (here stdin is empty: no).
    env.ok(&["attach", "-b", "zj-a"]);
    let out = env.ok(&["kill-all-sessions"]);
    assert!(out.contains("End zj-a"), "{out}");
    assert!(ls().contains("zj-a ["), "still running: {}", ls());

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

/// Parses JSON output from `tb`.
fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// zellij's tab actions: ids, names, lists, going to, renaming, moving, syncing and closing tabs.
#[test]
fn zellij_tab_actions() {
    let env = Env::new("e2e-tab-actions");
    env.tb(&["start"]);
    // new-tab prints the new tab's id; list-tabs prints zellij's columns.
    let two: u32 = env.tb(&["action", "new-tab", "-n", "two"]).trim().parse().expect("a tab id");
    let three: u32 = env.tb(&["action", "new-tab", "--name", "three"]).trim().parse().unwrap();
    let text = env.tb(&["action", "list-tabs"]);
    assert!(text.starts_with("TAB_ID  POSITION  NAME\n"), "{text}");
    assert!(text.contains(&format!("{two}  1  two")), "{text}");
    assert_eq!(env.tb(&["action", "query-tab-names"]).lines().skip(1).collect::<Vec<_>>(), ["two", "three"]);
    let tabs = json(&env.tb(&["action", "list-tabs", "-j"]));
    for field in ["tab_id", "position", "name", "active", "is_fullscreen_active", "is_sync_panes_active"] {
        assert!(tabs[0].get(field).is_some(), "list-tabs -j lacks {field}: {tabs}");
    }
    assert_eq!(tabs[2]["active"], true, "the newest tab is active");
    // Going to tabs by id and by name, and current-tab-info.
    env.tb(&["action", "go-to-tab-by-id", &two.to_string()]);
    assert_eq!(json(&env.tb(&["action", "current-tab-info", "-j"]))["name"], "two");
    env.tb(&["action", "go-to-tab-name", "three"]);
    assert_eq!(json(&env.tb(&["action", "current-tab-info", "-j"]))["tab_id"], three);
    assert!(
        Command::new(TB)
            .args(["-s", &env.session, "action", "go-to-tab-name", "nope"])
            .envs(env.vars.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap()
            .status
            .code()
            != Some(0)
    );
    env.tb(&["action", "go-to-tab-name", "-c", "made"]);
    assert_eq!(json(&env.tb(&["action", "current-tab-info", "-j"]))["name"], "made");
    // Renaming by id, and undoing it (the name follows the program again).
    env.tb(&["action", "rename-tab-by-id", &two.to_string(), "renamed"]);
    assert!(env.tb(&["action", "query-tab-names"]).contains("renamed"));
    env.tb(&["action", "undo-rename-tab", "-t", &two.to_string()]);
    assert!(!env.tb(&["action", "query-tab-names"]).contains("renamed"));
    // Moving a tab by id keeps the active tab active; sync by id.
    env.tb(&["action", "move-tab", "left", "-t", &three.to_string()]);
    let tabs = json(&env.tb(&["action", "list-tabs", "-j"]));
    let pos = |id: u32| tabs.as_array().unwrap().iter().find(|t| t["tab_id"] == id).unwrap()["position"].clone();
    assert_eq!(pos(three), 1, "{tabs}");
    assert_eq!(json(&env.tb(&["action", "current-tab-info", "-j"]))["name"], "made");
    env.tb(&["action", "toggle-active-sync-tab", "-t", &three.to_string()]);
    let tabs = json(&env.tb(&["action", "list-tabs", "-j"]));
    assert_eq!(tabs.as_array().unwrap().iter().find(|t| t["tab_id"] == three).unwrap()["is_sync_panes_active"], true);
    // Next and previous wrap around; closing by id.
    env.tb(&["action", "go-to-next-tab"]);
    assert_eq!(json(&env.tb(&["action", "current-tab-info", "-j"]))["position"], 0);
    env.tb(&["action", "go-to-previous-tab"]);
    assert_eq!(json(&env.tb(&["action", "current-tab-info", "-j"]))["name"], "made");
    env.tb(&["action", "close-tab-by-id", &three.to_string()]);
    env.tb(&["action", "close-tab"]);
    assert_eq!(json(&env.tb(&["action", "list-tabs", "-j"])).as_array().unwrap().len(), 2);
}

/// zellij's pane actions: new-pane and run print ids; list-panes has zellij's fields; focus, rename,
/// fullscreen, colours, edit and close work by id.
#[test]
fn zellij_pane_actions() {
    let mut env = Env::new("e2e-panes");
    let editor = env.root.join("editor.sh");
    let args = env.root.join("editor-args");
    fs::write(&editor, format!("#!/bin/sh\necho \"$@\" > {}\nsleep 300\n", args.display())).unwrap();
    Command::new("chmod").arg("755").arg(&editor).status().unwrap();
    env.set("TB_EDITOR", editor.to_str().unwrap());
    let s = env.attach(120, 40);
    s.wait_for("$");
    let first = env.focused();
    let side = env.tb(&["action", "new-pane", "-d", "right", "-n", "side", "--", "sleep", "300"]);
    let side_id: u64 = side.trim().strip_prefix("terminal_").expect("terminal_N").parse().unwrap();
    s.wait_for("┌ side ");
    let ran = Command::new(TB)
        .args(["-s", &env.session, "run", "-d", "down", "--", "sleep", "301"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&ran.stdout).trim().starts_with("terminal_"), "{ran:?}");
    // zellij's text columns and JSON fields.
    let text = env.tb(&["action", "list-panes", "-c"]);
    assert!(text.starts_with("PANE_ID  TYPE  TITLE  COMMAND  CWD\n"), "{text}");
    assert!(text.contains(&format!("terminal_{side_id}  terminal")), "{text}");
    let panes = json(&env.tb(&["action", "list-panes", "-j"]));
    let pane = panes.as_array().unwrap().iter().find(|p| p["id"] == side_id).unwrap().clone();
    for field in
        ["is_plugin", "is_focused", "title", "tab_id", "tab_name", "pane_command", "pane_cwd", "terminal_command"]
    {
        assert!(pane.get(field).is_some(), "list-panes -j lacks {field}: {pane}");
    }
    assert_eq!(pane["terminal_command"], "sleep 300");
    // Focus by id (bare and terminal_N), rename and undo, fullscreen by id.
    env.tb(&["action", "focus-pane-id", &first.to_string()]);
    assert_eq!(env.focused(), first);
    env.tb(&["action", "focus-pane-id", &format!("terminal_{side_id}")]);
    assert_eq!(env.focused(), side_id);
    env.tb(&["action", "rename-pane", "-p", &first.to_string(), "main-shell"]);
    s.wait_for("┌ main-shell ");
    env.tb(&["action", "undo-rename-pane", "-p", &first.to_string()]);
    wait("name gone", || (!s.text().contains("main-shell")).then_some(()));
    env.tb(&["action", "toggle-fullscreen", "-p", &first.to_string()]);
    let panes = json(&env.tb(&["action", "list-panes", "-j"]));
    assert!(panes.as_array().unwrap().iter().any(|p| p["id"] == first && p["is_fullscreen"] == true), "{panes}");
    env.tb(&["action", "toggle-fullscreen"]);
    // Colours, as zellij writes them.
    env.tb(&["action", "set-pane-color", "-p", &first.to_string(), "--bg", "rgb:10/20/30"]);
    let colour = |env: &Env| {
        json(&env.tb(&["action", "list-panes", "-j"])).as_array().unwrap().iter().find(|p| p["id"] == first).unwrap()
            ["default_bg"]
            .clone()
    };
    assert_eq!(colour(&env), "#102030");
    env.tb(&["action", "set-pane-color", "-p", &first.to_string(), "--reset"]);
    assert_eq!(colour(&env), serde_json::Value::Null);
    // edit opens the editor at a line, in a new pane.
    let file = env.root.join("notes.txt");
    fs::write(&file, "a\nb\nc\n").unwrap();
    let edited = env.tb(&["action", "edit", "-l", "3", file.to_str().unwrap()]);
    assert!(edited.trim().starts_with("terminal_"), "{edited}");
    let got = wait("editor started", || fs::read_to_string(&args).ok().filter(|a| !a.is_empty()));
    assert_eq!(got.trim(), format!("+3 {}", file.display()));
    // Close by id.
    env.tb(&["action", "close-pane", "-p", &format!("terminal_{side_id}")]);
    assert!(!json(&env.tb(&["action", "list-panes", "-j"])).as_array().unwrap().iter().any(|p| p["id"] == side_id));
}

/// zellij's input, scrolling and session actions.
#[test]
fn zellij_input_and_session_actions() {
    let env = Env::new("e2e-input");
    let s = env.attach(100, 30);
    s.wait_for("$");
    let id = env.focused().to_string();
    // write-chars and send-keys; write takes bytes ("echo bytes" and Enter); paste.
    env.tb(&["action", "write-chars", "echo one"]);
    env.tb(&["action", "send-keys", "Enter"]);
    s.wait_for("one");
    env.tb(&["action", "write", "-p", &id, "101", "99", "104", "111", "32", "98", "121", "116", "101", "115", "13"]);
    s.wait_for("bytes");
    env.tb(&["action", "paste", "echo pasted"]);
    env.tb(&["action", "send-keys", "Enter"]);
    s.wait_for("pasted");
    // dump-screen: to a file, and with colours.
    let out = env.root.join("screen.txt");
    env.tb(&["action", "dump-screen", "--path", out.to_str().unwrap()]);
    assert!(fs::read_to_string(&out).unwrap().contains("bytes"));
    env.tb(&["action", "write-chars", "--enter", "printf '\\033[31mred\\033[0m\\n'"]);
    s.wait_for("red");
    assert!(env.tb(&["action", "dump-screen", "-a"]).contains("\x1b[0;31mred"));
    // Scrolling shows in the frame; clear empties the pane.
    env.tb(&["action", "write-chars", "--enter", "seq 1 200"]);
    s.wait_for("200");
    env.tb(&["action", "scroll-to-top"]);
    s.wait_for("SCROLL:");
    env.tb(&["action", "scroll-to-bottom"]);
    wait("back at the bottom", || (!s.text().contains("SCROLL:")).then_some(()));
    env.tb(&["action", "clear"]);
    wait("cleared", || (!env.tb(&["action", "dump-screen"]).contains("200")).then_some(()));
    // switch-mode shows in the status bar.
    env.tb(&["action", "switch-mode", "locked"]);
    s.wait_for("LOCKED");
    env.tb(&["action", "switch-mode", "normal"]);
    // list-clients shows the attached client.
    let clients = env.tb(&["action", "list-clients"]);
    assert!(clients.starts_with("CLIENT_ID  PANE_ID  RUNNING_COMMAND\n") && clients.lines().count() == 2, "{clients}");
    // save-session writes the saved layout; dump-layout prints it.
    env.tb(&["action", "save-session"]);
    assert!(env.root.join(format!("state/sessions/{}.json", env.session)).exists());
    assert!(json(&env.tb(&["action", "dump-layout"]))["tabs"].is_array());
    // rename-session: the session answers under its new name; detach ends the client.
    let new = format!("{}-renamed", env.session);
    env.tb(&["action", "rename-session", &new]);
    let ls = env.tb(&["ls"]);
    assert!(ls.contains(&format!("{new} [")), "{ls}");
    let detach = Command::new(TB)
        .args(["-s", &new, "action", "detach"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(detach.status.success(), "{detach:?}");
    s.wait_exit();
    let _ = Command::new(TB).args(["-s", &new, "kill"]).envs(env.vars.iter().map(|(k, v)| (k, v))).output();
}

/// A `tb subscribe` running in the background; its output lines are collected.
struct Subscriber {
    child: std::process::Child,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Subscriber {
    fn start(env: &Env, args: &[&str]) -> Subscriber {
        use std::io::BufRead;
        let mut child = Command::new(TB)
            .args(["-s", &env.session, "subscribe"])
            .args(args)
            .envs(env.vars.iter().map(|(k, v)| (k, v)))
            .env_remove("TB_SESSION")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
                sink.lock().unwrap().push(line);
            }
        });
        Subscriber { child, lines }
    }

    /// Waits for a line containing `needle`, and returns it.
    fn wait_for(&self, needle: &str) -> String {
        wait(needle, || self.lines.lock().unwrap().iter().find(|l| l.contains(needle)).cloned())
    }

    /// Waits for a JSON event with these fields.
    fn wait_event(&self, fields: serde_json::Value) -> serde_json::Value {
        wait(&fields.to_string(), || {
            self.lines
                .lock()
                .unwrap()
                .iter()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .find(|e| fields.as_object().unwrap().iter().all(|(k, v)| &e[k] == v))
        })
    }
}

impl Drop for Subscriber {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `tb subscribe`: session events as JSON lines, and a pane's text as it changes.
#[test]
fn subscribe_reports_events() {
    let env = Env::new("e2e-subscribe");
    env.tb(&["start"]);
    let first = env.focused().to_string();
    // The first delivery is the subscribed pane's text, so once it arrives, events are flowing.
    let events = Subscriber::start(&env, &["-f", "json", "-p", &first]);
    events.wait_event(serde_json::json!({"event": "pane_text"}));
    let tab: u64 = env.tb(&["action", "new-tab", "-n", "two"]).trim().parse().unwrap();
    events.wait_event(serde_json::json!({"event": "tab_opened", "tab_id": tab, "name": "two"}));
    events.wait_event(serde_json::json!({"event": "tab_focused", "tab_id": tab}));
    env.tb(&["action", "rename-tab", "renamed"]);
    events.wait_event(serde_json::json!({"event": "tab_renamed", "tab_id": tab, "name": "renamed"}));
    // A pane that exits reports its exit status.
    let pane = env.tb(&["action", "new-pane", "--", "sh", "-c", "sleep 0.5; exit 3"]);
    let id: u64 = pane.trim().strip_prefix("terminal_").unwrap().parse().unwrap();
    events.wait_event(serde_json::json!({"event": "pane_opened", "pane_id": id}));
    events.wait_event(serde_json::json!({"event": "pane_closed", "pane_id": id, "exit_status": 3}));
    env.tb(&["action", "switch-mode", "locked"]);
    events.wait_event(serde_json::json!({"event": "mode_changed", "mode": "locked"}));
    env.tb(&["action", "switch-mode", "normal"]);
    env.tb(&["action", "close-tab"]);
    events.wait_event(serde_json::json!({"event": "tab_closed", "tab_id": tab}));
    // Raw format: the pane's text, printed when it changes.
    let text = Subscriber::start(&env, &["-p", &format!("terminal_{first}")]);
    env.tb(&["action", "write-chars", "-p", &first, "--enter", "echo hello-sub"]);
    text.wait_for("hello-sub");
    // A pane that doesn't exist is an error.
    let bad = env.run(&["-s", &env.session, "subscribe", "-p", "999"]);
    assert!(!bad.status.success() && String::from_utf8_lossy(&bad.stderr).contains("no pane 999"), "{bad:?}");
}

/// Status-bar text from extensions: segments by id, expiry, clearing, and clicks reported to subscribers.
#[test]
fn status_segments() {
    let env = Env::new("e2e-status");
    let s = env.attach(100, 30);
    s.wait_for("$");
    let first = env.focused().to_string();
    let events = Subscriber::start(&env, &["-f", "json", "-p", &first]);
    events.wait_event(serde_json::json!({"event": "pane_text"}));
    env.tb(&["action", "set-status", "--id", "prbot", "PR #487 needs review"]);
    env.tb(&["action", "set-status", "--id", "clock", "12:00"]);
    let bottom = |s: &Screen| s.text().lines().last().unwrap_or_default().to_owned();
    wait("both segments", || {
        (bottom(&s).contains("PR #487 needs review") && bottom(&s).contains("12:00")).then_some(())
    });
    // Setting an id again replaces its text, in place.
    env.tb(&["action", "set-status", "--id", "prbot", "PR #490 too"]);
    wait("replaced", || {
        let b = bottom(&s);
        (b.contains("PR #490 too") && !b.contains("#487") && b.find("#490") < b.find("12:00")).then_some(())
    });
    // A click on a segment reaches subscribers with the segment's id.
    let line = bottom(&s);
    let x = line[..line.find("12:00").unwrap()].chars().count() as u16;
    s.send(&click(x, 29));
    events.wait_event(serde_json::json!({"event": "status_clicked", "id": "clock", "button": "left"}));
    // --ttl expires a segment; clear-status removes one, or all.
    env.tb(&["action", "set-status", "--id", "brief", "--ttl", "1", "gone soon"]);
    s.wait_for("gone soon");
    wait("expired", || (!bottom(&s).contains("gone soon")).then_some(()));
    env.tb(&["action", "clear-status", "--id", "clock"]);
    wait("clock cleared", || (!bottom(&s).contains("12:00")).then_some(()));
    env.tb(&["action", "clear-status"]);
    wait("all cleared", || (!bottom(&s).contains("PR #490")).then_some(()));
}

/// `switch-session` moves the attached client to another session; `tb watch` shows a session read-only.
#[test]
fn switch_session_and_watch() {
    let env = Env::new("e2e-switch");
    let other = format!("{}-other", env.session);
    let s = env.attach(100, 30);
    s.wait_for("$");
    env.tb(&["action", "switch-session", &other]);
    wait("switched", || {
        let ls = env.tb(&["ls"]);
        (ls.contains(&format!("{other} [1 tabs, 1 attached]"))
            && ls.contains(&format!("{} [1 tabs, 0 attached]", env.session)))
        .then_some(())
    });
    s.wait_for("$");
    // A watcher sees the session, can't type into it, and doesn't change its size.
    let size =
        |env: &Env| json(&env.ok(&["-s", &other, "action", "current-tab-info", "-j"]))["viewport_columns"].clone();
    let before = size(&env);
    let w = Screen::spawn(&env, &["watch", &other], 60, 20);
    w.wait_for("WATCH");
    w.wait_for("read-only");
    w.send(b"echo typed-by-watcher\r");
    sleep(Duration::from_millis(500));
    assert!(!env.ok(&["-s", &other, "dump-screen"]).contains("typed-by-watcher"));
    assert_eq!(size(&env), before, "the watcher's size doesn't change the layout");
    // The attached client can still type, and the watcher sees it.
    s.send(b"echo from-the-owner\r");
    w.wait_for("from-the-owner");
    w.send(b"q");
    w.wait_exit();
    assert!(env.ok(&["ls"]).contains(&format!("{other} [1 tabs, 1 attached]")));
}

/// Keys bound in config.kdl, in zellij's syntax: Run a command, type text; reload-config reports what it
/// skipped, and a broken file keeps the old bindings.
#[test]
fn config_key_bindings() {
    let mut env = Env::new("e2e-keys");
    let config = env.root.join("config.kdl");
    fs::write(
        &config,
        r#"
keybinds clear-defaults=true {
    tab {
        bind "p" { Run "sh" "-c" "echo bound-run; sleep 300" { name "bound"; }; SwitchToMode "Normal"; }
    }
    normal {
        bind "Alt y" { WriteChars "echo from-binding"; Write 13; }
    }
}
"#,
    )
    .unwrap();
    env.set("TB_CONFIG", config.to_str().unwrap());
    let s = env.attach(100, 30);
    s.wait_for("$");
    // Ctrl+t then p: the bound Run opens a named pane, and the binding switches back to normal mode.
    s.send(b"\x14p");
    s.wait_for("┌ bound ");
    s.wait_for("bound-run");
    s.wait_for("<g> LOCK");
    // Alt+y types into the focused pane.
    env.tb(&["action", "focus-previous-pane"]);
    // A line the command printed (not the echoed command line).
    let printed = |word: &'static str| move |t: &str| t.lines().any(|l| unframe(l).starts_with(word));
    s.send(b"\x1by");
    s.wait_until("from-binding printed", printed("from-binding"));
    // reload-config lists what it skipped; the bindings it could load still work.
    fs::write(&config, "keybinds {\n  normal {\n    bind \"Alt y\" { WriteChars \"echo reloaded\"; Write 13; }\n    bind \"Alt u\" { ToggleFloatingPanes; }\n  }\n}\n")
        .unwrap();
    let warnings = env.tb(&["action", "reload-config"]);
    assert!(
        warnings.contains("line 4: skipped this binding: tb has no action 'ToggleFloatingPanes' yet"),
        "{warnings}"
    );
    s.send(b"\x1by");
    s.wait_until("reloaded printed", printed("reloaded"));
    // A file that isn't KDL is an error, and the old bindings stay.
    fs::write(&config, "keybinds {\n  normal {\n    bind \"Alt y\" { WriteChars \"never\n").unwrap();
    let broken = env.run(&["-s", &env.session, "action", "reload-config"]);
    assert!(!broken.status.success() && String::from_utf8_lossy(&broken.stderr).contains("line 3"), "{broken:?}");
    s.send(b"\x1by");
    wait("old binding still works", || (s.text().matches("reloaded").count() >= 4).then_some(()));
}

/// docs/examples/exit-watcher.py, the example extension, against a real session: a failing pane puts a
/// warning in the status bar, and clicking it clears it.
#[test]
fn example_extension_runs() {
    if Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 isn't installed; skipping");
        return;
    }
    let env = Env::new("e2e-example");
    let s = env.attach(100, 30);
    s.wait_for("$");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/examples/exit-watcher.py");
    let mut child = Command::new("python3")
        .arg(script)
        .arg(&env.session)
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB", TB)
        .env_remove("TB_SESSION")
        .spawn()
        .unwrap();
    let bottom = |s: &Screen| s.text().lines().last().unwrap_or_default().to_owned();
    // The extension subscribes in the background, so try until it has seen a failure.
    let shown = (0..10).any(|_| {
        env.tb(&["action", "new-pane", "--", "sh", "-c", "sleep 0.3; exit 2"]);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(1500) {
            if bottom(&s).contains("exited 2") {
                return true;
            }
            sleep(Duration::from_millis(50));
        }
        false
    });
    assert!(shown, "no warning; the bar shows: {}", bottom(&s));
    let line = bottom(&s);
    let x = line[..line.find("exited 2").unwrap()].chars().count() as u16;
    s.send(&click(x, 29));
    wait("warning cleared", || (!bottom(&s).contains("exited 2")).then_some(()));
    let _ = child.kill();
    let _ = child.wait();
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
fn pdf_text_pages_and_zoom() {
    // Like finder-core's PDF tests, this needs poppler-utils.
    if Command::new("pdftoppm").arg("-v").output().is_err() {
        eprintln!("skipped: poppler-utils isn't installed");
        return;
    }
    let env = Env::new("e2e-pdf");
    let files = env.root.join("pdfs");
    fs::create_dir_all(&files).unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../finder-core/tests/data");
    fs::copy(data.join("two-pages.pdf"), files.join("a.pdf")).unwrap();
    fs::copy(data.join("scanned.pdf"), files.join("b-scan.pdf")).unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(100, 30);

    // Text first, with both pages and the page number and title in the header.
    let text = s.wait_for("Page one says alpha");
    s.wait_for("── page 2 ──");
    s.wait_for("page 1/2");
    assert!(text.contains("Fixture document"), "title in the header:\n{text}");

    // In the preview, t shows the page at fit width; this terminal gets it as blocks.
    s.send(b"l");
    s.send(b"t");
    s.wait_for("fit width");
    s.wait_until("the page drawn", |t| t.contains('▀') && !t.contains("rendering"));
    s.send(b"+");
    s.wait_for("150%");
    s.send(b"0");
    s.wait_for("fit width");
    s.send(b"]");
    s.wait_until("page 2 drawn", |t| t.contains("page 2/2") && !t.contains("rendering"));

    // t again: back to the text; h back to the list.
    s.send(b"t");
    s.wait_until("the text again", |t| t.contains("Page two says beta") && !t.contains('▀'));
    s.send(b"h");

    // A scanned PDF has no text, so it shows its page.
    s.send(b"j");
    s.wait_for("no text · showing pages");
    s.wait_until("the scanned page drawn", |t| t.contains('▀'));
}

#[test]
fn pdf_page_view_with_kitty_images() {
    if Command::new("pdftoppm").arg("-v").output().is_err() {
        eprintln!("skipped: poppler-utils isn't installed");
        return;
    }
    let mut env = Env::new("e2e-pdf-kitty");
    env.set("TB_IMAGES", "kitty");
    let files = env.root.join("pdfs");
    fs::create_dir_all(&files).unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../finder-core/tests/data");
    fs::copy(data.join("two-pages.pdf"), files.join("a.pdf")).unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(100, 30);
    s.wait_for("Page one says alpha");
    s.send(b"lt");
    s.wait_until("the page placed as an image", |t| {
        t.contains("fit width") && !t.contains('▀') && !t.contains("rendering")
    });
    // Each scroll step is a new picture, placed in turn; on to page 2 at the end of page 1.
    s.send(b"jjjjj   ");
    s.wait_until("page 2 placed", |t| t.contains("page 2/2") && !t.contains('▀') && !t.contains("rendering"));
}

/// The finder's file list and preview are two real panes: borders drag, the preview goes fullscreen, closing
/// one closes both, and a narrow tab shows one at a time.
#[test]
fn finder_is_two_panes() {
    let env = Env::new("e2e-twopane");
    let files = env.root.join("files");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("a.txt"), "first file\n").unwrap();
    env.tb(&["start"]);
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(100, 30);
    s.wait_for("first file");
    let pane = |kind: &str| env.panes().into_iter().find(|p| p["kind"] == kind).unwrap();
    let (list, preview) = (pane("finder"), pane("preview"));
    assert_eq!((list["pane_columns"].as_u64(), preview["pane_columns"].as_u64()), (Some(35), Some(65)));
    assert_eq!(list["is_focused"], true);

    // Drag the border (where the two frames meet, columns 34 and 35) to column 50.
    s.send(b"\x1b[<0;36;11M\x1b[<32;45;11M\x1b[<32;51;11M\x1b[<0;51;11m");
    wait("the list wider", || (pane("finder")["pane_columns"].as_u64() > Some(45)).then_some(()));

    // l focuses the preview pane; Ctrl+p f makes it fullscreen, and h comes back to the list.
    s.send(b"l");
    wait("preview focused", || (pane("preview")["is_focused"] == true).then_some(()));
    s.send(b"\x10f");
    wait("preview fullscreen", || (pane("preview")["is_fullscreen"] == true).then_some(()));
    assert_eq!(pane("finder")["visible"], false);
    s.send(b"h");
    wait("back to the list", || {
        let list = pane("finder");
        (list["is_focused"] == true && list["visible"] == true).then_some(())
    });

    // Closing the preview closes the finder too.
    let id = pane("preview")["id"].as_u64().unwrap();
    env.tb(&["action", "close-pane", "-p", &format!("preview_{id}")]);
    wait("both gone", || env.panes().iter().all(|p| p["kind"] == "terminal").then_some(()));
    drop(s);

    // In a narrow terminal a finder tab shows its focused pane; l and h switch.
    let out = Command::new(TB)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TB_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(50, 24);
    s.wait_for("a.txt");
    wait("only the list", || (pane("preview")["visible"] == false && pane("finder")["visible"] == true).then_some(()));
    s.send(b"l");
    s.wait_for("first file");
    wait("only the preview", || (pane("finder")["visible"] == false).then_some(()));
}

/// A session saved when the finder was one pane comes back with the finder as two.
#[test]
fn old_one_pane_finder_comes_back_as_two() {
    let env = Env::new("e2e-oldfinder");
    let files = env.root.join("files");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("a.txt"), "a").unwrap();
    fs::write(files.join("b.txt"), "the second file\n").unwrap();
    let dir = files.to_str().unwrap();
    let saved = serde_json::json!({
        "version": 1, "frames": true, "active": 0,
        "tabs": [{
            "name": "finder", "auto_name": false, "zoomed": false, "sync": false, "focus": 0,
            "layout": {"Pane": {
                "kind": "Finder", "name": null, "cwd": dir, "command": null,
                "finder": {"dir": dir, "selected": "b.txt", "show_hidden": false, "focus_preview": true,
                           "scroll": 0, "editing": null}
            }}
        }]
    });
    let sessions = env.root.join("state/sessions");
    fs::create_dir_all(&sessions).unwrap();
    fs::write(sessions.join(format!("{}.json", env.session)), saved.to_string()).unwrap();

    let s = env.attach(100, 30);
    s.wait_for("the second file");
    let kinds: Vec<_> = env.panes().iter().map(|p| p["kind"].as_str().unwrap().to_owned()).collect();
    assert_eq!(kinds, ["finder", "preview"]);
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
