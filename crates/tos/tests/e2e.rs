//! End-to-end tests: run `tos attach` in a pseudo-terminal, with `alacritty_terminal` playing the user's
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

const TOS: &str = env!("CARGO_BIN_EXE_tos");
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

/// An isolated tos setup: its own folders, environment and session name.
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
        let sock = PathBuf::from(format!("/tmp/tost-{}-{session}", std::process::id()));
        let vars = vec![
            ("TOS_SOCKET_DIR".to_owned(), sock.display().to_string()),
            ("TOS_STATE_DIR".to_owned(), root.join("state").display().to_string()),
            ("XDG_DATA_HOME".to_owned(), root.join("data").display().to_string()),
            // dash, because on macOS /bin/sh is bash, and as a login shell (which tos starts there) bash reads
            // /etc/bashrc, which replaces PS1. Ubuntu and macOS both have /bin/dash.
            ("SHELL".to_owned(), "/bin/dash".to_owned()),
            ("PS1".to_owned(), "$ ".to_owned()),
            ("TOS_EDITOR".to_owned(), String::new()),
            // No requests to GitHub from tests; `update_notice_shows_a_newer_release` turns it back on.
            ("TOS_NO_UPDATE_CHECK".to_owned(), "1".to_owned()),
        ];
        Env { _dir: dir, root, sock, session: session.to_owned(), vars, cell: (8, 16) }
    }

    fn set(&mut self, key: &str, value: &str) {
        self.vars.retain(|(k, _)| k != key);
        self.vars.push((key.to_owned(), value.to_owned()));
    }

    /// Runs `tos -s <session> <args>` and returns its output; panics if it fails.
    fn tos(&self, args: &[&str]) -> String {
        let out = Command::new(TOS)
            .arg("-s")
            .arg(&self.session)
            .args(args)
            .envs(self.vars.iter().map(|(k, v)| (k, v)))
            .env_remove("TOS_SESSION")
            .output()
            .unwrap();
        assert!(out.status.success(), "tos {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Runs `tos <args>` (no `-s`) outside any session and returns its output, failed or not.
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(TOS)
            .args(args)
            .envs(self.vars.iter().map(|(k, v)| (k, v)))
            .env_remove("TOS_SESSION")
            .output()
            .unwrap()
    }

    /// Like `run`, but panics if it fails, and returns stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "tos {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn panes(&self) -> Vec<serde_json::Value> {
        serde_json::from_str(&self.tos(&["list-panes", "-j"])).unwrap()
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
            Command::new(TOS).args(["-s", &self.session, "kill"]).envs(self.vars.iter().map(|(k, v)| (k, v))).output();
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
            shell: Some(tty::Shell::new(TOS.into(), args.iter().map(|s| s.to_string()).collect())),
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
    assert!(env.tos(&["ls"]).contains("e2e-attach [1 tabs, 0 attached]"));

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
    env.set("TOS_EDITOR", &editor.display().to_string());

    env.tos(&["start"]);
    // `tos finder` from inside the session opens a tab without attaching.
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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

    // Enter in the list focuses the preview, whose caption says what Enter does there (the rest of it doesn't
    // fit beside the key list at 120 columns).
    s.send(b"\r");
    s.wait_for("Enter edit · o app");
    assert!(!s.text().contains("EDITING"), "Enter in the list started the editor");
    // Enter in the preview opens the editor inside the finder; when it exits, the preview shows the change.
    s.send(b"\r");
    s.wait_for("EDITING");
    s.wait_for("✎ notes.md");
    s.send(b"added by the editor\r");
    s.wait_for("added by the editor");
    s.wait_until("editor closed", |t| !t.contains("EDITING"));
    assert!(fs::read_to_string(files.join("notes.md")).unwrap().ends_with("added by the editor\n"));

    // ← back to the list. Filter: / then letters narrows the list.
    s.send(b"\x1b[D");
    s.wait_until("the list's caption", |t| t.lines().last().is_some_and(|l| l.ends_with("o app · ←→ focus")));
    s.send(b"/csv");
    let text = s.wait_for("/csv");
    assert!(text.contains("data.csv") && !text.contains("code.rs"), "filtered list:\n{text}");
    let dump = env.tos(&["dump-screen"]);
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
    env.tos(&["start"]);
    env.tos(&["action", "split", "down", "--", "/bin/sh", "-c", "sleep 0.3"]);
    assert_eq!(env.panes().len(), 2);
    wait("the split to close", || (env.panes().len() == 1).then_some(()));
    env.tos(&["action", "write-chars", "--enter", "exit"]);
    // zellij's name for `ls` works too.
    wait("the session to end", || (!env.tos(&["list-sessions"]).contains("e2e-exit")).then_some(()));
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
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = env.attach(100, 24);
    s.wait_for("first.txt");
    fs::write(files.join("second.txt"), "2").unwrap();
    s.wait_for("second.txt");
}

#[test]
fn finder_shows_hidden_files_until_dot() {
    let env = Env::new("e2e-hidden");
    let files = env.root.join("h");
    fs::create_dir_all(files.join("sub")).unwrap();
    fs::write(files.join(".hidden"), "secret\n").unwrap();
    fs::write(files.join("seen"), "plain\n").unwrap();
    fs::write(files.join("sub/.inner"), "x\n").unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = env.attach(100, 24);
    // The folder comes first and is previewed, with its hidden file.
    let text = s.wait_for(".inner");
    assert!(text.contains(".hidden") && text.contains("seen") && !text.contains("1 hidden"), "{text}");
    s.send(b".");
    let text = s.wait_until("hidden files left out", |t| t.contains("1 hidden") && t.contains("(empty folder)"));
    assert!(!text.contains(".hidden") && !text.contains(".inner") && text.contains("seen"), "{text}");
    s.send(b".");
    s.wait_until("hidden files back", |t| t.contains(".hidden") && t.contains(".inner") && !t.contains("1 hidden"));
}

