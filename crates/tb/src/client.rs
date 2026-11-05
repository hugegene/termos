//! The client side: starting the server, attaching a terminal, and one-shot commands.

use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, bail};
use crossterm::cursor::{SetCursorStyle, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste, EnableFocusChange,
    EnableMouseCapture, Event, KeyCode, KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::style::ResetColor;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use crate::paths;
use crate::protocol::{
    ClientKind, ClientMsg, Command, Graphics, ImageProtocol, InputEvent, PROTOCOL_VERSION, PaneId, ServerMsg,
    read_frame, write_frame,
};

pub fn connect(session: &str) -> io::Result<UnixStream> {
    UnixStream::connect(paths::socket_path(session))
}

/// Connects to the session's server, starting it first if it isn't running. Returns true if it was started.
/// `run_commands`: a restored session runs the commands that were running, instead of typing them.
pub fn ensure_server(session: &str, run_commands: bool) -> anyhow::Result<bool> {
    if connect(session).is_ok() {
        return Ok(false);
    }
    paths::ensure_private_dir(&paths::socket_dir())?;
    let exe = env::current_exe().context("finding the tb program")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__server").arg(session);
    if run_commands {
        cmd.arg("--run-commands");
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // A new session (setsid) so closing this terminal doesn't end the server.
    unsafe {
        cmd.pre_exec(|| rustix::process::setsid().map(drop).map_err(io::Error::from));
    }
    let mut child = cmd.spawn().context("starting the session server")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if connect(session).is_ok() {
            return Ok(true);
        }
        if let Some(status) = child.try_wait()? {
            bail!("the session server stopped ({status}); see {}", paths::log_path(session).display());
        }
        if Instant::now() > deadline {
            bail!("the session server didn't start; see {}", paths::log_path(session).display());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Sends one command and returns the server's answer.
pub fn command(session: &str, cmd: Command) -> anyhow::Result<String> {
    let mut stream = connect(session).with_context(|| format!("no session named '{session}'"))?;
    write_frame(&mut stream, &ClientMsg::Hello { version: PROTOCOL_VERSION, kind: ClientKind::Command(cmd) })?;
    match read_frame::<_, ServerMsg>(&mut stream)? {
        ServerMsg::Reply { ok: true, text } => Ok(text),
        ServerMsg::Reply { ok: false, text } | ServerMsg::Exit { reason: text } => bail!(text),
        ServerMsg::Output(_) | ServerMsg::Event(_) | ServerMsg::Switch { .. } => {
            bail!("unexpected answer from the session")
        }
    }
}

pub fn list_sessions(short: bool, reverse: bool) -> anyhow::Result<()> {
    let current = env::var("TB_SESSION").ok();
    let mark = |name: &str| if current.as_deref() == Some(name) { " (current)" } else { "" };
    // (name, the full line)
    let mut lines: Vec<(String, String)> = Vec::new();
    for name in paths::session_names() {
        match command(&name, Command::Info) {
            // zellij's shape: the name first, details in brackets, so scripts can split on spaces.
            Ok(info) => lines.push((name.clone(), format!("{name} [{info}]{}", mark(&name)))),
            Err(e) => {
                if connect(&name).is_err() {
                    // Nobody answers: the socket is left over from a session that died.
                    let _ = fs::remove_file(paths::socket_path(&name));
                } else {
                    let why = if e.to_string().contains("uses protocol") {
                        "started by an older tb; `tb kill` ends it".to_owned()
                    } else {
                        format!("not answering: {e}")
                    };
                    lines.push((name.clone(), format!("{name} ({why}){}", mark(&name))));
                }
            }
        }
    }
    // Saved sessions that aren't running come back when attached.
    let running: Vec<String> = lines.iter().map(|(n, _)| n.clone()).collect();
    for name in paths::saved_names().into_iter().filter(|n| !running.contains(n)) {
        let line = format!("{name} (EXITED - attach to restore)");
        lines.push((name, line));
    }
    if reverse {
        lines.reverse();
    }
    for (name, line) in &lines {
        println!("{}", if short { name } else { line });
    }
    if lines.is_empty() && !short {
        println!("no sessions");
    }
    Ok(())
}

/// Sessions whose server answers, with the one we're in (if any) last, so ending it doesn't stop us early.
fn running_sessions() -> Vec<String> {
    let mut names: Vec<String> = paths::session_names().into_iter().filter(|n| connect(n).is_ok()).collect();
    if let Ok(current) = env::var("TB_SESSION")
        && let Some(i) = names.iter().position(|n| *n == current)
    {
        let me = names.remove(i);
        names.push(me);
    }
    names
}

/// Asks a yes/no question on the terminal; anything but "y" or "yes" is no.
fn confirm(question: &str) -> io::Result<bool> {
    print!("{question} (y/N) ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}

/// Ends every running session. Their layouts are kept, so attaching restores them.
pub fn kill_all_sessions(yes: bool) -> anyhow::Result<()> {
    let names = running_sessions();
    if names.is_empty() {
        println!("no running sessions");
        return Ok(());
    }
    if !yes && !confirm(&format!("End {} (attach again to restore)?", names.join(", ")))? {
        return Ok(());
    }
    let mut failed = 0;
    for name in &names {
        if let Err(e) = kill_session(name, false) {
            eprintln!("tb: {name}: {e:#}");
            failed += 1;
        }
    }
    if failed > 0 {
        bail!("{failed} of {} sessions didn't end", names.len());
    }
    Ok(())
}

/// Forgets every exited session; with `force`, also ends and forgets the running ones.
pub fn delete_all_sessions(yes: bool, force: bool) -> anyhow::Result<()> {
    let running = running_sessions();
    let exited: Vec<String> = paths::saved_names().into_iter().filter(|n| !running.contains(n)).collect();
    let mut targets = exited.clone();
    if force {
        targets.extend(running.iter().cloned());
    }
    if targets.is_empty() {
        if running.is_empty() {
            println!("no sessions");
        } else {
            println!("no exited sessions; running ones are only deleted with --force");
        }
        return Ok(());
    }
    let question = if force {
        format!("End and forget {}?", targets.join(", "))
    } else {
        format!("Forget {}?", targets.join(", "))
    };
    if !yes && !confirm(&question)? {
        return Ok(());
    }
    let mut failed = 0;
    for name in &exited {
        if let Err(e) = delete_session(name, false) {
            eprintln!("tb: {name}: {e:#}");
            failed += 1;
        }
    }
    if force {
        for name in &running {
            if let Err(e) = kill_session(name, true) {
                eprintln!("tb: {name}: {e:#}");
                failed += 1;
            }
        }
    } else if !running.is_empty() {
        println!("still running: {} (add --force to end and forget them)", running.join(", "));
    }
    if failed > 0 {
        bail!("{failed} of {} sessions weren't deleted", targets.len());
    }
    Ok(())
}

/// Deletes a session's saved layout before attaching, so it starts fresh. A running session is left alone.
pub fn forget_saved(session: &str) -> anyhow::Result<()> {
    if connect(session).is_ok() {
        eprintln!("tb: session '{session}' is running, so there's no saved layout to forget");
        return Ok(());
    }
    let path = paths::saved_path(session);
    match fs::remove_file(&path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e).with_context(|| format!("removing {}", path.display())),
        _ => Ok(()),
    }
}

/// Ends a session. A session started by an older `tb` can't read our commands, so its server process is ended
/// directly (it's ours: the socket only accepts our user).
pub fn kill_session(session: &str, forget: bool) -> anyhow::Result<()> {
    match command(session, Command::Kill { forget }) {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("uses protocol") => {
            let stream = connect(session)?;
            let peer = rustix::net::sockopt::socket_peercred(&stream)?;
            if peer.uid != rustix::process::getuid() {
                bail!("session '{session}' belongs to another user");
            }
            rustix::process::kill_process(peer.pid, rustix::process::Signal::TERM)?;
            let _ = fs::remove_file(paths::socket_path(session));
            println!("ended session '{session}', which an older tb had started");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Forgets a saved session. A running one is only ended with `force`.
pub fn delete_session(session: &str, force: bool) -> anyhow::Result<()> {
    if connect(session).is_ok() {
        if !force {
            bail!("session '{session}' is running; end and forget it with --force");
        }
        kill_session(session, true)?;
        return Ok(());
    }
    let path = paths::saved_path(session);
    if !path.exists() {
        bail!("no saved session named '{session}'");
    }
    fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
    Ok(())
}

/// `tb subscribe`: prints events as they happen until the session ends (or the subscribed panes close).
/// With `json` every event is a JSON line; otherwise pane text is printed as plain lines and other events
/// as JSON lines.
pub fn subscribe(
    session: &str,
    panes: Vec<PaneId>,
    scrollback: Option<Option<usize>>,
    ansi: bool,
    json: bool,
) -> anyhow::Result<()> {
    let mut stream = connect(session).with_context(|| format!("no session named '{session}'"))?;
    let hello = ClientMsg::Hello { version: PROTOCOL_VERSION, kind: ClientKind::Subscribe { panes, scrollback, ansi } };
    write_frame(&mut stream, &hello)?;
    let mut out = io::stdout().lock();
    loop {
        match read_frame::<_, ServerMsg>(&mut stream) {
            Ok(ServerMsg::Event(line)) => {
                let text = if json {
                    line
                } else {
                    match serde_json::from_str::<serde_json::Value>(&line) {
                        Ok(v) if v["event"] == "pane_text" => v["lines"]
                            .as_array()
                            .map(|l| l.iter().filter_map(|s| s.as_str()).collect::<Vec<_>>().join("\n"))
                            .unwrap_or_default(),
                        _ => line,
                    }
                };
                // A closed pipe (e.g. `| head`) ends the subscription quietly.
                if writeln!(out, "{text}").and_then(|()| out.flush()).is_err() {
                    return Ok(());
                }
            }
            Ok(ServerMsg::Exit { reason }) if reason.starts_with("no pane") => bail!(reason),
            Ok(ServerMsg::Reply { ok: false, text }) => bail!(text),
            Ok(_) => return Ok(()),
            Err(_) => return Ok(()),
        }
    }
}

/// Puts the terminal into the state the session needs, and restores it when dropped (also on panic).
struct TerminalGuard {
    kitty_keys: bool,
}

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let kitty_keys = matches!(terminal::supports_keyboard_enhancement(), Ok(true));
        let mut out = io::stdout();
        execute!(out, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste, EnableFocusChange)?;
        if kitty_keys {
            execute!(out, PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
        }
        Ok(TerminalGuard { kitty_keys })
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut out = io::stdout();
        if self.kitty_keys {
            let _ = execute!(out, PopKeyboardEnhancementFlags);
        }
        let _ = execute!(
            out,
            DisableFocusChange,
            DisableBracketedPaste,
            DisableMouseCapture,
            ResetColor,
            SetCursorStyle::DefaultUserShape,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// Works out how this terminal shows images (step 5). Runs in raw mode on the alternate screen, before
/// anything else reads input.
fn detect_graphics() -> Graphics {
    let var = |k: &str| env::var(k).unwrap_or_default();
    let mut cell_px = terminal::window_size()
        .ok()
        .filter(|w| w.width > 0 && w.height > 0 && w.columns > 0 && w.rows > 0)
        .map(|w| (w.width / w.columns, w.height / w.rows));
    let forced = match var("TB_IMAGES").as_str() {
        "kitty" => Some(ImageProtocol::Kitty),
        "iterm2" => Some(ImageProtocol::Iterm2),
        "sixel" => Some(ImageProtocol::Sixel),
        "blocks" => Some(ImageProtocol::Blocks),
        _ => None,
    };
    if let Some(protocol) = forced {
        if protocol == ImageProtocol::Sixel {
            // Forced: ask for the colours anyway, but keep sixel whatever the answer.
            sixel_colors_ok();
        }
        return Graphics { protocol, cell_px };
    }
    // Images rarely make it through another multiplexer.
    if ["TMUX", "ZELLIJ", "STY"].iter().any(|k| env::var_os(k).is_some()) {
        return Graphics { protocol: ImageProtocol::Blocks, cell_px };
    }
    let replies = query_terminal("\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[16t\x1b[c");
    let answers = parse_replies(&replies);
    cell_px = cell_px.or(answers.cell_px);
    let mut protocol = choose_protocol(&answers, &var("TERM_PROGRAM"), &var("LC_TERMINAL"));
    if protocol == ImageProtocol::Sixel && !sixel_colors_ok() {
        protocol = ImageProtocol::Blocks;
    }
    Graphics { protocol, cell_px }
}

/// Asks the terminal about kitty graphics, its cell size and its device attributes, and returns the raw
/// answers. Every terminal answers the device attributes query, so its answer ends the wait.
/// Our sixel images use 216 colours; xterm in VT340 mode has 16 colour registers unless a program asks for
/// more (XTSMGRAPHICS). Asks for 256 and returns false if the terminal refuses.
fn sixel_colors_ok() -> bool {
    sixel_registers_ok(&query_terminal("\x1b[?1;3;256S\x1b[c"))
}

/// Reads the XTSMGRAPHICS answer `CSI ? 1 ; status ; value S`. No answer means the terminal doesn't support
/// the query (most sixel terminals have plenty of colours), so that counts as fine.
fn sixel_registers_ok(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    // Find a `CSI ? … S` answer; a device-attributes answer (`CSI ? … c`) can start the same way.
    let answer = text.match_indices("\x1b[?").find_map(|(i, _)| {
        let rest = &text[i + 3..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
        (rest[end..].starts_with('S')).then(|| &rest[..end])
    });
    let Some(params) = answer else { return true };
    let nums: Vec<Option<u32>> = params.split(';').map(|n| n.parse().ok()).collect();
    match nums.as_slice() {
        [Some(1), Some(0), Some(n), ..] => *n >= 216,
        [Some(1), Some(0)] => true,
        _ => false,
    }
}

/// Sends `query` and reads the answers for up to a second, stopping at the device-attributes answer that
/// every terminal sends (so `query` must end with `CSI c`).
fn query_terminal(query: &str) -> Vec<u8> {
    use std::os::fd::AsFd;
    let mut out = io::stdout();
    if out.write_all(query.as_bytes()).and_then(|()| out.flush()).is_err() {
        return Vec::new();
    }
    let stdin = io::stdin();
    let fd = stdin.as_fd();
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut got = Vec::new();
    let mut buf = [0u8; 1024];
    while Instant::now() < deadline && !has_device_attributes(&got) {
        let left = deadline.saturating_duration_since(Instant::now());
        let left = left.max(Duration::from_millis(1));
        let timeout =
            rustix::event::Timespec { tv_sec: left.as_secs() as i64, tv_nsec: i64::from(left.subsec_nanos()) };
        let mut fds = [rustix::event::PollFd::new(&fd, rustix::event::PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(n) if n > 0 => match rustix::io::read(fd, &mut buf) {
                Ok(n) if n > 0 => got.extend_from_slice(&buf[..n]),
                _ => break,
            },
            Ok(_) => {}
            Err(_) => break,
        }
    }
    got
}

fn has_device_attributes(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    text.find("\x1b[?").is_some_and(|i| text[i..].contains('c'))
}

#[derive(Debug, Default, PartialEq)]
struct Answers {
    kitty: bool,
    cell_px: Option<(u16, u16)>,
    sixel: bool,
}

fn parse_replies(bytes: &[u8]) -> Answers {
    let text = String::from_utf8_lossy(bytes);
    let kitty = text.contains("\x1b_Gi=31;OK");
    // Cell size: CSI 6 ; height ; width t
    let cell_px = text.find("\x1b[6;").and_then(|i| {
        let rest = &text[i + 4..];
        let end = rest.find('t')?;
        let mut nums = rest[..end].split(';').map(|n| n.parse::<u16>().ok());
        let (h, w) = (nums.next()??, nums.next()??);
        (h > 0 && w > 0).then_some((w, h))
    });
    // Device attributes: CSI ? 6x ; ... c, where 4 means sixel.
    let sixel = text.find("\x1b[?").is_some_and(|i| {
        let rest = &text[i + 3..];
        rest.find('c').is_some_and(|end| rest[..end].split(';').any(|p| p == "4"))
    });
    Answers { kitty, cell_px, sixel }
}

/// kitty first, then iTerm2, then sixel: the order the rebuild plan sets.
fn choose_protocol(answers: &Answers, term_program: &str, lc_terminal: &str) -> ImageProtocol {
    if answers.kitty {
        ImageProtocol::Kitty
    } else if matches!(term_program, "iTerm.app" | "WezTerm") || lc_terminal == "iTerm2" {
        ImageProtocol::Iterm2
    } else if answers.sixel {
        ImageProtocol::Sixel
    } else {
        ImageProtocol::Blocks
    }
}

/// Attaches this terminal to the session until detach or the session ends.
/// Attaching from inside a session would nest one in the other; refuse.
pub fn ensure_outside() -> anyhow::Result<()> {
    if let Ok(current) = env::var("TB_SESSION") {
        bail!("already inside session '{current}' (detach first with Ctrl+O d)");
    }
    Ok(())
}

pub fn attach(session: &str, run_commands: bool) -> anyhow::Result<()> {
    ensure_outside()?;
    show(session, run_commands, false)
}

/// `tb watch`: shows a running session read-only. `q` or Ctrl+C stops watching.
pub fn watch(session: &str) -> anyhow::Result<()> {
    ensure_outside()?;
    if connect(session).is_err() {
        bail!("no session named '{session}'");
    }
    show(session, false, true)
}

/// How showing a session ended.
enum End {
    Reason(String),
    /// `switch-session`: show this session next.
    Switch(String),
}

/// Shows sessions in this terminal until one ends, the client detaches, or the user stops watching. The
/// terminal stays in the session's state across `switch-session`.
fn show(first: &str, run_commands: bool, read_only: bool) -> anyhow::Result<()> {
    terminal::size().context("this command needs a terminal")?;
    let guard = TerminalGuard::enter()?;
    let graphics = detect_graphics();
    let mut session = first.to_owned();
    let mut run_commands = run_commands;
    let reason = loop {
        if !read_only {
            ensure_server(&session, run_commands)?;
        }
        run_commands = false;
        match show_one(&session, graphics, read_only)? {
            End::Switch(next) => session = next,
            End::Reason(reason) => break reason,
        }
    };
    drop(guard);
    match reason.as_str() {
        "detached" => println!("[detached from session {session}]"),
        "session ended" => println!("[session {session} ended]"),
        other => println!("[{other}]"),
    }
    Ok(())
}

fn show_one(session: &str, graphics: Graphics, read_only: bool) -> anyhow::Result<End> {
    let mut stream = connect(session).with_context(|| format!("connecting to session '{session}'"))?;
    let (cols, rows) = terminal::size().context("this command needs a terminal")?;
    let kind = if read_only {
        ClientKind::Watch { cols, rows, graphics }
    } else {
        ClientKind::Attach { cols, rows, graphics }
    };
    write_frame(&mut stream, &ClientMsg::Hello { version: PROTOCOL_VERSION, kind })?;
    let mut reader = stream.try_clone()?;
    let (done_tx, done_rx) = mpsc::channel::<End>();
    thread::spawn(move || {
        let end = loop {
            match read_frame::<_, ServerMsg>(&mut reader) {
                Ok(ServerMsg::Output(bytes)) => {
                    let mut out = io::stdout().lock();
                    if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
                        break End::Reason("the terminal closed".to_owned());
                    }
                }
                Ok(ServerMsg::Exit { reason }) => break End::Reason(reason),
                Ok(ServerMsg::Reply { text, .. }) => break End::Reason(text),
                Ok(ServerMsg::Switch { session }) => break End::Switch(session),
                Ok(ServerMsg::Event(_)) => {}
                Err(_) => break End::Reason("lost the connection to the session".to_owned()),
            }
        };
        let _ = done_tx.send(end);
    });

    loop {
        if let Ok(end) = done_rx.try_recv() {
            return Ok(end);
        }
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let msg = match event::read()? {
            Event::Resize(cols, rows) => ClientMsg::Resize { cols, rows },
            // Watching: only q and Ctrl+C do anything.
            Event::Key(k) if read_only => {
                let ctrl_c = k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL);
                if k.code == KeyCode::Char('q') || ctrl_c {
                    let _ = write_frame(&mut stream, &ClientMsg::Detach);
                    return Ok(End::Reason(format!("stopped watching session {session}")));
                }
                continue;
            }
            _ if read_only => continue,
            Event::Key(k) => ClientMsg::Input(InputEvent::Key(k)),
            Event::Mouse(m) => ClientMsg::Input(InputEvent::Mouse(m)),
            Event::Paste(s) => ClientMsg::Input(InputEvent::Paste(s)),
            Event::FocusGained => ClientMsg::Input(InputEvent::FocusGained),
            Event::FocusLost => ClientMsg::Input(InputEvent::FocusLost),
        };
        if write_frame(&mut stream, &msg).is_err() {
            let end = done_rx.recv_timeout(Duration::from_secs(1));
            return Ok(end.unwrap_or_else(|_| End::Reason("lost the connection".into())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_answers_are_read() {
        let ghostty = b"\x1b_Gi=31;OK\x1b\\\x1b[6;20;10t\x1b[?62;22c";
        let a = parse_replies(ghostty);
        assert_eq!(a, Answers { kitty: true, cell_px: Some((10, 20)), sixel: false });
        assert!(has_device_attributes(ghostty));
        assert_eq!(choose_protocol(&a, "ghostty", ""), ImageProtocol::Kitty);

        let xterm = parse_replies(b"\x1b[?63;1;2;4;6;9c");
        assert!(xterm.sixel && !xterm.kitty);
        assert_eq!(choose_protocol(&xterm, "", ""), ImageProtocol::Sixel);
        assert_eq!(choose_protocol(&xterm, "iTerm.app", ""), ImageProtocol::Iterm2, "iTerm2 beats sixel");
        assert_eq!(choose_protocol(&parse_replies(b"\x1b[?6c"), "", ""), ImageProtocol::Blocks);
        assert!(!has_device_attributes(b"\x1b_Gi=31;OK\x1b\\"));
    }

    #[test]
    fn sixel_colour_answers_are_read() {
        // xterm with sixel on grants 256; with sixel off it answers status 3.
        assert!(sixel_registers_ok(b"\x1b[?1;0;256S\x1b[?63;1;2;4;6;9;15;16;22;28c"));
        assert!(!sixel_registers_ok(b"\x1b[?1;3S\x1b[?64;1;2;6;9c"));
        assert!(!sixel_registers_ok(b"\x1b[?1;0;16S\x1b[?63;4c"), "too few colours");
        // No answer (the query is unknown): fine. A VT100 device-attributes answer isn't mistaken for one.
        assert!(sixel_registers_ok(b"\x1b[?62;4c"));
        assert!(sixel_registers_ok(b"\x1b[?1;2c"));
        assert!(sixel_registers_ok(b""));
    }
}
