//! `tos docs --list FILE`: the docs pane's program. It follows the list that the server appends to (one JSON
//! line per Markdown file an agent wrote, `server/docs_pane.rs`) and shows the files as an accordion: a header
//! line per file, newest first, and one file open below its header, drawn with the finder's Markdown renderer.
//! Text in the open file is selected and copied as in the finder's preview, with the same code (`select.rs`);
//! being a program, it copies by writing OSC 52, which `tos` passes on to the clipboard.
//! Plans: docs/plans/2026-11-17-agent-plan-pane.md, docs/plans/2026-11-21-preview-select-copy.md.

use std::collections::{HashMap, HashSet};
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{fs, thread};

use anyhow::Context;
use crossterm::event::{
    self as ct, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::{cursor, execute, terminal};
use notify::{RecursiveMode, Watcher};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use unicode_width::UnicodeWidthStr;

use crate::paths;
use crate::server::finder::select::{self, Making, Selection};
use crate::server::finder::view::markdown;

/// One file in the list.
#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    /// The path as shown: relative to the agent's folder, else under `~`.
    pub label: String,
    /// When the agent last wrote it (seconds since 1970, from the list).
    pub written: f64,
    /// The file's modification time when it was last on screen, open.
    pub seen: Option<SystemTime>,
}

/// What a key or click asks the program to do beyond redrawing.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Redraw,
    Quit,
    Edit(PathBuf),
    /// Text for the clipboard.
    Copy(String),
}

/// A selection in the open file, and which file and version of it: it clears when either changes.
struct Selected {
    sel: Selection,
    path: PathBuf,
    mtime: Option<SystemTime>,
}

/// The accordion's state.
#[derive(Default)]
pub struct Docs {
    /// Newest first, in the order files first appeared.
    pub entries: Vec<Entry>,
    /// The open section.
    pub open: Option<usize>,
    /// The section Tab and Enter work on.
    pub sel: usize,
    /// The file written last opens by itself.
    pub following: bool,
    /// Lines of the list already read.
    read: usize,
    /// Each file's scroll position.
    scroll: HashMap<PathBuf, usize>,
    /// Rendered files: (modification time, width, lines).
    cache: HashMap<PathBuf, (Option<SystemTime>, usize, Vec<Line<'static>>)>,
    /// From the last draw: the screen row of each header shown, for clicks.
    header_rows: Vec<(u16, usize)>,
    /// From the last draw: the open section's height on screen, and its length.
    body_rows: usize,
    body_len: usize,
    /// From the last draw: where the open section's lines are on screen, the column their text starts in, and
    /// the width they were rendered at.
    body_area: Rect,
    body_x: u16,
    body_width: usize,
    /// Text selected in the open section.
    selection: Option<Selected>,
    /// Said in the title line until the next key: why nothing was copied.
    note: Option<String>,
}

/// How a path shows: relative to `cwd` if inside it, else with `~` for the home folder.
fn label(path: &Path, cwd: Option<&Path>) -> String {
    if let Some(rel) = cwd.and_then(|c| path.strip_prefix(c).ok()) {
        return rel.to_string_lossy().into_owned();
    }
    for home in paths::home_dirs() {
        if let Ok(rel) = path.strip_prefix(home) {
            return format!("~/{}", rel.to_string_lossy());
        }
    }
    path.to_string_lossy().into_owned()
}

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// "now", "5m", "3h" or "2d": how long ago `t` (seconds since 1970) was.
fn age(t: f64, now: f64) -> String {
    let s = (now - t).max(0.0) as u64;
    match s {
        0..60 => "now".into(),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

/// Cuts `text` to `width` columns from the left, keeping the end: `…/plans/2026-11-21-x.md`.
fn fit_left(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut out: Vec<char> = Vec::new();
    let mut used = 1;
    for c in text.chars().rev() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w > width {
            break;
        }
        used += w;
        out.push(c);
    }
    std::iter::once('…').chain(out.into_iter().rev()).collect()
}

impl Docs {
    pub fn new() -> Docs {
        Docs { following: true, ..Docs::default() }
    }

    /// Reads the list's new lines. Only whole lines count: a line still being written waits for next time.
    pub fn ingest(&mut self, text: &str) {
        let lines: Vec<&str> = text.split_inclusive('\n').filter(|l| l.ends_with('\n')).collect();
        for line in lines.iter().skip(self.read) {
            self.read += 1;
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let Some(path) = v["path"].as_str().map(PathBuf::from) else { continue };
            let cwd = v["cwd"].as_str().map(Path::new);
            let t = v["t"].as_f64().unwrap_or_else(now_secs);
            let i = match self.entries.iter().position(|e| e.path == path) {
                Some(i) => {
                    self.entries[i].written = t;
                    i
                }
                None => {
                    let label = label(&path, cwd);
                    self.entries.insert(0, Entry { path, label, written: t, seen: None });
                    self.open = self.open.map(|o| o + 1);
                    self.sel = if self.entries.len() > 1 { self.sel + 1 } else { 0 };
                    0
                }
            };
            if self.following {
                self.open = Some(i);
                self.sel = i;
            }
        }
    }

    /// Drops files that no longer exist.
    pub fn forget_deleted(&mut self) {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].path.exists() {
                i += 1;
            } else {
                self.remove(i);
            }
        }
    }