#[test]
fn finder_menu_renames_and_deletes() {
    let mut env = Env::new("e2e-menu");
    // A trash that always fails, so Delete asks before deleting for good and the user's trash is never used.
    env.set("TOS_TRASH", "false");
    let files = env.root.join("m");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("keep.txt"), "keep\n").unwrap();
    fs::write(files.join("old.txt"), "old\n").unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = env.attach(100, 24);
    let text = s.wait_for(" old.txt");
    let (y, line) = text.lines().enumerate().find(|(_, l)| l.contains(" old.txt")).unwrap();
    let x = line.chars().position(|c| c == 'o').unwrap() + 1;
    // A right-click on old.txt selects it and opens the menu.
    s.send(format!("\x1b[<2;{x};{y1}M\x1b[<2;{x};{y1}m", y1 = y + 1).as_bytes());
    s.wait_for("Copy path");
    // Rename it: clear the field, type the new name, Enter.
    s.send(b"r");
    s.send(b"\x15new.txt\r");
    s.wait_until("the renamed file", |t| t.contains(" new.txt") && t.contains("renamed 'old.txt' to 'new.txt'"));
    assert!(files.join("new.txt").exists() && !files.join("old.txt").exists());
    // Delete it: the trash fails, so it asks; y deletes it for good.
    s.send(b"md");
    s.wait_for("Delete it for good? y / n");
    s.send(b"y");
    s.wait_for("deleted 'new.txt'");
    assert!(!files.join("new.txt").exists() && files.join("keep.txt").exists());
}

