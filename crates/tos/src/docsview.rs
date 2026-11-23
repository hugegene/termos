//! `tos docs --list FILE`: the docs pane's program. It follows the list that the server appends to (one JSON
//! line per Markdown file an agent wrote, `server/docs_pane.rs`) and shows the files as an accordion: a header
//! line per file, newest first, and one file open below its header, drawn with the finder's Markdown renderer.
//! Below the agent's files come the other Markdown files of the git repository the pane starts in (or of its
//! folder, outside a repository), listed again by a thread of their own.
//! Text in the open file is selected and copied as in the finder's preview, with the same code (`select.rs`);
//! being a program, it copies by writing OSC 52, which `tos` passes on to the clipboard.
//! `/` filters the headers by path, with the finder's matching.
//! Plans: docs/plans/2026-11-17-agent-plan-pane.md, docs/plans/2026-11-21-preview-select-copy.md,
//! docs/plans/2026-11-23-docs-pane-filter.md.

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::io::{self, Stdout};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
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

use crate::server::finder::select::{self, Making, Selection};
use crate::server::finder::view::markdown;
use crate::{agents, paths};

/// The open file's least share of the pane's rows, when not every header fits beside it (an estimate).
const BODY_SHARE: (usize, usize) = (2, 3);
/// The folder is listed again at least this often, and no sooner than this after the last time (estimates).
const RELIST_EVERY: Duration = Duration::from_secs(20);
const RELIST_GAP: Duration = Duration::from_secs(2);
/// Outside a git repository, the walk goes this many folders deep, and stops after this many folders or files
/// (estimates): enough for a project, cheap in `~`.
const WALK_DEPTH: usize = 3;
const WALK_DIRS: usize = 1000;
const WALK_FILES: usize = 1000;