    fn remove(&mut self, i: usize) {
        self.entries.remove(i);
        self.open = match self.open {
            Some(o) if o == i => None,
            Some(o) if o > i => Some(o - 1),
            o => o,
        };
        if self.sel > i || self.sel >= self.entries.len() {
            self.sel = self.sel.saturating_sub(1);
        }
    }

    /// Opens or closes section `i`. Choosing a section yourself stops following.
    fn toggle(&mut self, i: usize) {
        self.open = if self.open == Some(i) { None } else { Some(i) };
        self.sel = i;
        self.following = false;
    }

    /// Opens the next (`down`) or previous section.
    fn step(&mut self, down: bool) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        self.sel = if down { (self.sel + 1).min(last) } else { self.sel.saturating_sub(1) };
        self.open = Some(self.sel);
        self.following = false;
    }

    fn scroll_by(&mut self, delta: isize) {
        let Some(o) = self.open else { return };
        let max = self.body_len.saturating_sub(self.body_rows);
        let s = self.scroll.entry(self.entries[o].path.clone()).or_insert(0);
        *s = s.saturating_add_signed(delta).min(max);
    }

    fn scroll_to(&mut self, end: bool) {
        let Some(o) = self.open else { return };
        let to = if end { self.body_len.saturating_sub(self.body_rows) } else { 0 };
        self.scroll.insert(self.entries[o].path.clone(), to);
    }

    pub fn key(&mut self, k: KeyEvent) -> Outcome {
        self.check_selection();
        self.note = None;
        let page = self.body_rows.max(2) as isize - 1;
        let ctrl_c = k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(making) = self.selection.as_ref().map(|s| s.sel.making) {
            // Selecting lines with `v`: the scroll keys move its end; y, Ctrl+c or Enter copy it.
            if making == Making::Keys {
                match k.code {
                    KeyCode::Char('j') | KeyCode::Down => self.extend(1),
                    KeyCode::Char('k') | KeyCode::Up => self.extend(-1),
                    KeyCode::PageDown | KeyCode::Char(' ') => self.extend(page),
                    KeyCode::PageUp => self.extend(-page),
                    KeyCode::Char('g') | KeyCode::Home => self.extend(-(self.body_len as isize)),
                    KeyCode::Char('G') | KeyCode::End => self.extend(self.body_len as isize),
                    KeyCode::Char('y') | KeyCode::Enter => return self.copy(true),
                    _ if ctrl_c => return self.copy(true),
                    KeyCode::Esc => self.selection = None,
                    KeyCode::Char('q') => return Outcome::Quit,
                    _ => {}
                }
                return Outcome::Redraw;
            }
            // A selection made with the mouse stays until Esc: y or Ctrl+c copy it again.
            match k.code {
                KeyCode::Char('y') => return self.copy(false),
                _ if ctrl_c => return self.copy(false),
                KeyCode::Esc => {
                    self.selection = None;
                    return Outcome::Redraw;
                }
                _ => {}
            }
        }
        match k.code {
            KeyCode::Char('q') => return Outcome::Quit,
            // Ctrl+c copies, when something is selected (above); it never closes the pane.
            _ if ctrl_c => {}
            KeyCode::Char('v' | 'V') => self.select_lines(),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(page),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(false),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(true),
            KeyCode::Tab => self.step(true),
            KeyCode::BackTab => self.step(false),
            KeyCode::Enter if !self.entries.is_empty() => self.toggle(self.sel),
            KeyCode::Char('f') => {
                self.following = true;
                if let Some(newest) = self.newest() {
                    self.open = Some(newest);
                    self.sel = newest;
                }
            }
            KeyCode::Char('x') if !self.entries.is_empty() => self.remove(self.sel),
            KeyCode::Char('e') => {
                if let Some(e) = self.entries.get(self.sel) {
                    return Outcome::Edit(e.path.clone());
                }
            }
            _ => {}
        }
        Outcome::Redraw
    }

    /// The open section's file.
    fn open_path(&self) -> Option<&Path> {
        self.open.and_then(|o| self.entries.get(o)).map(|e| e.path.as_path())
    }

    /// Forgets a selection whose file closed or changed.
    fn check_selection(&mut self) {
        let stale = match &self.selection {
            Some(s) => self.open_path() != Some(s.path.as_path()) || mtime(&s.path) != s.mtime,
            None => false,
        };
        if stale {
            self.selection = None;
        }
    }

    /// Starts a selection in the open section. Selecting stops following, so the agent's next file doesn't
    /// replace what you're selecting.
    fn start_selection(&mut self, sel: Selection) {
        let Some(path) = self.open_path().map(Path::to_path_buf) else { return };
        let mtime = mtime(&path);
        self.selection = Some(Selected { sel, path, mtime });
        self.following = false;
    }

    /// `v`: whole lines, starting at the first line on screen.
    fn select_lines(&mut self) {
        if self.body_rows > 0 {
            let top = self.scroll_top();
            self.start_selection(Selection::keys(top));
        }
    }

    fn scroll_top(&self) -> usize {
        self.open_path().and_then(|p| self.scroll.get(p)).copied().unwrap_or(0)
    }

    /// Moves a line selection's end `n` lines, scrolling to keep it on screen.
    fn extend(&mut self, n: isize) {
        let len = self.body_len;
        let Some(s) = self.selection.as_mut() else { return };
        s.sel.extend(n, len);
        let head = s.sel.head.0;
        let top = self.scroll_top();
        if head < top {
            self.scroll_by(head as isize - top as isize);
        } else if head >= top + self.body_rows {
            self.scroll_by((head + 1 - self.body_rows) as isize - top as isize);
        }
    }

    /// The selection's text for the clipboard; `clear` ends the selection (one made with keys).
    fn copy(&mut self, clear: bool) -> Outcome {
        let (Some(o), Some(s)) = (self.open, self.selection.as_ref()) else { return Outcome::Redraw };
        let sel = s.sel.clone();
        let width = self.body_width.max(1);
        let text = sel.text(self.body(o, width), 0);
        if clear {
            self.selection = None;
        }
        if text.len() > select::MAX_COPY {
            self.note = Some(format!("too big to copy ({:.1} MB)", text.len() as f64 / f64::from(1 << 20)));
            Outcome::Redraw
        } else if text.is_empty() {
            self.note = Some("nothing to copy: the lines are blank".into());
            Outcome::Redraw
        } else {
            Outcome::Copy(text)
        }
    }

    /// The point in the open section's text at screen cell (`col`, `row`). Outside the section, `None`, unless
    /// `clamp`, which takes the nearest line.
    fn point_at(&self, col: u16, row: u16, clamp: bool) -> Option<select::Point> {
        let a = self.body_area;
        if self.body_rows == 0 || self.body_len == 0 {
            return None;
        }
        if !clamp && (row < a.y || row >= a.bottom() || col < a.x || col >= a.right()) {
            return None;
        }
        let r = row.clamp(a.y, a.bottom() - 1) - a.y;
        let line = self.scroll_top() + usize::from(r);
        if line >= self.body_len {
            return Some((self.body_len - 1, usize::MAX));
        }
        Some((line, usize::from(col.saturating_sub(self.body_x))))
    }

    /// The section written last.
    fn newest(&self) -> Option<usize> {
        (0..self.entries.len()).max_by(|&a, &b| self.entries[a].written.total_cmp(&self.entries[b].written))
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Outcome {
        self.check_selection();
        let making = self.selection.as_ref().map(|s| s.sel.making);
        match m.kind {
            MouseEventKind::ScrollDown => self.scroll_by(3),
            MouseEventKind::ScrollUp => self.scroll_by(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.selection = None;
                if let Some(&(_, i)) = self.header_rows.iter().find(|(y, _)| *y == m.row) {
                    self.toggle(i);
                } else if let Some(at) = self.point_at(m.column, m.row, false) {
                    self.start_selection(Selection::press(at));
                }
            }
            // Dragging above or below the open section scrolls it.
            MouseEventKind::Drag(MouseButton::Left) if making == Some(Making::Mouse) => {
                if m.row < self.body_area.y {
                    self.scroll_by(-1);
                } else if m.row >= self.body_area.bottom() {
                    self.scroll_by(1);
                }
                if let (Some(at), Some(s)) = (self.point_at(m.column, m.row, true), self.selection.as_mut()) {
                    s.sel.drag(at);
                }
            }
            MouseEventKind::Up(MouseButton::Left) if making == Some(Making::Mouse) => {
                let shown = self.selection.as_ref().is_some_and(|s| s.sel.shown());
                if !shown {
                    // A click in the text selects nothing.
                    self.selection = None;
                    return Outcome::Redraw;
                }
                if let Some(s) = self.selection.as_mut() {
                    s.sel.making = Making::Done;
                }
                return self.copy(false);
            }
            _ => {}
        }
        Outcome::Redraw
    }

    /// The open file's lines at `width`, rendered again only when the file or the width changed.
    fn body(&mut self, i: usize, width: usize) -> &[Line<'static>] {
        let path = self.entries[i].path.clone();
        let m = mtime(&path);
        let fresh = matches!(self.cache.get(&path), Some((t, w, _)) if *t == m && *w == width);
        if !fresh {
            let lines = match fs::read_to_string(&path) {
                Ok(src) => markdown(&src, width).0,
                Err(e) => vec![Line::from(format!("can't read {}: {e}", path.display()))],
            };
            self.cache.insert(path.clone(), (m, width, lines));
        }
        &self.cache[&path].2
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, now: f64) {
        self.check_selection();
        self.header_rows.clear();
        self.body_rows = 0;
        self.body_len = 0;
        if area.height == 0 || area.width < 4 {
            return;
        }
        let dim = Style::default().fg(Color::DarkGray);
        let n = self.entries.len();
        let follow = if self.following { "following" } else { "f follows" };
        let title = match (&self.note, self.selection.as_ref().filter(|s| s.sel.shown()).map(|s| s.sel.making)) {
            (Some(note), _) => format!(" {note}"),
            (None, Some(Making::Keys)) => " j/k extend · y copy · Esc cancel".to_owned(),
            (None, Some(_)) => " Ctrl+c copy · Esc clear".to_owned(),
            (None, None) => format!(" docs · {n} · {follow}"),
        };
        buf.set_stringn(area.x, area.y, &title, usize::from(area.width), dim);
        let avail = usize::from(area.height - 1);
        if n == 0 {
            let lines = [
                " No Markdown files yet.",
                "",
                " The .md files that an agent in this tab",
                " writes in its folder show here.",
                "",
                " q closes · Ctrl+t d reopens",
            ];
            for (k, l) in lines.iter().enumerate().take(avail) {
                buf.set_stringn(area.x, area.y + 1 + k as u16, l, usize::from(area.width), dim);
            }
            return;
        }
        // Rows from the top: each header, and the open section's lines under its header.
        let body_h = match self.open {
            Some(_) if avail > n => avail - n,
            _ => 0,
        };
        let mut rows: Vec<Option<usize>> = Vec::new(); // Some(entry) for a header, None for a body line
        for i in 0..n {
            rows.push(Some(i));
            if self.open == Some(i) {
                rows.extend(std::iter::repeat_n(None, body_h));
            }
        }
        let start = if rows.len() > avail { self.sel.saturating_sub(avail - 1).min(rows.len() - avail) } else { 0 };
        let width = usize::from(area.width);
        let text_w = width.saturating_sub(2).max(1);
        let mut body_line = 0;
        let mut body: Vec<Line<'static>> = Vec::new();
        let mut scroll = 0;
        if let Some(o) = self.open.filter(|_| body_h > 0) {
            body = self.body(o, text_w).to_vec();
            self.body_rows = body_h;
            self.body_len = body.len();
            self.body_width = text_w;
            self.body_x = area.x + 2;
            let max = body.len().saturating_sub(body_h);
            let s = self.scroll.entry(self.entries[o].path.clone()).or_insert(0);
            *s = (*s).min(max);
            scroll = *s;
            self.entries[o].seen = mtime(&self.entries[o].path);
        }
        for (r, row) in rows.iter().enumerate().skip(start).take(avail) {
            let y = area.y + 1 + (r - start) as u16;
            match *row {
                Some(i) => {
                    self.header_rows.push((y, i));
                    let e = &self.entries[i];
                    let open = self.open == Some(i);
                    let changed = !open && mtime(&e.path).is_some_and(|m| e.seen.is_none_or(|s| m > s));
                    let right = format!("{}{} ", if changed { "● " } else { "" }, age(e.written, now));
                    let arrow = if open { "▾ " } else { "▸ " };
                    let room = width.saturating_sub(arrow.width() + right.width() + 1);
                    let name = fit_left(&e.label, room);
                    let mut style = Style::default();
                    if open {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    if i == self.sel {
                        style = style.bg(Color::Indexed(236));
                        for x in area.x..area.x + area.width {
                            buf[(x, y)].set_style(style);
                        }
                    }
                    buf.set_stringn(area.x, y, format!("{arrow}{name}"), width, style);
                    let rx = area.x + area.width - right.width() as u16;
                    let right_style = if changed { style.fg(Color::Yellow) } else { style.fg(Color::DarkGray) };
                    buf.set_stringn(rx, y, &right, right.width(), right_style);
                }
                None => {
                    if body_line == 0 {
                        self.body_area = Rect::new(area.x, y, area.width, body_h as u16);
                    }
                    if let Some(line) = body.get(scroll + body_line) {
                        buf.set_line(area.x + 2, y, line, area.width.saturating_sub(2));
                    }
                    body_line += 1;
                }
            }
        }
        if let Some(s) = self.selection.as_ref().filter(|s| s.sel.shown() && self.body_rows > 0) {
            s.sel.draw(buf, &body, scroll, 0, self.body_x, self.body_area);
        }
    }
}

/// Puts the terminal back as it was, also after a panic.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), DisableMouseCapture, terminal::LeaveAlternateScreen, cursor::Show);
        let _ = terminal::disable_raw_mode();
    }
}

