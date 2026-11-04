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
    EnableMouseCapture, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::style::ResetColor;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};

use crate::paths;
use crate::protocol::{
    ClientKind, ClientMsg, Command, InputEvent, PROTOCOL_VERSION, ServerMsg, read_frame, write_frame,
};

pub fn connect(session: &str) -> io::Result<UnixStream> {
    UnixStream::connect(paths::socket_path(session))
}

/// Connects to the session's server, starting it first if it isn't running. Returns true if it was started.
pub fn ensure_server(session: &str) -> anyhow::Result<bool> {
    if connect(session).is_ok() {
        return Ok(false);
    }
    paths::ensure_private_dir(&paths::socket_dir())?;
    let exe = env::current_exe().context("finding the tb program")?;
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__server").arg(session).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
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
        ServerMsg::Output(_) => bail!("unexpected answer from the session"),
    }
}

pub fn list_sessions() -> anyhow::Result<()> {
    let mut any = false;
    for name in paths::session_names() {
        match command(&name, Command::Info) {
            Ok(info) => {
                println!("{name}: {info}");
                any = true;
            }
            // Nobody answers: the socket is left over from a session that died.
            Err(_) => {
                if connect(&name).is_err() {
                    let _ = fs::remove_file(paths::socket_path(&name));
                }
            }
        }
    }
    if !any {
        println!("no sessions");
    }
    Ok(())
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

/// Attaches this terminal to the session until detach or the session ends.
pub fn attach(session: &str) -> anyhow::Result<()> {
    if let Ok(current) = env::var("TB_SESSION") {
        bail!("already inside session '{current}' (detach first with Ctrl+B d)");
    }
    ensure_server(session)?;
    let mut stream = connect(session).with_context(|| format!("connecting to session '{session}'"))?;
    let (cols, rows) = terminal::size().context("this command needs a terminal")?;
    write_frame(&mut stream, &ClientMsg::Hello { version: PROTOCOL_VERSION, kind: ClientKind::Attach { cols, rows } })?;

    let guard = TerminalGuard::enter()?;
    let mut reader = stream.try_clone()?;
    let (done_tx, done_rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        let reason = loop {
            match read_frame::<_, ServerMsg>(&mut reader) {
                Ok(ServerMsg::Output(bytes)) => {
                    let mut out = io::stdout().lock();
                    if out.write_all(&bytes).and_then(|()| out.flush()).is_err() {
                        break "the terminal closed".to_owned();
                    }
                }
                Ok(ServerMsg::Exit { reason }) => break reason,
                Ok(ServerMsg::Reply { text, .. }) => break text,
                Err(_) => break "lost the connection to the session".to_owned(),
            }
        };
        let _ = done_tx.send(reason);
    });

    let reason = loop {
        if let Ok(reason) = done_rx.try_recv() {
            break reason;
        }
        if !event::poll(Duration::from_millis(50))? {
            continue;
        }
        let msg = match event::read()? {
            Event::Key(k) => ClientMsg::Input(InputEvent::Key(k)),
            Event::Mouse(m) => ClientMsg::Input(InputEvent::Mouse(m)),
            Event::Paste(s) => ClientMsg::Input(InputEvent::Paste(s)),
            Event::Resize(cols, rows) => ClientMsg::Resize { cols, rows },
            Event::FocusGained => ClientMsg::Input(InputEvent::FocusGained),
            Event::FocusLost => ClientMsg::Input(InputEvent::FocusLost),
        };
        if write_frame(&mut stream, &msg).is_err() {
            break done_rx.recv_timeout(Duration::from_secs(1)).unwrap_or_else(|_| "lost the connection".into());
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