#[test]
fn finder_follows_markdown_links() {
    let env = Env::new("e2e-links");
    let files = env.root.join("l");
    fs::create_dir_all(files.join("sub")).unwrap();
    fs::write(files.join("a.md"), "# Page A\n\nSee [the bee](sub/b.md) here, or [gone](nope.md).\n").unwrap();
    fs::write(files.join("sub/b.md"), "# Bee Title\n\nBack to [a](../a.md).\n").unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    // Wide enough for the preview's key hints after the mode list.
    let s = env.attach(160, 30);
    s.wait_for("sub/");
    s.send(b"j");
    let text = s.wait_for("See the bee here");
    // Where a word is on screen: (column, row).
    let at = |text: &str, word: &str| {
        let (row, line) = text.lines().enumerate().find(|(_, l)| l.contains(word)).unwrap();
        let col = line[..line.find(word).unwrap()].chars().count();
        (col as u16, row as u16)
    };

    // A click on the link previews sub/b.md, and the list moves to sub/ with it selected.
    let (x, y) = at(&text, "the bee");
    s.send(&click(x + 2, y));
    let text = s.wait_for("Bee Title");
    assert!(text.contains("l/sub"), "the list shows sub/:\n{text}");
    s.wait_for("Backspace back · ←→ focus");

    // Backspace (the click focused the preview) goes back to a.md.
    s.send(b"\x7f");
    let text = s.wait_for("See the bee here");
    assert!(!text.contains("Bee Title"), "back on a.md:\n{text}");

    // A link to a missing file says so.
    let (x, y) = at(&text, "gone");
    s.send(&click(x, y));
    s.wait_for("no such file: nope.md");
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
    env.set("TOS_EDITOR", &editor.display().to_string());
    let s = env.attach(100, 30);
    s.wait_for("$");
    let screen = |id: u64| env.tos(&["dump-screen", "-p", &id.to_string()]);

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
fn tabs_are_named_after_the_working_folder() {
    let mut env = Env::new("e2e-names");
    let home = env.root.join("home");
    let proj = home.join("proj");
    fs::create_dir_all(&proj).unwrap();
    env.set("HOME", home.to_str().unwrap());
    let s = env.attach(100, 30);
    s.wait_for("$");
    s.send(format!("cd {}\r", proj.display()).as_bytes());
    s.wait_for(" 1 proj ");
    // The frame follows the running program. `cat` starts two seconds after the last output and prints
    // nothing, so the frame must update without any output to trigger it. The tab keeps the folder.
    // Two seconds, not less: a slow CI runner once missed the "sleep" frame in half a second.
    s.send(b"sleep 2; cat\r");
    s.wait_for("┌ sleep ");
    s.wait_for("┌ cat ");
    s.wait_for(" 1 proj ");
    s.send(b"\x04");
    s.wait_for("┌ dash ");
    s.send(b"cd\r");
    s.wait_for(" 1 ~ ");
    // A named tab keeps its name when the folder changes; removing the name follows the folder again.
    s.send(b"\x14rwork\r");
    s.wait_for(" 1 work ");
    s.send(format!("cd {}; echo mo''ved\r", proj.display()).as_bytes());
    s.wait_for("moved");
    s.wait_for(" 1 work ");
    env.tos(&["action", "undo-rename-tab"]);
    s.wait_for(" 1 proj ");
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
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    s.wait_for("a.txt");
    s.send(b"j");
    s.wait_for("┌ b.txt ");

    env.tos(&["kill"]);
    s.wait_exit();
    let ls = env.tos(&["ls"]);
    assert!(ls.contains("e2e-persist (EXITED - attach to restore)"), "{ls}");

    // Attaching brings it back: tabs, names, folders, the finder's selection, and the command at the prompt.
    // The finder tab stays first, without a number.
    let s = env.attach(100, 30);
    s.wait_for(" finder   1 work ");
    s.wait_for("┌ b.txt ");
    let panes = env.panes();
    // The finder is two panes, its file list and its preview, linked again.
    let kinds: Vec<_> = panes.iter().map(|p| p["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["finder", "preview", "terminal", "terminal"], "{panes:?}");
    // The kernel gives the real path: on macOS the temp folder's /var is /private/var.
    assert_eq!(panes[2]["cwd"].as_str(), Some(proj.canonicalize().unwrap().to_str().unwrap()));
    let second = panes[3]["id"].as_u64().unwrap().to_string();
    wait("sleep typed, not run", || env.tos(&["dump-screen", "-p", &second]).contains("$ sleep 300").then_some(()));
    assert_eq!(panes[3]["title"].as_str(), Some("dash"), "the shell, not sleep, is running");

    // `tos delete` refuses a running session without --force, then forgets it.
    let out = Command::new(TOS)
        .args(["-s", &env.session, "delete"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(!out.status.success());
    env.tos(&["delete", "--force"]);
    s.wait_exit();
    wait("forgotten", || env.tos(&["ls"]).contains("no sessions").then_some(()));
}

/// Finder tabs stay on the left without numbers, a new one after those already there; other tabs count from 1.
#[test]
fn finder_tabs_stay_left() {
    let env = Env::new("e2e-finder-tabs");
    for d in ["one", "two"] {
        fs::create_dir(env.root.join(d)).unwrap();
    }
    let s = env.attach(120, 30);
    s.wait_for("$");
    let bar = |want: &str| s.wait_until(want, |t| t.lines().next().is_some_and(|l| l.starts_with(want)));
    let active = || {
        let tabs = json(&env.tos(&["action", "list-tabs", "-j"]));
        let tab = tabs.as_array().unwrap().iter().find(|t| t["active"] == true).cloned().unwrap();
        (tab["position"].as_u64().unwrap(), tab["name"].as_str().unwrap().to_owned())
    };

    // Two shell tabs, in one and two.
    s.send(format!("cd {}\r", env.root.join("one").display()).as_bytes());
    bar(" 1 one ");
    s.send(b"\x14n");
    s.send(format!("cd {}\r", env.root.join("two").display()).as_bytes());
    bar(" 1 one   2 two ");

    // Ctrl+T f opens a finder on two at the far left; the numbers start after it.
    s.send(b"\x14f");
    bar(" finder   1 one   2 two ");
    // Ctrl+T 1 goes to the tab numbered 1. A second finder from there goes after the first, and both show
    // their folder.
    s.send(b"\x141");
    wait("on one", || (active() == (1, "one".to_owned())).then_some(()));
    s.send(b"\x14f");
    bar(" finder: two   finder: one   1 one   2 two ");
    assert_eq!(active(), (1, "finder: one".to_owned()), "positions count the finders");

    // go-to-tab N uses the numbers on the bar.
    env.tos(&["action", "go-to-tab", "2"]);
    wait("on two", || (active() == (3, "two".to_owned())).then_some(()));
    let out = env.run(&["-s", &env.session, "action", "go-to-tab", "3"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && err.contains("no tab 3 (there are 2 numbered tabs"), "{err}");

    // Alt+I moves two left, then wraps to the end of the numbered tabs rather than passing the finders.
    s.send(b"\x1bi");
    bar(" finder: two   finder: one   1 two   2 one ");
    s.send(b"\x1bi");
    bar(" finder: two   finder: one   1 one   2 two ");

    // Ctrl+T 0 goes to the first finder.
    s.send(b"\x140");
    wait("on the first finder", || (active() == (0, "finder: two".to_owned())).then_some(()));
}

/// Programs in a pane see the real terminal's cell size, so tools like chafa keep images in shape.
#[test]
fn panes_get_the_terminal_cell_size() {
    let mut env = Env::new("e2e-cell");
    env.set("TOS_IMAGES", "blocks");
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
    let out = Command::new(TOS)
        .args(["ls"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", "zj-b")
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

/// Parses JSON output from `tos`.
fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// zellij's tab actions: ids, names, lists, going to, renaming, moving, syncing and closing tabs.
#[test]
fn zellij_tab_actions() {
    let env = Env::new("e2e-tab-actions");
    env.tos(&["start"]);
    // new-tab prints the new tab's id; list-tabs prints zellij's columns.
    let two: u32 = env.tos(&["action", "new-tab", "-n", "two"]).trim().parse().expect("a tab id");
    let three: u32 = env.tos(&["action", "new-tab", "--name", "three"]).trim().parse().unwrap();
    let text = env.tos(&["action", "list-tabs"]);
    assert!(text.starts_with("TAB_ID  POSITION  NAME\n"), "{text}");
    assert!(text.contains(&format!("{two}  1  two")), "{text}");
    assert_eq!(env.tos(&["action", "query-tab-names"]).lines().skip(1).collect::<Vec<_>>(), ["two", "three"]);
    let tabs = json(&env.tos(&["action", "list-tabs", "-j"]));
    for field in ["tab_id", "position", "name", "active", "is_fullscreen_active", "is_sync_panes_active"] {
        assert!(tabs[0].get(field).is_some(), "list-tabs -j lacks {field}: {tabs}");
    }
    assert_eq!(tabs[2]["active"], true, "the newest tab is active");
    // Going to tabs by id and by name, and current-tab-info.
    env.tos(&["action", "go-to-tab-by-id", &two.to_string()]);
    assert_eq!(json(&env.tos(&["action", "current-tab-info", "-j"]))["name"], "two");
    env.tos(&["action", "go-to-tab-name", "three"]);
    assert_eq!(json(&env.tos(&["action", "current-tab-info", "-j"]))["tab_id"], three);
    assert!(
        Command::new(TOS)
            .args(["-s", &env.session, "action", "go-to-tab-name", "nope"])
            .envs(env.vars.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap()
            .status
            .code()
            != Some(0)
    );
    env.tos(&["action", "go-to-tab-name", "-c", "made"]);
    assert_eq!(json(&env.tos(&["action", "current-tab-info", "-j"]))["name"], "made");
    // Renaming by id, and undoing it (the name follows the program again).
    env.tos(&["action", "rename-tab-by-id", &two.to_string(), "renamed"]);
    assert!(env.tos(&["action", "query-tab-names"]).contains("renamed"));
    env.tos(&["action", "undo-rename-tab", "-t", &two.to_string()]);
    assert!(!env.tos(&["action", "query-tab-names"]).contains("renamed"));
    // Moving a tab by id keeps the active tab active; sync by id.
    env.tos(&["action", "move-tab", "left", "-t", &three.to_string()]);
    let tabs = json(&env.tos(&["action", "list-tabs", "-j"]));
    let pos = |id: u32| tabs.as_array().unwrap().iter().find(|t| t["tab_id"] == id).unwrap()["position"].clone();
    assert_eq!(pos(three), 1, "{tabs}");
    assert_eq!(json(&env.tos(&["action", "current-tab-info", "-j"]))["name"], "made");
    env.tos(&["action", "toggle-active-sync-tab", "-t", &three.to_string()]);
    let tabs = json(&env.tos(&["action", "list-tabs", "-j"]));
    assert_eq!(tabs.as_array().unwrap().iter().find(|t| t["tab_id"] == three).unwrap()["is_sync_panes_active"], true);
    // Next and previous wrap around; closing by id.
    env.tos(&["action", "go-to-next-tab"]);
    assert_eq!(json(&env.tos(&["action", "current-tab-info", "-j"]))["position"], 0);
    env.tos(&["action", "go-to-previous-tab"]);
    assert_eq!(json(&env.tos(&["action", "current-tab-info", "-j"]))["name"], "made");
    env.tos(&["action", "close-tab-by-id", &three.to_string()]);
    env.tos(&["action", "close-tab"]);
    assert_eq!(json(&env.tos(&["action", "list-tabs", "-j"])).as_array().unwrap().len(), 2);
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
    env.set("TOS_EDITOR", editor.to_str().unwrap());
    let s = env.attach(120, 40);
    s.wait_for("$");
    let first = env.focused();
    let side = env.tos(&["action", "new-pane", "-d", "right", "-n", "side", "--", "sleep", "300"]);
    let side_id: u64 = side.trim().strip_prefix("terminal_").expect("terminal_N").parse().unwrap();
    s.wait_for("┌ side ");
    let ran = Command::new(TOS)
        .args(["-s", &env.session, "run", "-d", "down", "--", "sleep", "301"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&ran.stdout).trim().starts_with("terminal_"), "{ran:?}");
    // zellij's text columns and JSON fields.
    let text = env.tos(&["action", "list-panes", "-c"]);
    assert!(text.starts_with("PANE_ID  TYPE  TITLE  COMMAND  CWD\n"), "{text}");
    assert!(text.contains(&format!("terminal_{side_id}  terminal")), "{text}");
    let panes = json(&env.tos(&["action", "list-panes", "-j"]));
    let pane = panes.as_array().unwrap().iter().find(|p| p["id"] == side_id).unwrap().clone();
    for field in
        ["is_plugin", "is_focused", "title", "tab_id", "tab_name", "pane_command", "pane_cwd", "terminal_command"]
    {
        assert!(pane.get(field).is_some(), "list-panes -j lacks {field}: {pane}");
    }
    assert_eq!(pane["terminal_command"], "sleep 300");
    // Focus by id (bare and terminal_N), rename and undo, fullscreen by id.
    env.tos(&["action", "focus-pane-id", &first.to_string()]);
    assert_eq!(env.focused(), first);
    env.tos(&["action", "focus-pane-id", &format!("terminal_{side_id}")]);
    assert_eq!(env.focused(), side_id);
    env.tos(&["action", "rename-pane", "-p", &first.to_string(), "main-shell"]);
    s.wait_for("┌ main-shell ");
    env.tos(&["action", "undo-rename-pane", "-p", &first.to_string()]);
    wait("name gone", || (!s.text().contains("main-shell")).then_some(()));
    env.tos(&["action", "toggle-fullscreen", "-p", &first.to_string()]);
    let panes = json(&env.tos(&["action", "list-panes", "-j"]));
    assert!(panes.as_array().unwrap().iter().any(|p| p["id"] == first && p["is_fullscreen"] == true), "{panes}");
    env.tos(&["action", "toggle-fullscreen"]);
    // Colours, as zellij writes them.
    env.tos(&["action", "set-pane-color", "-p", &first.to_string(), "--bg", "rgb:10/20/30"]);
    let colour = |env: &Env| {
        json(&env.tos(&["action", "list-panes", "-j"])).as_array().unwrap().iter().find(|p| p["id"] == first).unwrap()
            ["default_bg"]
            .clone()
    };
    assert_eq!(colour(&env), "#102030");
    env.tos(&["action", "set-pane-color", "-p", &first.to_string(), "--reset"]);
    assert_eq!(colour(&env), serde_json::Value::Null);
    // edit opens the editor at a line, in a new pane.
    let file = env.root.join("notes.txt");
    fs::write(&file, "a\nb\nc\n").unwrap();
    let edited = env.tos(&["action", "edit", "-l", "3", file.to_str().unwrap()]);
    assert!(edited.trim().starts_with("terminal_"), "{edited}");
    let got = wait("editor started", || fs::read_to_string(&args).ok().filter(|a| !a.is_empty()));
    assert_eq!(got.trim(), format!("+3 {}", file.display()));
    // Close by id.
    env.tos(&["action", "close-pane", "-p", &format!("terminal_{side_id}")]);
    assert!(!json(&env.tos(&["action", "list-panes", "-j"])).as_array().unwrap().iter().any(|p| p["id"] == side_id));
}

/// zellij's input, scrolling and session actions.
#[test]
fn zellij_input_and_session_actions() {
    let env = Env::new("e2e-input");
    let s = env.attach(100, 30);
    s.wait_for("$");
    let id = env.focused().to_string();
    // write-chars and send-keys; write takes bytes ("echo bytes" and Enter); paste.
    env.tos(&["action", "write-chars", "echo one"]);
    env.tos(&["action", "send-keys", "Enter"]);
    s.wait_for("one");
    env.tos(&["action", "write", "-p", &id, "101", "99", "104", "111", "32", "98", "121", "116", "101", "115", "13"]);
    s.wait_for("bytes");
    env.tos(&["action", "paste", "echo pasted"]);
    env.tos(&["action", "send-keys", "Enter"]);
    s.wait_for("pasted");
    // dump-screen: to a file, and with colours.
    let out = env.root.join("screen.txt");
    env.tos(&["action", "dump-screen", "--path", out.to_str().unwrap()]);
    assert!(fs::read_to_string(&out).unwrap().contains("bytes"));
    env.tos(&["action", "write-chars", "--enter", "printf '\\033[31mred\\033[0m\\n'"]);
    s.wait_for("red");
    assert!(env.tos(&["action", "dump-screen", "-a"]).contains("\x1b[0;31mred"));
    // Scrolling shows in the frame; clear empties the pane.
    env.tos(&["action", "write-chars", "--enter", "seq 1 200"]);
    s.wait_for("200");
    env.tos(&["action", "scroll-to-top"]);
    s.wait_for("SCROLL:");
    env.tos(&["action", "scroll-to-bottom"]);
    wait("back at the bottom", || (!s.text().contains("SCROLL:")).then_some(()));
    env.tos(&["action", "clear"]);
    wait("cleared", || (!env.tos(&["action", "dump-screen"]).contains("200")).then_some(()));
    // switch-mode shows in the status bar.
    env.tos(&["action", "switch-mode", "locked"]);
    s.wait_for("LOCKED");
    env.tos(&["action", "switch-mode", "normal"]);
    // list-clients shows the attached client.
    let clients = env.tos(&["action", "list-clients"]);
    assert!(clients.starts_with("CLIENT_ID  PANE_ID  RUNNING_COMMAND\n") && clients.lines().count() == 2, "{clients}");
    // save-session writes the saved layout; dump-layout prints it.
    env.tos(&["action", "save-session"]);
    assert!(env.root.join(format!("state/sessions/{}.json", env.session)).exists());
    assert!(json(&env.tos(&["action", "dump-layout"]))["tabs"].is_array());
    // rename-session: the session answers under its new name; detach ends the client.
    let new = format!("{}-renamed", env.session);
    env.tos(&["action", "rename-session", &new]);
    let ls = env.tos(&["ls"]);
    assert!(ls.contains(&format!("{new} [")), "{ls}");
    let detach = Command::new(TOS)
        .args(["-s", &new, "action", "detach"])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap();
    assert!(detach.status.success(), "{detach:?}");
    s.wait_exit();
    let _ = Command::new(TOS).args(["-s", &new, "kill"]).envs(env.vars.iter().map(|(k, v)| (k, v))).output();
}

/// A `tos subscribe` running in the background; its output lines are collected.
struct Subscriber {
    child: std::process::Child,
    lines: Arc<Mutex<Vec<String>>>,
}

impl Subscriber {
    fn start(env: &Env, args: &[&str]) -> Subscriber {
        use std::io::BufRead;
        let mut child = Command::new(TOS)
            .args(["-s", &env.session, "subscribe"])
            .args(args)
            .envs(env.vars.iter().map(|(k, v)| (k, v)))
            .env_remove("TOS_SESSION")
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

/// `tos subscribe`: session events as JSON lines, and a pane's text as it changes.
#[test]
fn subscribe_reports_events() {
    let env = Env::new("e2e-subscribe");
    env.tos(&["start"]);
    let first = env.focused().to_string();
    // The first delivery is the subscribed pane's text, so once it arrives, events are flowing.
    let events = Subscriber::start(&env, &["-f", "json", "-p", &first]);
    events.wait_event(serde_json::json!({"event": "pane_text"}));
    let tab: u64 = env.tos(&["action", "new-tab", "-n", "two"]).trim().parse().unwrap();
    events.wait_event(serde_json::json!({"event": "tab_opened", "tab_id": tab, "name": "two"}));
    events.wait_event(serde_json::json!({"event": "tab_focused", "tab_id": tab}));
    env.tos(&["action", "rename-tab", "renamed"]);
    events.wait_event(serde_json::json!({"event": "tab_renamed", "tab_id": tab, "name": "renamed"}));
    // A pane that exits reports its exit status.
    let pane = env.tos(&["action", "new-pane", "--", "sh", "-c", "sleep 0.5; exit 3"]);
    let id: u64 = pane.trim().strip_prefix("terminal_").unwrap().parse().unwrap();
    events.wait_event(serde_json::json!({"event": "pane_opened", "pane_id": id}));
    events.wait_event(serde_json::json!({"event": "pane_closed", "pane_id": id, "exit_status": 3}));
    env.tos(&["action", "switch-mode", "locked"]);
    events.wait_event(serde_json::json!({"event": "mode_changed", "mode": "locked"}));
    env.tos(&["action", "switch-mode", "normal"]);
    env.tos(&["action", "close-tab"]);
    events.wait_event(serde_json::json!({"event": "tab_closed", "tab_id": tab}));
    // Raw format: the pane's text, printed when it changes.
    let text = Subscriber::start(&env, &["-p", &format!("terminal_{first}")]);
    env.tos(&["action", "write-chars", "-p", &first, "--enter", "echo hello-sub"]);
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
    env.tos(&["action", "set-status", "--id", "prbot", "PR #487 needs review"]);
    env.tos(&["action", "set-status", "--id", "clock", "12:00"]);
    let bottom = |s: &Screen| s.text().lines().last().unwrap_or_default().to_owned();
    wait("both segments", || {
        (bottom(&s).contains("PR #487 needs review") && bottom(&s).contains("12:00")).then_some(())
    });
    // Setting an id again replaces its text, in place.
    env.tos(&["action", "set-status", "--id", "prbot", "PR #490 too"]);
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
    env.tos(&["action", "set-status", "--id", "brief", "--ttl", "1", "gone soon"]);
    s.wait_for("gone soon");
    wait("expired", || (!bottom(&s).contains("gone soon")).then_some(()));
    env.tos(&["action", "clear-status", "--id", "clock"]);
    wait("clock cleared", || (!bottom(&s).contains("12:00")).then_some(()));
    env.tos(&["action", "clear-status"]);
    wait("all cleared", || (!bottom(&s).contains("PR #490")).then_some(()));
}

/// `switch-session` moves the attached client to another session; `tos watch` shows a session read-only.
#[test]
fn switch_session_and_watch() {
    let env = Env::new("e2e-switch");
    let other = format!("{}-other", env.session);
    let s = env.attach(100, 30);
    s.wait_for("$");
    env.tos(&["action", "switch-session", &other]);
    wait("switched", || {
        let ls = env.tos(&["ls"]);
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
    env.set("TOS_CONFIG", config.to_str().unwrap());
    let s = env.attach(100, 30);
    s.wait_for("$");
    // Ctrl+t then p: the bound Run opens a named pane, and the binding switches back to normal mode.
    s.send(b"\x14p");
    s.wait_for("┌ bound ");
    s.wait_for("bound-run");
    s.wait_for("<g> LOCK");
    // Alt+y types into the focused pane.
    env.tos(&["action", "focus-previous-pane"]);
    // A line the command printed (not the echoed command line).
    let printed = |word: &'static str| move |t: &str| t.lines().any(|l| unframe(l).starts_with(word));
    s.send(b"\x1by");
    s.wait_until("from-binding printed", printed("from-binding"));
    // reload-config lists what it skipped; the bindings it could load still work.
    fs::write(&config, "keybinds {\n  normal {\n    bind \"Alt y\" { WriteChars \"echo reloaded\"; Write 13; }\n    bind \"Alt u\" { ToggleFloatingPanes; }\n  }\n}\n")
        .unwrap();
    let warnings = env.tos(&["action", "reload-config"]);
    assert!(
        warnings.contains("line 4: skipped this binding: tos has no action 'ToggleFloatingPanes' yet"),
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
        .env("TOS", TOS)
        .env_remove("TOS_SESSION")
        .spawn()
        .unwrap();
    let bottom = |s: &Screen| s.text().lines().last().unwrap_or_default().to_owned();
    // The extension subscribes in the background, so try until it has seen a failure.
    let shown = (0..10).any(|_| {
        env.tos(&["action", "new-pane", "--", "sh", "-c", "sleep 0.3; exit 2"]);
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
    env.set("TOS_IMAGES", protocol);
    let files = env.root.join("pics");
    fs::create_dir_all(&files).unwrap();
    let img = image::RgbImage::from_fn(200, 100, |x, _| image::Rgb([if x < 100 { 255 } else { 0 }, 0, 128]));
    img.save(files.join("pic.png")).unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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
    env.set("TOS_IMAGES", "kitty");
    let files = env.root.join("pdfs");
    fs::create_dir_all(&files).unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../finder-core/tests/data");
    fs::copy(data.join("two-pages.pdf"), files.join("a.pdf")).unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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

/// Opens a finder on `dir` in the test's session, which must be running.
fn open_finder(env: &Env, dir: &Path) {
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", dir.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// The program `o` runs to open a file in its default app.
const OPENER: &str = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };

/// Writes a stand-in for a program into `bin`: it writes its arguments and `DISPLAY` to `<bin>/<name>.out`.
fn fake_program(bin: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let out = bin.join(format!("{name}.out"));
    // Only shell built-ins, as the test's PATH may hold nothing else.
    let script =
        format!("#!/bin/sh\n{{ printf '%s\\n' \"$@\"; printf 'DISPLAY=%s\\n' \"$DISPLAY\"; }} > '{}'\n", out.display());
    let path = bin.join(name);
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    out
}

/// What a fake program wrote, once it has run.
fn ran(out: &Path) -> String {
    wait("the program to run", || fs::read_to_string(out).ok().filter(|t| t.contains("DISPLAY=")))
}

#[test]
fn video_preview_and_open() {
    // Like finder-core's video tests, this needs ffmpeg.
    if Command::new("ffprobe").arg("-version").output().is_err() {
        eprintln!("skipped: ffmpeg isn't installed");
        return;
    }
    let mut env = Env::new("e2e-video");
    let bin = env.root.join("bin");
    let files = env.root.join("clips");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&files).unwrap();
    let opener = fake_program(&bin, OPENER);
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../finder-core/tests/data");
    fs::copy(data.join("two-seconds.mp4"), files.join("clip.mp4")).unwrap();
    env.set("PATH", &format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default()));
    // The server starts with one display; the client that presses o has another.
    env.set("DISPLAY", ":41");
    env.tos(&["start"]);
    open_finder(&env, &files);
    env.set("DISPLAY", ":42");
    let s = env.attach(100, 30);

    // The header first, then the frame, drawn as blocks in this terminal.
    s.wait_for("video · 0:02 · 64×48 · h264");
    s.wait_until("the frame drawn", |t| t.contains('▀') && !t.contains("loading frame"));

    // Enter only focuses the preview; it plays and opens nothing.
    s.send(b"\r");
    sleep(Duration::from_millis(300));
    assert!(!opener.exists() && !s.text().contains("opening"), "{}", s.text());
    // o opens it in the default app, on the client's display.
    s.send(b"o");
    s.wait_for("opening clip.mp4 in its default app");
    let clip = files.join("clip.mp4");
    assert_eq!(ran(&opener), format!("{}\nDISPLAY=:42\n", clip.display()));
}

#[test]
fn spreadsheets_word_and_powerpoint() {
    let env = Env::new("e2e-office");
    let files = env.root.join("office");
    fs::create_dir_all(&files).unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../finder-core/tests/data");
    fs::copy(data.join("sheet.xlsx"), files.join("a-budget.xlsx")).unwrap();
    fs::copy(data.join("office.docx"), files.join("b-trip.docx")).unwrap();
    fs::copy(data.join("deck.pptx"), files.join("c-deck.pptx")).unwrap();
    env.tos(&["start"]);
    open_finder(&env, &files);
    let s = env.attach(140, 30);

    // The first sheet, with formulas' values and dates.
    s.wait_for("xlsx · sheet 1/2 Budget · 3 rows");
    s.wait_until("the table", |t| t.contains("Apples") && t.contains("2026-11-07") && t.contains("1.5"));
    // In the preview, ] shows the next sheet.
    s.send(b"l]");
    s.wait_for("sheet 2/2 Notes");
    s.wait_for("Second sheet");
    // Past the last sheet nothing happens, and no message takes the caption's place.
    s.send(b"]");
    sleep(Duration::from_millis(200));
    let bar = s.text().lines().last().unwrap_or_default().to_owned();
    assert!(bar.ends_with("Enter grid · o app · [ ] sheet · ←→ focus"), "status bar: {bar}");

    // Enter opens that sheet in the grid, read-only; [ goes back a sheet there too.
    s.send(b"\r");
    s.wait_for("▦ a-budget.xlsx");
    s.wait_for("sheet 2/2 Notes · row 0/0 · col 1 · read-only");
    s.send(b"e");
    s.wait_for("read-only: tos can't save .xlsx");
    s.send(b"[");
    s.wait_until("sheet 1 in the grid", |t| t.contains("sheet 1/2 Budget · row 0/2") && t.contains("Pears"));
    s.send(b"q");
    s.wait_until("grid closed", |t| !t.contains("▦ a-budget.xlsx"));

    // Word and PowerPoint as Markdown.
    s.send(b"hj");
    s.wait_until("the docx", |t| t.contains("Trip report") && t.contains("We went to Lisbon"));
    // tos can't edit a docx: its preview's caption has no Enter, and Enter there says so.
    s.send(b"\r");
    s.wait_for("o app · [ ] page · ←→ focus");
    assert!(!s.text().lines().last().unwrap_or_default().contains("Enter"), "{}", s.text());
    s.send(b"\r");
    s.wait_for("tos can't edit .docx; o opens it in its default app");
    // ← back to the list, and on to the pptx.
    s.send(b"\x1b[Dj");
    s.wait_for("pptx · 3 slides");
    s.wait_for("2 · Results");
    s.wait_for("Mostly in Europe");
}

#[test]
fn video_without_ffmpeg_an_opener_or_a_display() {
    let mut env = Env::new("e2e-video-none");
    let bin = env.root.join("bin");
    let files = env.root.join("clips");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("clip.mp4"), "not really a video").unwrap();
    // Nothing on PATH: no ffmpeg, and nothing to open files with.
    env.set("PATH", bin.to_str().unwrap());
    env.set("DISPLAY", ":42");
    env.tos(&["start"]);
    open_finder(&env, &files);
    let s = env.attach(100, 30);
    s.wait_for("video preview needs ffmpeg");
    s.send(b"o");
    let missing = if cfg!(target_os = "macos") {
        "o needs macOS's open command, which isn't on PATH"
    } else {
        "o needs xdg-open (package xdg-utils)"
    };
    s.wait_for(missing);

    // A client logged in over SSH, without X forwarding, has nowhere to show it.
    let opened = fake_program(&bin, OPENER);
    env.set("DISPLAY", "");
    env.set("WAYLAND_DISPLAY", "");
    env.set("SSH_CONNECTION", "10.0.0.2 50000 10.0.0.1 22");
    let ssh = env.attach(100, 30);
    ssh.wait_for("video preview needs ffmpeg");
    ssh.send(b"o");
    ssh.wait_for("no display to open it on: you're attached over SSH");
    sleep(Duration::from_millis(200));
    assert!(!opened.exists(), "the opener ran");
}

/// The finder's file list and preview are two real panes: borders drag, the preview goes fullscreen, closing
/// one closes both, and a narrow tab shows one at a time.
#[test]
fn finder_is_two_panes() {
    let env = Env::new("e2e-twopane");
    let files = env.root.join("files");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("a.txt"), "first file\n").unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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
    env.tos(&["action", "close-pane", "-p", &format!("preview_{id}")]);
    wait("both gone", || env.panes().iter().all(|p| p["kind"] == "terminal").then_some(()));
    drop(s);

    // In a narrow terminal a finder tab shows its focused pane; l and h switch.
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let s = env.attach(100, 30);
    s.wait_for("data.csv");
    // Enter focuses the preview; Enter there opens the grid.
    s.send(b"\r\r");
    s.wait_for("▦ data.csv");
    s.wait_for("row 0/2");
    // v starts a range at apple; down and right stretch it to pear's qty; y copies it as tab-separated text.
    s.send(b"jvjl");
    // At 100 columns only the caption's first hint fits beside the key list.
    s.wait_until("the range's caption", |t| t.lines().last().is_some_and(|l| l.ends_with(" y copy")));
    s.send(b"y");
    let clip = wait("clipboard", || s.proxy.clipboard.lock().unwrap().last().cloned());
    assert_eq!(clip, "apple\t3\npear\t10");
    s.wait_for("copied 2 rows × 2 columns");
    // Back to the top left.
    s.send(b"g0");
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

/// ← and → move focus (and up and into folders), ↓ ↑ move and scroll, [ ] page, and the captions say so in the
/// room a 130-column status bar leaves.
#[test]
fn finder_arrows_pages_and_captions() {
    let env = Env::new("e2e-arrows");
    let files = env.root.join("files");
    fs::create_dir_all(files.join("sub")).unwrap();
    fs::write(files.join("sub/inner.txt"), "inside the folder\n").unwrap();
    let long: String = (1..=300).map(|i| format!("line {i}\n")).collect();
    fs::write(files.join("a-long.txt"), long).unwrap();
    fs::write(files.join("b-page.html"), "<h1>Hello page</h1><p>Some words.</p>").unwrap();
    env.tos(&["start"]);
    open_finder(&env, &files);
    let s = env.attach(130, 30);
    let bar = |s: &Screen| s.text().lines().last().unwrap_or_default().to_owned();
    let shows = |t: &str, line: &str| t.lines().any(|l| unframe(l).ends_with(line));

    // sub/ comes first. → goes into it, ← comes back up.
    s.wait_for("inner.txt");
    s.send(b"\x1b[C");
    s.wait_until("inside sub/", |t| t.contains("files/sub") && t.contains("inside the folder"));
    s.send(b"\x1b[D");
    s.wait_until("back up", |t| t.contains("a-long.txt"));

    // ↓ to the long file; → focuses its preview, where ↓ scrolls and ] and [ page.
    s.send(b"\x1b[B");
    s.wait_until("the long file", |t| shows(t, "line 1"));
    s.send(b"\x1b[C");
    s.wait_until("the preview's caption", |_| bar(&s).ends_with("Enter edit · o app · [ ] page"));
    s.send(b"\x1b[B\x1b[B\x1b[B");
    s.wait_until("scrolled 3 lines", |t| shows(t, "line 4") && !shows(t, "line 3"));
    s.send(b"]");
    s.wait_until("a page down", |t| !shows(t, "line 4") && shows(t, "line 40"));
    s.send(b"[");
    s.wait_until("a page up", |t| shows(t, "line 4") && !shows(t, "line 3"));
    // ↑ scrolls back; ← returns to the list.
    s.send(b"\x1b[A");
    s.wait_until("scrolled up", |t| shows(t, "line 3"));
    s.send(b"\x1b[D");

    // At 130 columns the list's caption keeps its first hints, Enter-less.
    s.send(b"\x1b[B");
    s.wait_for("Hello page");
    s.wait_until("the list's caption", |_| bar(&s).ends_with("o app · ←→ focus · / filter"));
}

/// Prints what a split session and the finder look like. Run with:
/// `cargo test -p tos --test e2e show_screens -- --ignored --nocapture`
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
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", docs.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
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
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", sheet.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    s.wait_for("fruit.csv");
    s.send(b"\r\rjl");
    sleep(Duration::from_millis(300));
    println!("{}\n{}", "=".repeat(110), s.text());
}

/// Opens a file in the real Neovim (needs `nvim`), types, and closes without saving. Run with:
/// `cargo test -p tos --test e2e real_neovim -- --ignored --nocapture`
#[test]
#[ignore]
fn real_neovim() {
    let mut env = Env::new("e2e-nvim");
    env.vars.retain(|(k, _)| k != "TOS_EDITOR");
    let files = env.root.join("n");
    fs::create_dir_all(&files).unwrap();
    fs::write(files.join("a.md"), "# Hello\n").unwrap();
    env.tos(&["start"]);
    let out = Command::new(TOS)
        .args(["-s", &env.session, "finder", files.to_str().unwrap()])
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("TOS_SESSION", &env.session)
        .output()
        .unwrap();
    assert!(out.status.success());
    let s = env.attach(110, 26);
    s.wait_for("a.md");
    s.send(b"\r\r");
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

/// The target name the installer picks on this machine.
fn release_target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        other => panic!("no release target for {other:?}"),
    }
}

/// A release folder like GitHub's: install.sh, and a tarball whose `tos` is a script printing `version`.
fn fake_release(dir: &Path, version: &str) {
    let target = release_target();
    let folder = format!("tos-{version}-{target}");
    fs::create_dir_all(dir.join(&folder)).unwrap();
    let bin = dir.join(&folder).join("tos");
    fs::write(&bin, format!("#!/bin/sh\necho 'tos {version} (extension API 2)'\n")).unwrap();
    Command::new("chmod").arg("755").arg(&bin).status().unwrap();
    let tarball = format!("tos-{target}.tar.gz");
    let script = format!(
        "tar -czf {tarball} {folder} && (sha256sum {tarball} 2>/dev/null || shasum -a 256 {tarball}) > {tarball}.sha256"
    );
    assert!(Command::new("sh").arg("-c").arg(script).current_dir(dir).status().unwrap().success());
    fs::copy(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/install.sh"), dir.join("install.sh")).unwrap();
}

/// Stands in for GitHub: `/releases/latest` redirects to the tag of `version`. Returns the releases URL.
fn fake_github(version: &str) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let location = format!("http://127.0.0.1:{port}/releases/tag/v{version}");
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut buf);
            let reply =
                format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = std::io::Write::write_all(&mut stream, reply.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}/releases")
}

/// Lays this build out as the installer would: `<data>/termos/versions/<version>/tos`, `<root>/bin/tos` a symlink
/// to it, and `install-dir` noting `<root>/bin`. Returns `<root>/bin/tos`.
fn install_like_the_installer(env: &Env) -> PathBuf {
    let data = env.root.join("data/termos");
    let installed = data.join("versions").join(env!("CARGO_PKG_VERSION"));
    fs::create_dir_all(&installed).unwrap();
    fs::copy(TOS, installed.join("tos")).unwrap();
    let bin = env.root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(installed.join("tos"), bin.join("tos")).unwrap();
    fs::write(data.join("install-dir"), format!("{}\n", bin.display())).unwrap();
    bin.join("tos")
}

/// Runs `<exe> update <args>` with the test's variables, a throwaway HOME and no PATH changes.
fn run_update(env: &Env, exe: &Path, args: &[&str], extra: &[(&str, String)]) -> std::process::Output {
    Command::new(exe)
        .arg("update")
        .args(args)
        .envs(env.vars.iter().map(|(k, v)| (k, v)))
        .env("HOME", env.root.join("home"))
        .env("TOS_NO_MODIFY_PATH", "1")
        .envs(extra.iter().map(|(k, v)| (k, v)))
        .output()
        .unwrap()
}

#[test]
fn update_replaces_an_installed_tos_and_keeps_the_one_before() {
    let env = Env::new("e2e-update");
    let tos = install_like_the_installer(&env);
    let versions = env.root.join("data/termos/versions");
    let release = env.root.join("release");
    fake_release(&release, "9.9.9");
    let from_release = [("TOS_DOWNLOAD_URL", format!("file://{}", release.display()))];

    // A tos that the installer didn't put there (this test's own build) isn't replaced.
    let out = run_update(&env, Path::new(TOS), &[], &from_release);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("wasn't installed by the installer"), "{out:?}");

    // The installed one is: bin/tos now points at 9.9.9, and the version before stays for running sessions.
    let out = run_update(&env, &tos, &[], &from_release);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_link(&tos).unwrap(), versions.join("9.9.9/tos"));
    assert!(versions.join(env!("CARGO_PKG_VERSION")).join("tos").exists());
    let said = Command::new(&tos).arg("--version").output().unwrap();
    assert_eq!(String::from_utf8_lossy(&said.stdout).trim(), "tos 9.9.9 (extension API 2)");
}

#[test]
fn update_does_nothing_when_already_the_newest() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl isn't installed; skipping");
        return;
    }
    let env = Env::new("e2e-update-newest");
    let tos = install_like_the_installer(&env);
    let before = fs::read_link(&tos).unwrap();
    let github = fake_github(env!("CARGO_PKG_VERSION"));
    let out = run_update(&env, &tos, &[], &[("TOS_RELEASES_URL", github)]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(said.contains(&format!("tos {} is already the newest release", env!("CARGO_PKG_VERSION"))), "{said}");
    assert_eq!(fs::read_link(&tos).unwrap(), before);
}

#[test]
fn update_to_a_release_without_update_needs_force() {
    let env = Env::new("e2e-update-old");
    let tos = install_like_the_installer(&env);
    let release = env.root.join("release");
    fake_release(&release, "0.2.0");
    let from_release = [("TOS_DOWNLOAD_URL", format!("file://{}", release.display()))];

    let out = run_update(&env, &tos, &["--version", "0.2.0"], &from_release);
    assert!(!out.status.success());
    let why = String::from_utf8_lossy(&out.stderr);
    assert!(why.contains("doesn't have `tos update`") && why.contains("--force"), "{why}");
    assert_eq!(fs::read_link(&tos).unwrap().parent().unwrap().file_name().unwrap(), env!("CARGO_PKG_VERSION"));

    let out = run_update(&env, &tos, &["--version", "0.2.0", "--force"], &from_release);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fs::read_link(&tos).unwrap(), env.root.join("data/termos/versions/0.2.0/tos"));

    // Before the rename it was tb, which `tos update` can't install even with --force.
    let out = run_update(&env, Path::new(TOS), &["--version", "0.1.1", "--force"], &from_release);
    assert!(String::from_utf8_lossy(&out.stderr).contains("called tb"), "{out:?}");
}

#[test]
fn update_notice_shows_a_newer_release() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("curl isn't installed; skipping");
        return;
    }
    let mut env = Env::new("e2e-notice");
    env.vars.retain(|(k, _)| k != "TOS_NO_UPDATE_CHECK");
    env.set("TOS_RELEASES_URL", &fake_github("9.9.9"));
    let s = env.attach(120, 20);
    s.wait_for("tos 9.9.9 is available: run tos update");
    // The answer is kept for a day, so other sessions don't ask again.
    let kept = fs::read_to_string(env.root.join("state/update-check")).unwrap();
    assert!(kept.trim().ends_with(" 9.9.9"), "{kept}");
}