/// One file in the list.
#[derive(Debug)]
pub struct Entry {
    pub path: PathBuf,
    /// The path as shown: relative to the repository (or the agent's folder), else under `~`.
    pub label: String,
    /// When the agent last wrote it (seconds since 1970, from the list); for the folder's other files, when
    /// they last changed as they were listed.
    pub written: f64,
    /// The file's modification time when it was last on screen, open, or when it was listed.
    pub seen: Option<SystemTime>,
    /// The agent wrote it, rather than it being one of the folder's other files.
    pub by_agent: bool,
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

/// While a filter is typed after `/`: what was showing before, for Esc to bring back.
#[derive(Default)]
struct Typing {
    open: Option<PathBuf>,
    following: bool,
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
    /// The folder whose Markdown files are listed: the git repository the pane is in, or the pane's folder.
    root: Option<PathBuf>,
    /// The folder has been listed once: files found later are new, and go on top.
    listed: bool,
    /// Files dropped with `x`: listing the folder again doesn't bring them back, the agent writing them does.
    dropped: HashSet<PathBuf>,
    /// With nothing open, the list of headers: the first one on screen, and how many rows it had last time.
    list_top: usize,
    list_rows: usize,
    /// Typed after `/`: only the headers whose label has its letters in order show, best match first.
    filter: String,
    /// The filter is being typed: keys go to it.
    typing: Option<Typing>,
}

/// How a path shows: relative to `root` or else `cwd` if inside one, else with `~` for the home folder.
fn label(path: &Path, root: Option<&Path>, cwd: Option<&Path>) -> String {
    for dir in [root, cwd].into_iter().flatten() {
        if let Ok(rel) = path.strip_prefix(dir) {
            return rel.to_string_lossy().into_owned();
        }
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

fn secs(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

fn now_secs() -> f64 {
    secs(SystemTime::now())
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|x| x.eq_ignore_ascii_case("md") || x.eq_ignore_ascii_case("markdown"))
}

/// The folder the docs pane lists: the top of the git repository that `dir` is in, else `dir`.
pub fn root_of(dir: &Path) -> PathBuf {
    let top = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-toplevel"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    match top {
        Ok(out) if out.status.success() && out.stdout.len() > 1 => {
            PathBuf::from(OsStr::from_bytes(out.stdout.strip_suffix(b"\n").unwrap_or(&out.stdout)))
        }
        _ => dir.to_path_buf(),
    }
}

/// The Markdown files under `root`. In a git repository, its tracked files and the new ones that `.gitignore`
/// doesn't ignore; elsewhere, a walk of the folder that stops early (`WALK_*`). Never files in agents' own
/// folders, such as Claude's memory in `~/.claude`.
pub fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let git = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
    let found = match git {
        Ok(out) if out.status.success() => out
            .stdout
            .split(|&b| b == 0)
            .filter(|p| !p.is_empty())
            .map(|p| root.join(OsStr::from_bytes(p)))
            .filter(|p| is_markdown(p))
            .collect(),
        _ => walk(root),
    };
    let mut files: Vec<PathBuf> =
        found.into_iter().filter(|p| p.is_file() && !agents::in_agent_dir(p, paths::home_dirs())).collect();
    // A file with a merge conflict comes once per side.
    files.dedup();
    files
}

/// The Markdown files in `root` and its folders, nearest first, skipping hidden folders, `node_modules` and
/// `target`, within the `WALK_*` limits.
fn walk(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = std::collections::VecDeque::from([(root.to_path_buf(), 0)]);
    let mut read = 0;
    while let Some((dir, depth)) = dirs.pop_front() {
        read += 1;
        if read > WALK_DIRS {
            break;
        }
        let Ok(items) = fs::read_dir(&dir) else { continue };
        for item in items.flatten() {
            let name = item.file_name();
            let name = name.to_string_lossy();
            let Ok(kind) = item.file_type() else { continue };
            if name.starts_with('.') {
                continue;
            }
            let path = item.path();
            if kind.is_dir() {
                if depth < WALK_DEPTH && name != "node_modules" && name != "target" {
                    dirs.push_back((path, depth + 1));
                }
            } else if is_markdown(&path) {
                out.push(path);
                if out.len() >= WALK_FILES {
                    return out;
                }
            }
        }
    }
    out
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

    /// A docs pane that labels files relative to `root`, the folder it lists.
    pub fn with_root(root: Option<PathBuf>) -> Docs {
        Docs { root, ..Docs::new() }
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
            self.dropped.remove(&path);
            let i = match self.entries.iter().position(|e| e.path == path) {
                Some(i) if self.entries[i].by_agent => {
                    self.entries[i].written = t;
                    i
                }
                // One of the folder's files, written by the agent for the first time: it joins the agent's, on top.
                Some(i) => {
                    self.move_to_top(i);
                    self.entries[0].by_agent = true;
                    self.entries[0].written = t;
                    0
                }
                None => {
                    let label = label(&path, self.root.as_deref(), cwd);
                    self.insert_top(Entry { path, label, written: t, seen: None, by_agent: true });
                    0
                }
            };
            if self.following {
                self.open = Some(i);
                self.sel = i;
            }
        }
    }

    /// Adds the folder's Markdown files that aren't listed yet. The first listing goes below the agent's files,
    /// most recently changed first; files that later listings find are new, so they go on top.
    pub fn add_files(&mut self, files: Vec<PathBuf>) {
        let known: HashSet<&Path> = self.entries.iter().map(|e| e.path.as_path()).collect();
        let mut new: Vec<(Option<SystemTime>, PathBuf)> = files
            .into_iter()
            .filter(|f| !known.contains(f.as_path()) && !self.dropped.contains(f))
            .map(|f| (mtime(&f), f))
            .collect();
        new.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let root = self.root.clone();
        let entry = |(m, path): (Option<SystemTime>, PathBuf)| Entry {
            label: label(&path, root.as_deref(), None),
            written: m.map_or(0.0, secs),
            seen: m,
            by_agent: false,
            path,
        };
        if self.listed {
            for item in new.into_iter().rev() {
                self.insert_top(entry(item));
            }
        } else {
            self.entries.extend(new.into_iter().map(entry));
            self.listed = true;
        }
    }

