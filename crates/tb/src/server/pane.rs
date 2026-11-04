//! A terminal pane: an `alacritty_terminal` terminal, the PTY with its program, and the PTY's I/O thread.

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::Sender;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;

use super::Ev;
use crate::protocol::PaneId;

/// Forwards events from a pane's terminal and I/O thread to the server's main loop.
#[derive(Clone)]
pub struct EventProxy {
    id: PaneId,
    tx: Sender<Ev>,
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let _ = self.tx.send(Ev::Pane(self.id, event));
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Size {
    pub cols: usize,
    pub lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Default, Clone)]
pub struct SpawnOptions {
    pub cwd: Option<PathBuf>,
    /// Program and arguments; empty means the user's shell.
    pub command: Vec<String>,
    pub env: HashMap<String, String>,
}

pub struct TermPane {
    pub term: Arc<FairMutex<Term<EventProxy>>>,
    sender: EventLoopSender,
    /// A copy of the PTY's controlling side, used to see which program runs in the foreground.
    master: Option<File>,
    pub pid: u32,
    /// Title set by the program (OSC 0/2), if any.
    pub title: Option<String>,
    pub cols: u16,
    pub rows: u16,
}

fn window_size(cols: u16, rows: u16) -> WindowSize {
    // Pixel sizes are guesses; few programs use them, and none may divide by them if they're zero.
    WindowSize { num_cols: cols, num_lines: rows, cell_width: 8, cell_height: 16 }
}

impl TermPane {
    pub fn spawn(
        id: PaneId,
        cols: u16,
        rows: u16,
        opts: SpawnOptions,
        session: &str,
        tx: Sender<Ev>,
    ) -> std::io::Result<Self> {
        let (cols, rows) = (cols.max(2), rows.max(1));
        let proxy = EventProxy { id, tx };
        let size = Size { cols: usize::from(cols), lines: usize::from(rows) };
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &size, proxy.clone())));

        let mut env = opts.env;
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());
        env.insert("TB_SESSION".into(), session.into());
        env.insert("TB_PANE_ID".into(), id.to_string());
        let shell = opts.command.split_first().map(|(p, args)| tty::Shell::new(p.clone(), args.to_vec()));
        let options = tty::Options { shell, working_directory: opts.cwd, drain_on_exit: true, env };

        let pty = tty::new(&options, window_size(cols, rows), u64::from(id))?;
        let pid = pty.child().id();
        let master = pty.file().try_clone().ok();
        let event_loop = EventLoop::new(term.clone(), proxy, pty, true, false)?;
        let sender = event_loop.channel();
        // Detached: when the loop ends, the PTY is dropped on its own thread (that drop waits for the child).
        drop(event_loop.spawn());
        Ok(TermPane { term, sender, master, pid, title: None, cols, rows })
    }

    pub fn write(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.sender.send(Msg::Input(bytes.into()));
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        self.term.lock().resize(Size { cols: usize::from(cols), lines: usize::from(rows) });
        let _ = self.sender.send(Msg::Resize(window_size(cols, rows)));
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(Msg::Shutdown);
    }

    /// The shell's working folder (follows `cd`).
    pub fn cwd(&self) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{}/cwd", self.pid)).ok()
    }

    /// Name of the program in the foreground, for example "nvim" or "bash".
    pub fn foreground_name(&self) -> Option<String> {
        let pgid = self.master.as_ref().and_then(|f| rustix::termios::tcgetpgrp(f).ok())?;
        let comm = std::fs::read_to_string(format!("/proc/{}/comm", pgid.as_raw_nonzero())).ok()?;
        Some(comm.trim().to_owned())
    }

    /// What to call this pane: the program's title, else the foreground program's name.
    pub fn display_title(&self) -> String {
        self.title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| self.foreground_name())
            .unwrap_or_else(|| "shell".into())
    }

    /// The pane's text: the screen, or with `full` the whole scrollback too. Trailing spaces and empty
    /// lines at the end are removed.
    pub fn text(&self, full: bool) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let top = if full { -(grid.history_size() as i32) } else { 0 };
        let mut lines = Vec::new();
        for line in top..grid.screen_lines() as i32 {
            let row = &grid[Line(line)];
            let mut s = String::new();
            for col in 0..grid.columns() {
                let cell = &row[Column(col)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                s.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    s.extend(zw);
                }
            }
            lines.push(s.trim_end().to_owned());
        }
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.join("\n")
    }
}
