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
    /// The program the pane was started with; empty for the user's shell.
    pub command: Vec<String>,
    /// Title set by the program (OSC 0/2), if any.
    pub title: Option<String>,
    pub cols: u16,
    pub rows: u16,
    /// Cell size in pixels, from the attached terminal (8×16 if it's unknown). Programs that size images
    /// (chafa, zellij, yazi) read it from the PTY and from `CSI 14 t` / `CSI 16 t`.
    pub cell: (u16, u16),
}

fn window_size(cols: u16, rows: u16, cell: (u16, u16)) -> WindowSize {
    // Never zero: some programs divide by it.
    WindowSize { num_cols: cols, num_lines: rows, cell_width: cell.0.max(1), cell_height: cell.1.max(1) }
}

impl TermPane {
    pub fn spawn(
        id: PaneId,
        cols: u16,
        rows: u16,
        cell: (u16, u16),
        opts: SpawnOptions,
        session: &str,
        tx: Sender<Ev>,
    ) -> std::io::Result<Self> {
        let (cols, rows) = (cols.max(2), rows.max(1));
        let proxy = EventProxy { id, tx };
        let size = Size { cols: usize::from(cols), lines: usize::from(rows) };
        let term = Arc::new(FairMutex::new(Term::new(Config::default(), &size, proxy.clone())));

        let command = opts.command.clone();
        let mut env = opts.env;
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());
        env.insert("TB_SESSION".into(), session.into());
        env.insert("TB_PANE_ID".into(), id.to_string());
        let shell = opts.command.split_first().map(|(p, args)| tty::Shell::new(p.clone(), args.to_vec()));
        let options = tty::Options { shell, working_directory: opts.cwd, drain_on_exit: true, env };

        let pty = tty::new(&options, window_size(cols, rows, cell), u64::from(id))?;
        let pid = pty.child().id();
        let master = pty.file().try_clone().ok();
        let event_loop = EventLoop::new(term.clone(), proxy, pty, true, false)?;
        let sender = event_loop.channel();
        // Detached: when the loop ends, the PTY is dropped on its own thread (that drop waits for the child).
        drop(event_loop.spawn());
        Ok(TermPane { term, sender, master, pid, command, title: None, cols, rows, cell })
    }

    pub fn write(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            let _ = self.sender.send(Msg::Input(bytes.into()));
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell: (u16, u16)) {
        let (cols, rows) = (cols.max(2), rows.max(1));
        if (cols, rows, cell) == (self.cols, self.rows, self.cell) {
            return;
        }
        if (cols, rows) != (self.cols, self.rows) {
            self.term.lock().resize(Size { cols: usize::from(cols), lines: usize::from(rows) });
        }
        self.cols = cols;
        self.rows = rows;
        self.cell = cell;
        // Also when only the cell size changed: programs get SIGWINCH and read the new pixel size.
        let _ = self.sender.send(Msg::Resize(window_size(cols, rows, cell)));
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

    /// The command running in the foreground, unless it's the pane's shell: what to offer again after a
    /// restore. A pipeline gives only its first command.
    pub fn running_command(&self) -> Option<Vec<String>> {
        let pgid = self.master.as_ref().and_then(|f| rustix::termios::tcgetpgrp(f).ok())?;
        if pgid.as_raw_nonzero().get() as u32 == self.pid {
            // The pane's own program has the foreground: a shell, or a command it was started with.
            return (!self.command.is_empty()).then(|| self.command.clone());
        }
        let raw = std::fs::read(format!("/proc/{}/cmdline", pgid.as_raw_nonzero())).ok()?;
        let args: Vec<String> =
            raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        (!args.is_empty()).then_some(args)
    }

    /// The pane's own program and its arguments (its shell, or the command it was started with).
    pub fn program(&self) -> Option<String> {
        let raw = std::fs::read(format!("/proc/{}/cmdline", self.pid)).ok()?;
        let args: Vec<String> =
            raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        (!args.is_empty()).then(|| args.join(" "))
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

    /// Like `text`, with colours and styles as ANSI escape sequences (`dump-screen --ansi`).
    pub fn ansi_text(&self, full: bool) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let top = if full { -(grid.history_size() as i32) } else { 0 };
        let mut lines = Vec::new();
        for line in top..grid.screen_lines() as i32 {
            let row = &grid[Line(line)];
            // Stop after the last cell that shows something: a character, or a coloured background.
            let used = (0..grid.columns())
                .rev()
                .find(|c| {
                    let cell = &row[Column(*c)];
                    cell.c != ' ' || sgr_bg(cell.bg).is_some() || cell.flags.contains(Flags::INVERSE)
                })
                .map_or(0, |c| c + 1);
            let mut s = String::new();
            let mut last = String::new();
            for col in 0..used {
                let cell = &row[Column(col)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                let sgr = sgr(cell);
                if sgr != last {
                    s.push_str(&format!("\x1b[0{sgr}m"));
                    last = sgr;
                }
                s.push(cell.c);
                if let Some(zw) = cell.zerowidth() {
                    s.extend(zw);
                }
            }
            if !last.is_empty() {
                s.push_str("\x1b[0m");
            }
            lines.push(s);
        }
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines.join("\n")
    }
}

/// The SGR parameters for a cell, each starting with `;` (empty for the default style).
fn sgr(cell: &alacritty_terminal::term::cell::Cell) -> String {
    let mut s = String::new();
    for (flag, code) in [
        (Flags::BOLD, "1"),
        (Flags::DIM, "2"),
        (Flags::ITALIC, "3"),
        (Flags::UNDERLINE, "4"),
        (Flags::INVERSE, "7"),
        (Flags::HIDDEN, "8"),
        (Flags::STRIKEOUT, "9"),
    ] {
        if cell.flags.contains(flag) {
            s.push(';');
            s.push_str(code);
        }
    }
    if let Some(fg) = sgr_color(cell.fg, 30) {
        s.push_str(&fg);
    }
    if let Some(bg) = sgr_bg(cell.bg) {
        s.push_str(&bg);
    }
    s
}

fn sgr_bg(c: alacritty_terminal::vte::ansi::Color) -> Option<String> {
    sgr_color(c, 40)
}

/// `base` is 30 for the foreground and 40 for the background. The default colours give `None`.
fn sgr_color(c: alacritty_terminal::vte::ansi::Color, base: u8) -> Option<String> {
    use alacritty_terminal::vte::ansi::Color as C;
    Some(match c {
        C::Spec(rgb) => format!(";{};2;{};{};{}", base + 8, rgb.r, rgb.g, rgb.b),
        C::Indexed(i) => format!(";{};5;{i}", base + 8),
        C::Named(n) => {
            let i = n as usize;
            match i {
                0..=7 => format!(";{}", usize::from(base) + i),
                8..=15 => format!(";{}", usize::from(base) + 60 + i - 8),
                _ => return None,
            }
        }
    })
}