/// Copies `text` the way a program does: OSC 52, which `tos` passes on to the clipboard and announces in the
/// status bar.
fn osc52(text: &str) {
    use base64::Engine;
    use std::io::Write;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut out = io::stdout();
    let _ = write!(out, "\x1b]52;c;{encoded}\x07");
    let _ = out.flush();
}

enum Msg {
    Term(Event),
    Changed,
    Tick,
}

/// Runs the docs pane on `list` until `q`.
pub fn run(list: PathBuf) -> anyhow::Result<()> {
    let (tx, rx) = mpsc::channel::<Msg>();
    let keys = tx.clone();
    thread::spawn(move || {
        while let Ok(ev) = ct::read() {
            if keys.send(Msg::Term(ev)).is_err() {
                break;
            }
        }
    });
    let ticks = tx.clone();
    thread::spawn(move || {
        // Ages on the headers ("5m") move on.
        while ticks.send(Msg::Tick).is_ok() {
            thread::sleep(Duration::from_secs(20));
        }
    });
    let changes = tx;
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
            let _ = changes.send(Msg::Changed);
        }
    })
    .context("watching files")?;
    let mut watched: HashSet<PathBuf> = HashSet::new();
    let mut watch = |dir: &Path, watcher: &mut notify::RecommendedWatcher| {
        if watched.insert(dir.to_path_buf()) {
            let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
        }
    };
    if let Some(dir) = list.parent() {
        watch(dir, &mut watcher);
    }

    terminal::enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), terminal::EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
    let mut term: Terminal<CrosstermBackend<Stdout>> = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut docs = Docs::new();
    loop {
        docs.ingest(&fs::read_to_string(&list).unwrap_or_default());
        docs.forget_deleted();
        for e in &docs.entries {
            if let Some(dir) = e.path.parent() {
                watch(dir, &mut watcher);
            }
        }
        term.draw(|f| {
            let area = f.area();
            docs.draw(area, f.buffer_mut(), now_secs());
        })?;
        let mut msg = rx.recv()?;
        loop {
            match msg {
                Msg::Term(Event::Key(k)) if k.kind != KeyEventKind::Release => match docs.key(k) {
                    Outcome::Quit => return Ok(()),
                    Outcome::Edit(path) => {
                        // `tos edit` opens the file in a new pane beside this one.
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe)
                                .arg("edit")
                                .arg(path)
                                .stdin(std::process::Stdio::null())
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn();
                        }
                    }
                    Outcome::Copy(text) => osc52(&text),
                    Outcome::Redraw => {}
                },
                Msg::Term(Event::Mouse(m)) => {
                    if let Outcome::Copy(text) = docs.mouse(m) {
                        osc52(&text);
                    }
                }
                Msg::Term(_) | Msg::Changed | Msg::Tick => {}
            }
            // Take whatever else is waiting before drawing, so a burst of changes draws once.
            match rx.try_recv() {
                Ok(next) => msg = next,
                Err(_) => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_line(path: &Path, cwd: &Path, t: f64) -> String {
        format!("{}\n", serde_json::json!({ "path": path, "cwd": cwd, "t": t }))
    }

    fn screen(docs: &mut Docs, w: u16, h: u16) -> Vec<String> {
        let area = Rect::new(0, 0, w, h);
        let mut buf = Buffer::empty(area);
        docs.draw(area, &mut buf, 1000.0);
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol().to_owned()).collect::<String>().trim_end().to_owned())
            .collect()
    }

    #[test]
    fn newest_first_and_following_opens_the_latest_write() {
        let dir = tempfile::tempdir().unwrap();
        let w = dir.path();
        fs::write(w.join("a.md"), "# Alpha\n\nfirst file").unwrap();
        fs::write(w.join("b.md"), "# Beta\n\nsecond file").unwrap();
        let mut docs = Docs::new();
        let mut list = list_line(&w.join("a.md"), w, 990.0);
        docs.ingest(&list);
        list += &list_line(&w.join("b.md"), w, 995.0);
        docs.ingest(&list);
        let rows = screen(&mut docs, 40, 10);
        assert_eq!(rows[0], " docs · 2 · following");
        assert!(rows[1].starts_with("▾ b.md") && rows[1].ends_with("now"), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("Beta")), "{rows:?}");
        assert!(rows[9].starts_with("▸ a.md"), "the other header stays at the bottom: {rows:?}");

        // a.md is written again: it doesn't move, and following opens it.
        list += &list_line(&w.join("a.md"), w, 999.0);
        docs.ingest(&list);
        let rows = screen(&mut docs, 40, 10);
        assert!(rows[1].starts_with("▸ b.md") && rows[2].starts_with("▾ a.md"), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("Alpha")));
    }

    #[test]
    fn choosing_a_section_stops_following_and_marks_changes() {
        let dir = tempfile::tempdir().unwrap();
        let w = dir.path();
        fs::write(w.join("a.md"), "# A").unwrap();
        fs::write(w.join("b.md"), "# B").unwrap();
        let mut docs = Docs::new();
        let mut list = list_line(&w.join("a.md"), w, 990.0) + &list_line(&w.join("b.md"), w, 991.0);
        docs.ingest(&list);
        screen(&mut docs, 40, 8);
        // Click a.md's header (row 7: the last line, under b.md's open section).
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 7,
            modifiers: KeyModifiers::NONE,
        };
        docs.mouse(click);
        assert_eq!((docs.open, docs.following), (Some(1), false));
        // b.md changes on disk and in the list: its header gets ●, a.md stays open.
        std::thread::sleep(Duration::from_millis(20));
        fs::write(w.join("b.md"), "# B again").unwrap();
        list += &list_line(&w.join("b.md"), w, 998.0);
        docs.ingest(&list);
        let rows = screen(&mut docs, 40, 8);
        assert!(rows[1].starts_with("▸ b.md") && rows[1].contains("●"), "{rows:?}");
        assert!(rows[2].starts_with("▾ a.md"), "{rows:?}");
        // f follows again: b.md opens.
        docs.key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE));
        assert_eq!(docs.open, Some(0));
    }

    #[test]
    fn deleted_files_and_x_leave_the_list() {
        let dir = tempfile::tempdir().unwrap();
        let w = dir.path();
        for f in ["a.md", "b.md", "c.md"] {
            fs::write(w.join(f), "x").unwrap();
        }
        let mut docs = Docs::new();
        let list: String =
            ["a.md", "b.md", "c.md"].iter().enumerate().map(|(i, f)| list_line(&w.join(f), w, i as f64)).collect();
        docs.ingest(&list);
        assert_eq!(docs.open, Some(0)); // c.md
        fs::remove_file(w.join("c.md")).unwrap();
        docs.forget_deleted();
        assert_eq!(docs.entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["b.md", "a.md"]);
        assert_eq!(docs.open, None);
        docs.sel = 1;
        docs.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(docs.entries.len(), 1);
        assert_eq!(docs.sel, 0);
    }

    fn mouse_at(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// The screen cell where `word` starts.
    fn cell_of(rows: &[String], word: &str) -> (u16, u16) {
        let (row, line) = rows.iter().enumerate().find(|(_, l)| l.contains(word)).expect(word);
        (line[..line.find(word).unwrap()].chars().count() as u16, row as u16)
    }

    /// A docs pane showing one file, open.
    fn one_file(text: &str) -> (tempfile::TempDir, Docs, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.md"), text).unwrap();
        let mut docs = Docs::new();
        docs.ingest(&list_line(&dir.path().join("a.md"), dir.path(), 999.0));
        let rows = screen(&mut docs, 40, 10);
        (dir, docs, rows)
    }

    #[test]
    fn dragging_in_the_open_file_copies_it_and_it_stays_selected() {
        let (_dir, mut docs, rows) = one_file("alpha beta\n\ngamma delta\n");
        let (bx, by) = cell_of(&rows, "beta");
        let (dx, dy) = cell_of(&rows, "delta");
        let left = MouseButton::Left;
        assert_eq!(docs.mouse(mouse_at(MouseEventKind::Down(left), bx, by)), Outcome::Redraw);
        assert!(!docs.following, "selecting stops following");
        docs.mouse(mouse_at(MouseEventKind::Drag(left), dx + 4, dy));
        let copied = docs.mouse(mouse_at(MouseEventKind::Up(left), dx + 4, dy));
        assert_eq!(copied, Outcome::Copy("beta\n\ngamma delta".into()));

        // It stays highlighted, the title says how to copy it again, and Ctrl+c does.
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        docs.draw(area, &mut buf, 1000.0);
        assert!(buf[(bx, by)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(bx - 1, by)].modifier.contains(Modifier::REVERSED));
        assert_eq!(screen(&mut docs, 40, 10)[0], " Ctrl+c copy · Esc clear");
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(docs.key(ctrl_c), Outcome::Copy("beta\n\ngamma delta".into()));
        // Esc clears it; then Ctrl+c does nothing, and never closes the pane.
        docs.key(key(KeyCode::Esc));
        assert!(docs.selection.is_none());
        assert_eq!(docs.key(ctrl_c), Outcome::Redraw);
    }

    #[test]
    fn a_click_in_the_text_selects_nothing() {
        let (_dir, mut docs, rows) = one_file("alpha beta\n");
        let (x, y) = cell_of(&rows, "beta");
        docs.mouse(mouse_at(MouseEventKind::Down(MouseButton::Left), x, y));
        assert_eq!(docs.mouse(mouse_at(MouseEventKind::Up(MouseButton::Left), x, y)), Outcome::Redraw);
        assert!(docs.selection.is_none());
    }

    #[test]
    fn v_selects_lines_from_the_top_and_y_copies_them() {
        let (_dir, mut docs, _) = one_file("one\n\ntwo\n\nthree\n");
        docs.key(key(KeyCode::Char('v')));
        assert_eq!(screen(&mut docs, 40, 10)[0], " j/k extend · y copy · Esc cancel");
        docs.key(key(KeyCode::Char('j')));
        docs.key(key(KeyCode::Char('j')));
        assert_eq!(docs.key(key(KeyCode::Char('y'))), Outcome::Copy("one\n\ntwo".into()));
        assert!(docs.selection.is_none(), "copying lines ends the selection");
        // Esc cancels without copying.
        docs.key(key(KeyCode::Char('V')));
        docs.key(key(KeyCode::Esc));
        assert!(docs.selection.is_none());
    }

    #[test]
    fn the_selection_clears_when_the_file_changes_or_another_opens() {
        let (dir, mut docs, _) = one_file("one\n");
        docs.key(key(KeyCode::Char('v')));
        std::thread::sleep(Duration::from_millis(20));
        fs::write(dir.path().join("a.md"), "one, changed\n").unwrap();
        screen(&mut docs, 40, 10);
        assert!(docs.selection.is_none());
        // During a line selection Enter copies it, rather than closing the section.
        docs.key(key(KeyCode::Char('v')));
        assert_eq!(docs.key(key(KeyCode::Enter)), Outcome::Copy("one, changed".into()));
        assert_eq!(docs.open, Some(0));
        // Closing the section (Enter, with nothing selected) clears a mouse selection.
        let rows = screen(&mut docs, 40, 10);
        let (x, y) = cell_of(&rows, "changed");
        docs.mouse(mouse_at(MouseEventKind::Down(MouseButton::Left), x, y));
        docs.mouse(mouse_at(MouseEventKind::Drag(MouseButton::Left), x + 3, y));
        docs.mouse(mouse_at(MouseEventKind::Up(MouseButton::Left), x + 3, y));
        assert!(docs.selection.is_some());
        docs.key(key(KeyCode::Enter));
        screen(&mut docs, 40, 10);
        assert_eq!(docs.open, None);
        assert!(docs.selection.is_none());
    }

    #[test]
    fn half_written_lines_wait() {
        let mut docs = Docs::new();
        docs.ingest("{\"path\": \"/nowhere/a.md\", \"t\": 1}");
        assert!(docs.entries.is_empty());
        docs.ingest("{\"path\": \"/nowhere/a.md\", \"t\": 1}\n");
        assert_eq!(docs.entries.len(), 1);
    }

    #[test]
    fn labels_and_ages() {
        assert_eq!(label(Path::new("/w/docs/x.md"), Some(Path::new("/w"))), "docs/x.md");
        assert_eq!(label(Path::new("/elsewhere/x.md"), Some(Path::new("/w"))), "/elsewhere/x.md");
        assert_eq!(age(100.0, 130.0), "now");
        assert_eq!(age(100.0, 400.0), "5m");
        assert_eq!(age(0.0, 7200.0), "2h");
        assert_eq!(age(0.0, 200_000.0), "2d");
        assert_eq!(fit_left("docs/plans/x.md", 8), "…ns/x.md");
        assert_eq!(fit_left("abcdef", 4), "…def");
        assert_eq!(fit_left("abc", 4), "abc");
    }
}