    /// Puts a new entry on top; the open and highlighted ones stay the same files.
    fn insert_top(&mut self, e: Entry) {
        self.entries.insert(0, e);
        self.open = self.open.map(|o| o + 1);
        self.sel = if self.entries.len() > 1 { self.sel + 1 } else { 0 };
        if self.list_top > 0 {
            self.list_top += 1;
        }
    }

    /// Moves entry `i` to the top; the open and highlighted ones stay the same files.
    fn move_to_top(&mut self, i: usize) {
        let e = self.entries.remove(i);
        self.entries.insert(0, e);
        let moved = |x: usize| match x {
            x if x == i => 0,
            x if x < i => x + 1,
            x => x,
        };
        self.open = self.open.map(moved);
        self.sel = moved(self.sel);
    }

    /// The entries whose headers show, in order: all of them, or with a filter those that match, best first.
    fn shown(&self) -> Vec<usize> {
        finder_core::filter_names(self.entries.iter().map(|e| e.label.as_str()), &self.filter)
    }

    /// Whether the highlighted entry's header shows.
    fn sel_shown(&self) -> bool {
        self.shown().contains(&self.sel)
    }

    /// Moves the highlight `delta` headers down (or up) among those that show.
    fn move_sel(&mut self, delta: isize) {
        let shown = self.shown();
        if let Some(last) = shown.len().checked_sub(1) {
            let at = shown.iter().position(|&i| i == self.sel).map_or(0, |at| at.saturating_add_signed(delta));
            self.sel = shown[at.min(last)];
        }
    }

    /// `/`: starts typing a filter. The open file closes, so the matches show as a list.
    fn start_filter(&mut self) {
        let open = self.open_path().map(Path::to_path_buf);
        self.typing = Some(Typing { open, following: self.following });
        self.filter.clear();
        self.open = None;
        self.following = false;
        self.selection = None;
    }

    /// A key while the filter is typed.
    fn filter_key(&mut self, k: KeyEvent) {
        match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.push(c);
                self.refilter();
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.refilter();
            }
            KeyCode::Down => self.move_sel(1),
            KeyCode::Up => self.move_sel(-1),
            // Keep the filter and open the highlighted file.
            KeyCode::Enter => {
                self.typing = None;
                if self.sel_shown() {
                    self.open = Some(self.sel);
                }
            }
            // Cancel: what was showing before comes back, or with following, the file written last.
            KeyCode::Esc => {
                let before = self.typing.take().unwrap_or_default();
                self.filter.clear();
                self.following = before.following;
                self.open = if before.following {
                    self.newest()
                } else {
                    before.open.and_then(|p| self.entries.iter().position(|e| e.path == p))
                };
                if let Some(o) = self.open {
                    self.sel = o;
                }
            }
            _ => {}
        }
    }

    /// The filter changed: the best match is highlighted.
    fn refilter(&mut self) {
        if let Some(&best) = self.shown().first() {
            self.sel = best;
        }
        self.list_top = 0;
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

    /// Opens the next (`down`) or previous section among those that show.
    fn step(&mut self, down: bool) {
        self.move_sel(if down { 1 } else { -1 });
        if self.sel_shown() {
            self.open = Some(self.sel);
            self.following = false;
        }
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
        if self.typing.is_some() {
            self.filter_key(k);
            return Outcome::Redraw;
        }
        let page = self.body_rows.max(2) as isize - 1;
        let list_page = self.list_rows.max(2) as isize - 1;
        // Nothing open: the keys that scroll a file move the highlight through the list instead.
        let listing = self.open.is_none();
        let all = self.entries.len() as isize;
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
            KeyCode::Char('j') | KeyCode::Down if listing => self.move_sel(1),
            KeyCode::Char('k') | KeyCode::Up if listing => self.move_sel(-1),
            KeyCode::PageDown | KeyCode::Char(' ') if listing => self.move_sel(list_page),
            KeyCode::PageUp if listing => self.move_sel(-list_page),
            KeyCode::Char('g') | KeyCode::Home if listing => self.move_sel(-all),
            KeyCode::Char('G') | KeyCode::End if listing => self.move_sel(all),
            KeyCode::Char('j') | KeyCode::Down => self.scroll_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(page),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Char('g') | KeyCode::Home => self.scroll_to(false),
            KeyCode::Char('G') | KeyCode::End => self.scroll_to(true),
            KeyCode::Tab => self.step(true),
            KeyCode::BackTab => self.step(false),
            KeyCode::Enter if self.sel_shown() => self.toggle(self.sel),
            KeyCode::Char('/') => self.start_filter(),
            KeyCode::Esc if !self.filter.is_empty() => self.filter.clear(),
            KeyCode::Char('f') => {
                // The agent's next file might not match a filter.
                self.filter.clear();
                self.following = true;
                if let Some(newest) = self.newest() {
                    self.open = Some(newest);
                    self.sel = newest;
                }
            }
            KeyCode::Char('x') => {
                let shown = self.shown();
                if let Some(at) = shown.iter().position(|&i| i == self.sel) {
                    self.dropped.insert(self.entries[self.sel].path.clone());
                    self.remove(self.sel);
                    // The highlight goes to the header that took the dropped one's place.
                    let shown = self.shown();
                    if let Some(&i) = shown.get(at.min(shown.len().saturating_sub(1))) {
                        self.sel = i;
                    }
                }
            }
            KeyCode::Char('e') if self.sel_shown() => return Outcome::Edit(self.entries[self.sel].path.clone()),
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
            MouseEventKind::ScrollDown if self.open.is_none() => self.move_sel(3),
            MouseEventKind::ScrollUp if self.open.is_none() => self.move_sel(-3),
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
        let shown = self.shown();
        let matching = shown.len();
        let follow = if self.following { "following" } else { "f follows" };
        let title = match (&self.note, self.selection.as_ref().filter(|s| s.sel.shown()).map(|s| s.sel.making)) {
            (Some(note), _) => format!(" {note}"),
            (None, Some(Making::Keys)) => " j/k extend · y copy · Esc cancel".to_owned(),
            (None, Some(_)) => " Ctrl+c copy · Esc clear".to_owned(),
            (None, None) if self.typing.is_some() => {
                format!(" /{}▏ · {matching} of {n} · Enter opens · Esc cancels", self.filter)
            }
            (None, None) if !self.filter.is_empty() => format!(" /{} · {matching} of {n} · Esc clears", self.filter),
            (None, None) => format!(" docs · {n} · {follow}"),
        };
        buf.set_stringn(area.x, area.y, &title, usize::from(area.width), dim);
        let avail = usize::from(area.height - 1);
        self.list_rows = avail;
        if n == 0 {
            let place = self.root.as_deref().map_or_else(|| "this folder".to_owned(), |r| label(r, None, None));
            let lines = [
                " No Markdown files yet.".to_owned(),
                String::new(),
                format!(" The .md files in {place},"),
                " and those an agent in this tab writes,".to_owned(),
                " show here.".to_owned(),
                String::new(),
                " q closes · Ctrl+t d reopens".to_owned(),
            ];
            for (k, l) in lines.iter().enumerate().take(avail) {
                buf.set_stringn(area.x, area.y + 1 + k as u16, l, usize::from(area.width), dim);
            }
            return;
        }
        if matching == 0 {
            if avail > 0 {
                buf.set_stringn(area.x, area.y + 1, " (no match)", usize::from(area.width), dim);
            }
            return;
        }
        // Rows from the top: each header that shows, and the open section's lines under its header. The open
        // section gets the rows the headers leave, and at least 2/3 of them: when the headers don't all fit beside
        // that, only those nearest the open section show.
        let open = self.open.filter(|o| shown.contains(o));
        let body_h = match open {
            Some(_) => avail.saturating_sub(matching).max(avail * BODY_SHARE.0 / BODY_SHARE.1),
            None => 0,
        };
        let mut rows: Vec<Option<usize>> = Vec::new(); // Some(entry) for a header, None for a body line
        for &i in &shown {
            rows.push(Some(i));
            if open == Some(i) {
                rows.extend(std::iter::repeat_n(None, body_h));
            }
        }
        // Where a header is among those that show: its row, as the headers above it take one row each.
        let row_of = |i: usize| shown.iter().position(|&s| s == i).unwrap_or(0);
        let start = if rows.len() <= avail {
            0
        } else if let Some(o) = open.filter(|_| body_h > 0) {
            // The open section whole on screen, with the headers nearest it: half above, half below.
            let above = (avail - body_h - 1) / 2;
            row_of(o).saturating_sub(above).min(rows.len() - avail)
        } else {
            // A list of headers, scrolled as little as keeps the highlighted one on screen.
            let sel = row_of(self.sel);
            if sel < self.list_top {
                self.list_top = sel;
            } else if sel >= self.list_top + avail {
                self.list_top = sel + 1 - avail;
            }
            self.list_top = self.list_top.min(rows.len() - avail);
            self.list_top
        };
        let width = usize::from(area.width);
        let text_w = width.saturating_sub(2).max(1);
        let mut body_line = 0;
        let mut body: Vec<Line<'static>> = Vec::new();
        let mut scroll = 0;
        if let Some(o) = open.filter(|_| body_h > 0) {
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
                    let is_open = open == Some(i);
                    let m = mtime(&e.path);
                    let changed = !is_open && m.is_some_and(|m| e.seen.is_none_or(|s| m > s));
                    // The agent's files: when it wrote them. The others: when they last changed.
                    let when = if e.by_agent { e.written } else { m.map_or(e.written, secs) };
                    let right = format!("{}{} ", if changed { "● " } else { "" }, age(when, now));
                    let arrow = if is_open { "▾ " } else { "▸ " };
                    let room = width.saturating_sub(arrow.width() + right.width() + 1);
                    let name = fit_left(&e.label, room);
                    let mut style = Style::default();
                    if is_open {
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
    /// The folder's Markdown files, listed again.
    Files(Vec<PathBuf>),
}

/// Runs the docs pane on `list` until `q`.
pub fn run(list: PathBuf) -> anyhow::Result<()> {
    // The pane starts in the agent's folder, or the focused pane's: list the Markdown files of its repository.
    let root = std::env::current_dir().ok().map(|d| root_of(&d));
    let (tx, rx) = mpsc::channel::<Msg>();
    let keys = tx.clone();
    thread::spawn(move || {
        while let Ok(ev) = ct::read() {
            if keys.send(Msg::Term(ev)).is_err() {
                break;
            }
        }
    });
    // The lister: the folder's Markdown files, every RELIST_EVERY, or after a change but at most every RELIST_GAP.
    // Its own thread, so a slow git never holds up the keys. Each listing also moves the headers' ages ("5m") on.
    let (relist, wanted) = mpsc::channel::<()>();
    let files = tx.clone();
    let dir = root.clone();
    thread::spawn(move || {
        loop {
            let found = dir.as_deref().map(markdown_files).unwrap_or_default();
            if files.send(Msg::Files(found)).is_err() {
                break;
            }
            thread::sleep(RELIST_GAP);
            match wanted.recv_timeout(RELIST_EVERY - RELIST_GAP) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => while wanted.try_recv().is_ok() {},
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
    let changes = tx;
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| !matches!(e.kind, notify::EventKind::Access(_))) {
            let _ = changes.send(Msg::Changed);
            let _ = relist.send(());
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
    // The repository's top folder, not all of it: a recursive watch of target/ alone takes thousands of watches.
    if let Some(dir) = &root {
        watch(dir, &mut watcher);
    }

    terminal::enable_raw_mode()?;
    let _restore = Restore;
    execute!(io::stdout(), terminal::EnterAlternateScreen, EnableMouseCapture, cursor::Hide)?;
    let mut term: Terminal<CrosstermBackend<Stdout>> = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut docs = Docs::with_root(root);
    loop {
        docs.ingest(&fs::read_to_string(&list).unwrap_or_default());
        docs.forget_deleted();
        // The folders of the agent's files and of the open file, so that their changes redraw at once.
        let open = docs.open.and_then(|o| docs.entries.get(o));
        for e in docs.entries.iter().filter(|e| e.by_agent).chain(open) {
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
                Msg::Files(found) => docs.add_files(found),
                Msg::Term(_) | Msg::Changed => {}
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

    /// A folder of Markdown files, `a.md` changed longest ago, each one second after the last.
    fn folder(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let base = SystemTime::now() - Duration::from_secs(1000);
        let paths: Vec<PathBuf> = names
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let p = dir.path().join(n);
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(&p, format!("# {n}\n\nThe text of {n}.\n")).unwrap();
                let f = fs::File::options().write(true).open(&p).unwrap();
                f.set_modified(base + Duration::from_secs(i as u64)).unwrap();
                p
            })
            .collect();
        (dir, paths)
    }

    fn labels(docs: &Docs) -> Vec<&str> {
        docs.entries.iter().map(|e| e.label.as_str()).collect()
    }

    #[test]
    fn the_folders_files_go_below_the_agents_and_an_agents_write_brings_one_to_the_top() {
        let (dir, paths) = folder(&["a.md", "docs/b.md", "c.md", "new.md"]);
        let w = dir.path();
        let mut docs = Docs::with_root(Some(w.to_path_buf()));
        // The agent wrote new.md; then the folder is listed: the rest below, most recently changed first.
        let mut list = list_line(&paths[3], w, 990.0);
        docs.ingest(&list);
        docs.add_files(paths[..3].to_vec());
        assert_eq!(labels(&docs), ["new.md", "c.md", "docs/b.md", "a.md"]);
        assert_eq!(docs.open, Some(0), "following opened the agent's file");
        let rows = screen(&mut docs, 40, 12);
        assert!(!rows.iter().any(|r| r.contains('●')), "listed files aren't new: {rows:?}");

        // The agent writes a.md, one of the folder's: it moves to the top once and opens.
        list += &list_line(&paths[0], w, 995.0);
        docs.ingest(&list);
        assert_eq!(labels(&docs), ["a.md", "new.md", "c.md", "docs/b.md"]);
        assert_eq!(docs.open, Some(0));
        // A file found by a later listing is new: it goes on top, and the open file stays open.
        let later = w.join("later.md");
        fs::write(&later, "# Later").unwrap();
        docs.add_files(vec![later, paths[1].clone()]);
        assert_eq!(labels(&docs), ["later.md", "a.md", "new.md", "c.md", "docs/b.md"]);
        assert_eq!(docs.open, Some(1));

        // x drops c.md, and listing again doesn't bring it back; the agent writing it does.
        docs.sel = 3;
        docs.key(key(KeyCode::Char('x')));
        docs.add_files(paths.clone());
        assert!(!labels(&docs).contains(&"c.md"));
        list += &list_line(&paths[2], w, 999.0);
        docs.ingest(&list);
        assert_eq!(labels(&docs)[0], "c.md");
    }

    #[test]
    fn with_many_files_the_open_one_keeps_two_thirds_and_the_nearest_headers_show() {
        let names: Vec<String> = (0..30).map(|i| format!("f{i:02}.md")).collect();
        let (dir, paths) = folder(&names.iter().map(String::as_str).collect::<Vec<_>>());
        let mut docs = Docs::with_root(Some(dir.path().to_path_buf()));
        docs.add_files(paths);
        // Nothing open: a list. j moves the highlight, and the list scrolls to keep it on screen.
        let rows = screen(&mut docs, 40, 10);
        assert!(rows[1].starts_with("▸ f29.md") && rows[9].starts_with("▸ f21.md"), "{rows:?}");
        for _ in 0..12 {
            docs.key(key(KeyCode::Char('j')));
        }
        assert_eq!((docs.sel, docs.open), (12, None));
        let rows = screen(&mut docs, 40, 10);
        assert!(rows[9].starts_with("▸ f17.md"), "the highlighted header at the bottom: {rows:?}");

        // Enter opens f17.md: 2/3 of the 21 rows are its text, with the headers nearest it around it.
        docs.key(key(KeyCode::Enter));
        let rows = screen(&mut docs, 40, 22);
        let open = rows.iter().position(|r| r.starts_with("▾ f17.md")).expect("f17.md's header");
        assert_eq!(open, 4, "three headers above it: {rows:?}");
        assert!(rows[1].starts_with("▸ f20.md") && rows[open + 1].contains("f17.md"), "{rows:?}");
        assert!(rows[open + 15].starts_with("▸ f16.md"), "14 rows of text, then the next header: {rows:?}");
        assert!(rows[21].starts_with("▸ f14.md"), "{rows:?}");
        // Tab opens the next one, and the headers move with it.
        docs.key(key(KeyCode::Tab));
        let rows = screen(&mut docs, 40, 22);
        assert!(rows[4].starts_with("▾ f16.md"), "{rows:?}");
    }

    fn typed(docs: &mut Docs, text: &str) -> Vec<Outcome> {
        text.chars().map(|c| docs.key(key(KeyCode::Char(c)))).collect()
    }

    #[test]
    fn slash_filters_the_headers_best_match_first_and_enter_opens_one() {
        let (dir, paths) =
            folder(&["README.md", "docs/findings/agent-hooks.md", "docs/plans/agent-pane.md", "notes.md"]);
        let mut docs = Docs::with_root(Some(dir.path().to_path_buf()));
        docs.add_files(paths);
        assert_eq!(
            labels(&docs),
            ["notes.md", "docs/plans/agent-pane.md", "docs/findings/agent-hooks.md", "README.md"]
        );
        docs.key(key(KeyCode::Enter));
        assert_eq!(docs.open, Some(0), "notes.md is open");

        // `/` closes it, and typing narrows the list: both "agent" files, the earlier match first.
        docs.key(key(KeyCode::Char('/')));
        assert_eq!(docs.open, None);
        assert_eq!(screen(&mut docs, 40, 8)[0], " /▏ · 4 of 4 · Enter opens · Esc cancels");
        typed(&mut docs, "agent");
        let rows = screen(&mut docs, 50, 8);
        assert_eq!(rows[0], " /agent▏ · 2 of 4 · Enter opens · Esc cancels");
        assert!(rows[1].starts_with("▸ docs/plans/agent-pane.md"), "{rows:?}");
        assert!(rows[2].starts_with("▸ docs/findings/agent-hooks.md"), "{rows:?}");
        assert_eq!(rows[3], "", "{rows:?}");
        assert_eq!(docs.sel, 1, "the best match is highlighted");
        // Every letter types, q included; Backspace takes it back.
        assert_eq!(typed(&mut docs, "q"), [Outcome::Redraw]);
        assert_eq!(screen(&mut docs, 50, 8)[1], " (no match)");
        docs.key(key(KeyCode::Backspace));

        // ↓ and Enter open the second match and keep the filter; Tab moves only among the matches.
        docs.key(key(KeyCode::Down));
        docs.key(key(KeyCode::Enter));
        let rows = screen(&mut docs, 50, 8);
        assert_eq!(rows[0], " /agent · 2 of 4 · Esc clears");
        assert!(rows[2].starts_with("▾ docs/findings/agent-hooks.md"), "{rows:?}");
        docs.key(key(KeyCode::Tab));
        assert_eq!(docs.open, Some(2), "Tab stays on the last match");
        docs.key(key(KeyCode::BackTab));
        assert_eq!(docs.open, Some(1), "Shift+Tab opens the plan");

        // Esc clears the filter; the plan stays open.
        docs.key(key(KeyCode::Esc));
        assert_eq!(screen(&mut docs, 50, 8)[0], " docs · 4 · f follows");
        assert_eq!(docs.open, Some(1));
        // f clears a filter too, as the agent's next file might not match it.
        docs.key(key(KeyCode::Char('/')));
        typed(&mut docs, "readme");
        docs.key(key(KeyCode::Enter));
        assert_eq!(docs.open, Some(3));
        docs.key(key(KeyCode::Char('f')));
        assert!(docs.filter.is_empty() && docs.following);
    }

    #[test]
    fn esc_while_typing_brings_back_the_open_file_and_following() {
        let dir = tempfile::tempdir().unwrap();
        let w = dir.path();
        for f in ["a.md", "b.md", "c.md"] {
            fs::write(w.join(f), "x").unwrap();
        }
        let mut docs = Docs::new();
        let mut list = list_line(&w.join("a.md"), w, 990.0) + &list_line(&w.join("b.md"), w, 991.0);
        docs.ingest(&list);
        assert_eq!((docs.open, docs.following), (Some(0), true), "b.md, followed");
        docs.key(key(KeyCode::Char('/')));
        typed(&mut docs, "a");
        assert_eq!((docs.open, docs.following), (None, false));
        // The agent writes c.md while the filter is typed: it doesn't match, so it waits, hidden.
        list += &list_line(&w.join("c.md"), w, 995.0);
        docs.ingest(&list);
        assert_eq!(docs.open, None);
        let rows = screen(&mut docs, 40, 8);
        assert!(rows[1].starts_with("▸ a.md") && !rows.iter().any(|r| r.contains("c.md")), "{rows:?}");
        // Esc: following again, so c.md, the file written last, opens.
        docs.key(key(KeyCode::Esc));
        assert_eq!(labels(&docs), ["c.md", "b.md", "a.md"]);
        assert_eq!((docs.open, docs.following), (Some(0), true));
        // Without following, Esc brings back the file that was open.
        docs.key(key(KeyCode::Tab));
        assert_eq!((docs.open, docs.following), (Some(1), false), "b.md");
        docs.key(key(KeyCode::Char('/')));
        typed(&mut docs, "a");
        docs.key(key(KeyCode::Esc));
        assert_eq!((docs.open, docs.following), (Some(1), false));
    }

    #[test]
    fn a_repository_lists_its_files_and_a_plain_folder_is_walked() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().canonicalize().unwrap().join("repo");
        for f in ["README.md", "docs/plan.md", "src/main.rs", "target/out.md", "sub/deep/NOTES.markdown"] {
            let p = repo.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x").unwrap();
        }
        fs::write(repo.join(".gitignore"), "target/\n").unwrap();
        let git = Command::new("git").arg("-C").arg(&repo).args(["init", "-q"]).status();
        assert!(git.is_ok_and(|s| s.success()), "git init");
        // From a subfolder, the repository's top; untracked files count, ignored ones don't.
        assert_eq!(root_of(&repo.join("sub/deep")), repo);
        let mut found = markdown_files(&repo);
        found.sort();
        let want: Vec<PathBuf> =
            ["README.md", "docs/plan.md", "sub/deep/NOTES.markdown"].iter().map(|f| repo.join(f)).collect();
        assert_eq!(found, want);

        // Outside a repository: the folder's own, then 3 levels down, skipping hidden folders and target/.
        let plain = dir.path().canonicalize().unwrap().join("plain");
        for f in ["a.md", "x/b.md", "x/y/z/c.md", "x/y/z/w/too-deep.md", ".hidden/d.md", "target/e.md"] {
            let p = plain.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x").unwrap();
        }
        assert_eq!(root_of(&plain), plain);
        let found = markdown_files(&plain);
        assert_eq!(found, ["a.md", "x/b.md", "x/y/z/c.md"].map(|f| plain.join(f)));
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
        assert_eq!(label(Path::new("/w/docs/x.md"), None, Some(Path::new("/w"))), "docs/x.md");
        assert_eq!(label(Path::new("/w/docs/x.md"), Some(Path::new("/w/docs")), Some(Path::new("/w"))), "x.md");
        assert_eq!(
            label(Path::new("/elsewhere/x.md"), Some(Path::new("/r")), Some(Path::new("/w"))),
            "/elsewhere/x.md"
        );
        assert_eq!(age(100.0, 130.0), "now");
        assert_eq!(age(100.0, 400.0), "5m");
        assert_eq!(age(0.0, 7200.0), "2h");
        assert_eq!(age(0.0, 200_000.0), "2d");
        assert_eq!(fit_left("docs/plans/x.md", 8), "…ns/x.md");
        assert_eq!(fit_left("abcdef", 4), "…def");
        assert_eq!(fit_left("abc", 4), "abc");
    }
}
