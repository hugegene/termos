//! The built-in finder: a file list and a preview, drawn by the server. Enter opens Neovim in an embedded
//! terminal inside the preview area.

mod grid;
mod highlight;
pub mod view;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use finder_core::{Entry, Kind, Limits};
use notify::{EventKind, RecursiveMode, Watcher};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Ev;
use super::layout::inner;
use super::pane::TermPane;
use super::render::{Cursor, TermDecor, draw_frame, draw_term, fill, put_str};
use crate::protocol::PaneId;
use view::{Body, View};

/// Below this width the finder shows one side at a time (list, or preview when it has focus).
const NARROW: u16 = 60;

/// Half-block colours for an image: (top, bottom) per cell.
type ImageCells = Vec<Vec<(Color, Color)>>;

/// What a finder saves for a restore (step 4).
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, PartialEq)]
pub struct FinderState {
    pub dir: PathBuf,
    pub selected: Option<String>,
    pub show_hidden: bool,
    pub focus_preview: bool,
    pub scroll: usize,
    /// The file open in the editor.
    pub editing: Option<PathBuf>,
}

pub enum FinderEvent {
    View { path: PathBuf, width: u16, view: View },
    Changed(Vec<PathBuf>),
}

/// An image the server should draw for the client in `area`: as a real image if the client's terminal can,
/// else as blocks.
pub struct ImageSlot {
    pub id: u64,
    pub img: std::sync::Arc<image::RgbImage>,
    pub area: Rect,
}

/// What the server should do after a finder key or click.
pub enum Action {
    None,
    OpenEditor(PathBuf),
    /// Show a short message on the status bar.
    Flash(String),
}

struct Request {
    path: PathBuf,
    kind: Kind,
    size: Option<u64>,
    width: u16,
}

pub struct Finder {
    pub dir: PathBuf,
    entries: Vec<Entry>,
    visible: Vec<usize>,
    selected: usize,
    list_top: usize,
    show_hidden: bool,
    filter: String,
    filtering: bool,
    focus_preview: bool,
    preview_path: Option<PathBuf>,
    preview_kind: Option<Kind>,
    view: Option<View>,
    view_width: u16,
    scroll: usize,
    image_cache: RefCell<Option<((u16, u16), ImageCells)>>,
    /// The embedded editor's pane, while editing.
    pub editor: Option<PaneId>,
    /// The CSV grid editor, while editing a CSV file.
    grid: Option<grid::Grid>,
    pub editing: Option<PathBuf>,
    rect: Rect,
    /// Draw the list and the preview in boxes (zellij-style pane frames) instead of a separator line.
    pub frames: bool,
    requests: Sender<Request>,
    watcher: Option<notify::RecommendedWatcher>,
    pending_g: bool,
    last_click: Option<(Instant, usize)>,
    message: Option<String>,
}

fn spawn_worker(id: PaneId, tx: Sender<Ev>) -> Sender<Request> {
    let (rtx, rrx) = mpsc::channel::<Request>();
    thread::spawn(move || {
        while let Ok(mut req) = rrx.recv() {
            // Skip requests that were overtaken while we were busy.
            while let Ok(newer) = rrx.try_recv() {
                req = newer;
            }
            let preview = finder_core::load_preview(&req.path, &req.kind, Limits::default());
            let view = view::build(preview, &req.kind, req.size, req.width);
            let ev = Ev::Finder(id, FinderEvent::View { path: req.path, width: req.width, view });
            if tx.send(ev).is_err() {
                break;
            }
        }
    });
    rtx
}

fn watcher(id: PaneId, tx: Sender<Ev>) -> Option<notify::RecommendedWatcher> {
    notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        // Reading files for previews causes access (and atime) events; only real changes matter.
        let relevant = match ev.kind {
            EventKind::Create(_) | EventKind::Remove(_) => true,
            EventKind::Modify(notify::event::ModifyKind::Metadata(_)) => false,
            EventKind::Modify(_) => true,
            _ => false,
        };
        if relevant {
            let _ = tx.send(Ev::Finder(id, FinderEvent::Changed(ev.paths)));
        }
    })
    .ok()
}

/// Draws an image as coloured half-blocks, centred across the top of `area`.
pub fn draw_blocks(buf: &mut Buffer, cells: &[Vec<(Color, Color)>], area: Rect) {
    let w = cells.first().map_or(0, |r| r.len() as u16);
    let x0 = area.x + area.width.saturating_sub(w) / 2;
    for (row, line) in cells.iter().enumerate() {
        for (col, (top, bottom)) in line.iter().enumerate() {
            if let Some(c) = buf.cell_mut((x0 + col as u16, area.y + row as u16)) {
                c.set_symbol("▀").set_fg(*top).set_bg(*bottom);
            }
        }
    }
}

pub fn home_relative(p: &Path) -> String {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(rest) = p.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
    }
    p.display().to_string()
}

impl Finder {
    pub fn new(id: PaneId, dir: PathBuf, rect: Rect, frames: bool, tx: Sender<Ev>) -> Self {
        let mut f = Finder {
            dir: dir.clone(),
            entries: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            list_top: 0,
            show_hidden: false,
            filter: String::new(),
            filtering: false,
            focus_preview: false,
            preview_path: None,
            preview_kind: None,
            view: None,
            view_width: 0,
            scroll: 0,
            image_cache: RefCell::new(None),
            editor: None,
            grid: None,
            editing: None,
            rect,
            frames,
            requests: spawn_worker(id, tx.clone()),
            watcher: watcher(id, tx),
            pending_g: false,
            last_click: None,
            message: None,
        };
        f.cd(dir, None);
        f
    }

    pub fn state(&self) -> FinderState {
        FinderState {
            dir: self.dir.clone(),
            selected: self.current().map(|e| e.name.clone()),
            show_hidden: self.show_hidden,
            focus_preview: self.focus_preview,
            scroll: self.scroll,
            editing: self.editing.clone(),
        }
    }

    /// Puts back a saved state (the folder comes from `new`).
    pub fn restore_state(&mut self, state: &FinderState) {
        self.show_hidden = state.show_hidden;
        let dir = self.dir.clone();
        self.cd(dir, state.selected.clone());
        self.focus_preview = state.focus_preview;
        self.scroll = state.scroll;
    }

    // ----- layout -----

    /// (list, separator, preview) areas inside `rect`. In a narrow pane one side gets everything. With
    /// frames the two boxes touch and there is no separator.
    fn areas(&self, rect: Rect) -> (Rect, Option<Rect>, Rect) {
        if rect.width < NARROW {
            let empty = Rect::new(rect.x, rect.y, 0, rect.height);
            let preview = self.focus_preview || self.editor.is_some() || self.grid.is_some();
            return if preview { (empty, None, rect) } else { (rect, None, empty) };
        }
        let list_w = (rect.width * 35 / 100).clamp(16, rect.width - 20);
        if self.frames {
            return (
                Rect::new(rect.x, rect.y, list_w, rect.height),
                None,
                Rect::new(rect.x + list_w, rect.y, rect.width - list_w, rect.height),
            );
        }
        (
            Rect::new(rect.x, rect.y, list_w, rect.height),
            Some(Rect::new(rect.x + list_w, rect.y, 1, rect.height)),
            Rect::new(rect.x + list_w + 1, rect.y, rect.width - list_w - 1, rect.height),
        )
    }

    /// Where preview content (and the editor) goes: inside the preview box, or below the header line.
    pub fn body_rect(&self, rect: Rect) -> Rect {
        let (_, _, p) = self.areas(rect);
        if self.frames { inner(p) } else { Rect::new(p.x, p.y + 1, p.width, p.height.saturating_sub(1)) }
    }

    /// Where the list's entries go inside the list area: inside its box, or below the title line.
    fn entries_rect(&self, list: Rect) -> Rect {
        if self.frames { inner(list) } else { Rect::new(list.x, list.y + 1, list.width, list.height.saturating_sub(1)) }
    }

    fn list_rows(&self) -> usize {
        usize::from(self.rect.height.saturating_sub(if self.frames { 2 } else { 1 })).max(1)
    }

    fn preview_rows(&self) -> usize {
        usize::from(self.body_rect(self.rect).height).max(1)
    }

    fn preview_width(&self) -> u16 {
        // One column of left padding.
        self.body_rect(self.rect).width.saturating_sub(1).max(10)
    }

    /// Called when the pane is resized.
    pub fn resize(&mut self, rect: Rect) {
        self.rect = rect;
        let body = self.body_rect(rect);
        if let Some(g) = self.grid.as_mut() {
            g.resize(body.width, body.height);
        }
        self.ensure_visible();
        if self.preview_width() != self.view_width {
            self.request_preview(true);
        }
    }

    // ----- listing -----

    fn current(&self) -> Option<&Entry> {
        self.visible.get(self.selected).and_then(|i| self.entries.get(*i))
    }

    fn reload(&mut self) {
        let keep = self.current().map(|e| e.name.clone());
        match finder_core::list_dir(&self.dir, self.show_hidden) {
            Ok(entries) => {
                self.entries = entries;
                self.message = None;
            }
            Err(e) => {
                self.entries.clear();
                self.message = Some(format!("can't read folder: {e}"));
            }
        }
        self.refilter(keep.as_deref());
    }

    fn refilter(&mut self, keep: Option<&str>) {
        self.visible = finder_core::filter(&self.entries, &self.filter);
        self.selected = keep
            .and_then(|name| self.visible.iter().position(|i| self.entries[*i].name == name))
            .unwrap_or(0)
            .min(self.visible.len().saturating_sub(1));
        self.ensure_visible();
        self.request_preview(false);
    }

    fn cd(&mut self, dir: PathBuf, select: Option<String>) {
        if let Some(w) = &mut self.watcher {
            let _ = w.unwatch(&self.dir);
            let _ = w.watch(&dir, RecursiveMode::NonRecursive);
        }
        self.dir = dir;
        self.filter.clear();
        self.filtering = false;
        self.list_top = 0;
        self.selected = 0;
        self.entries.clear();
        self.reload();
        if let Some(name) = select {
            self.refilter(Some(&name));
        }
    }

    fn ensure_visible(&mut self) {
        let rows = self.list_rows();
        if self.selected < self.list_top {
            self.list_top = self.selected;
        } else if self.selected >= self.list_top + rows {
            self.list_top = self.selected + 1 - rows;
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let last = self.visible.len() - 1;
        self.selected = self.selected.saturating_add_signed(delta).min(last);
        self.ensure_visible();
        self.request_preview(false);
    }

    fn select(&mut self, index: usize) {
        if index < self.visible.len() {
            self.selected = index;
            self.ensure_visible();
            self.request_preview(false);
        }
    }

    // ----- preview -----

    fn request_preview(&mut self, force: bool) {
        let Some(entry) = self.current().cloned() else {
            self.preview_path = None;
            self.view = None;
            return;
        };
        let width = self.preview_width();
        if !force && self.preview_path.as_ref() == Some(&entry.path) && self.view_width == width {
            return;
        }
        if self.preview_path.as_ref() != Some(&entry.path) {
            self.scroll = 0;
            self.view = None;
        }
        let kind = if entry.is_dir { Kind::Dir } else { finder_core::detect(&entry.path) };
        self.preview_path = Some(entry.path.clone());
        self.preview_kind = Some(kind.clone());
        self.view_width = width;
        let size = (!entry.is_dir).then_some(entry.size);
        let _ = self.requests.send(Request { path: entry.path, kind, size, width });
    }

    pub fn on_event(&mut self, ev: FinderEvent) {
        match ev {
            FinderEvent::View { path, width, view } => {
                if self.preview_path.as_ref() == Some(&path) && self.view_width == width {
                    self.view = Some(view);
                    self.image_cache.replace(None);
                }
            }
            FinderEvent::Changed(paths) => {
                let preview_changed = self.preview_path.as_ref().is_some_and(|p| paths.contains(p));
                self.reload();
                if preview_changed {
                    self.request_preview(true);
                }
            }
        }
    }

    /// The editor closed: show the (possibly changed) file again.
    pub fn editor_closed(&mut self) {
        self.editor = None;
        self.editing = None;
        self.reload();
        self.request_preview(true);
    }

    fn line_count(&self) -> usize {
        match self.view.as_ref().map(|v| &v.body) {
            Some(Body::Lines(lines)) => lines.len(),
            _ => 0,
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        let max = self.line_count().saturating_sub(self.preview_rows());
        self.scroll = self.scroll.saturating_add_signed(delta).min(max);
    }

    fn editable(&self) -> Option<PathBuf> {
        let entry = self.current()?;
        (!entry.is_dir && finder_core::detect(&entry.path).is_editable()).then(|| entry.path.clone())
    }

    /// Enter / double-click: open a folder, edit a file, or look at anything else.
    fn activate(&mut self) -> Action {
        let Some(entry) = self.current().cloned() else { return Action::None };
        if entry.is_dir {
            self.cd(entry.path, None);
            return Action::None;
        }
        if self.editable().is_some() {
            return self.edit_current();
        }
        self.focus_preview = true;
        Action::None
    }

    /// Edits the selected file: CSV and TSV in the grid, other text in Neovim.
    fn edit_current(&mut self) -> Action {
        let Some(path) = self.editable() else { return Action::None };
        let finder_core::Kind::Csv { delimiter } = finder_core::detect(&path) else {
            return Action::OpenEditor(path);
        };
        match grid::Grid::open(&path, delimiter) {
            Ok(g) => {
                self.grid = Some(g);
                // In a narrow pane the preview side only shows once the grid is open, so size it now.
                let body = self.body_rect(self.rect);
                if let Some(g) = self.grid.as_mut() {
                    g.resize(body.width, body.height);
                }
                Action::None
            }
            Err(e) => Action::Flash(e),
        }
    }

    fn up(&mut self) {
        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
            let name = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
            self.cd(parent, name);
        }
    }

    // ----- input -----

    pub fn handle_key(&mut self, key: &KeyEvent) -> Action {
        if let Some(g) = self.grid.as_mut() {
            if let grid::Outcome::Close = g.handle_key(key) {
                self.grid = None;
                self.reload();
                self.request_preview(true);
            }
            return Action::None;
        }
        // E edits any text file in Neovim, CSV included.
        if key.code == KeyCode::Char('E')
            && let Some(p) = self.editable()
        {
            return Action::OpenEditor(p);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let g_pending = std::mem::take(&mut self.pending_g);
        let page = self.list_rows() as isize;
        if self.filtering {
            match key.code {
                KeyCode::Char(c) if !ctrl => {
                    self.filter.push(c);
                    self.refilter(None);
                }
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.refilter(None);
                }
                KeyCode::Enter => self.filtering = false,
                KeyCode::Esc => {
                    self.filtering = false;
                    self.filter.clear();
                    self.refilter(None);
                }
                KeyCode::Down => self.move_selection(1),
                KeyCode::Up => self.move_selection(-1),
                _ => {}
            }
            return Action::None;
        }
        if self.focus_preview {
            let page = self.preview_rows() as isize;
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => self.scroll_by(1),
                KeyCode::Char('k') | KeyCode::Up => self.scroll_by(-1),
                KeyCode::Char('d') if ctrl => self.scroll_by(page / 2),
                KeyCode::Char('u') if ctrl => self.scroll_by(-page / 2),
                KeyCode::Char(' ') | KeyCode::PageDown => self.scroll_by(page),
                KeyCode::Char('b') | KeyCode::PageUp => self.scroll_by(-page),
                KeyCode::Char('g') if g_pending => self.scroll = 0,
                KeyCode::Char('g') => self.pending_g = true,
                KeyCode::Home => self.scroll = 0,
                KeyCode::Char('G') | KeyCode::End => self.scroll_by(isize::MAX / 2),
                KeyCode::Enter | KeyCode::Char('e') => return self.edit_current(),
                KeyCode::Char('h' | 'q') | KeyCode::Left | KeyCode::Esc | KeyCode::Tab => self.focus_preview = false,
                _ => {}
            }
            return Action::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('d') if ctrl => self.move_selection(page / 2),
            KeyCode::Char('u') if ctrl => self.move_selection(-page / 2),
            KeyCode::PageDown => self.move_selection(page),
            KeyCode::PageUp => self.move_selection(-page),
            KeyCode::Char('g') if g_pending => self.select(0),
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Home => self.select(0),
            KeyCode::Char('G') | KeyCode::End => self.select(self.visible.len().saturating_sub(1)),
            KeyCode::Enter => return self.activate(),
            KeyCode::Char('l') | KeyCode::Right => {
                if self.current().is_some_and(|e| e.is_dir) {
                    return self.activate();
                }
                self.focus_preview = true;
            }
            KeyCode::Char('e') => return self.edit_current(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => self.up(),
            KeyCode::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.reload();
            }
            KeyCode::Char('/') => {
                self.filtering = true;
                self.filter.clear();
            }
            KeyCode::Char('~') => {
                if let Some(home) = std::env::var_os("HOME") {
                    self.cd(home.into(), None);
                }
            }
            KeyCode::Char('r') => {
                self.reload();
                self.request_preview(true);
            }
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.refilter(None);
            }
            KeyCode::Tab => self.focus_preview = true,
            _ => {}
        }
        Action::None
    }

    /// A mouse event at absolute screen coordinates inside the finder's `rect`.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> Action {
        let (list, _, preview) = self.areas(self.rect);
        if self.grid.is_some() {
            let body = self.body_rect(self.rect);
            if let Some(g) = self.grid.as_mut()
                && super::layout::contains(preview, ev.column, ev.row)
            {
                g.handle_mouse(ev, body);
            }
            return Action::None;
        }
        let (x, y) = (ev.column, ev.row);
        let in_list = super::layout::contains(list, x, y);
        let in_preview = super::layout::contains(preview, x, y);
        match ev.kind {
            MouseEventKind::ScrollDown if in_list => self.move_selection(3),
            MouseEventKind::ScrollUp if in_list => self.move_selection(-3),
            MouseEventKind::ScrollDown if in_preview => self.scroll_by(3),
            MouseEventKind::ScrollUp if in_preview => self.scroll_by(-3),
            MouseEventKind::Down(MouseButton::Left) if in_list => {
                self.focus_preview = false;
                let rows = self.entries_rect(list);
                if !super::layout::contains(rows, x, y) {
                    return Action::None;
                }
                let index = self.list_top + usize::from(y - rows.y);
                if index >= self.visible.len() {
                    return Action::None;
                }
                let double =
                    self.last_click.is_some_and(|(t, i)| i == index && t.elapsed() < Duration::from_millis(400));
                self.select(index);
                self.last_click = Some((Instant::now(), index));
                if double {
                    self.last_click = None;
                    return self.activate();
                }
            }
            MouseEventKind::Down(MouseButton::Left) if in_preview => self.focus_preview = true,
            _ => {}
        }
        Action::None
    }

    /// One line of key hints for the status bar.
    pub fn hint(&self) -> &'static str {
        if let Some(g) = &self.grid {
            g.hint()
        } else if self.editor.is_some() {
            "editing · Alt+s save · Alt+q close"
        } else if self.filtering {
            "filter: type to narrow · Enter keep · Esc clear"
        } else if self.focus_preview {
            "preview · j/k scroll · Space/b page · Enter edit · h back"
        } else {
            "j/k move · Enter open/edit · l preview · h up · / filter · . hidden"
        }
    }

    // ----- drawing -----

    /// Draws the finder into `rect`. `accent` colours the active box's frame when the pane has focus. With
    /// `images`, a picture is left blank and handed back for the server to draw; otherwise it's drawn as blocks.
    pub fn render(
        &self,
        buf: &mut Buffer,
        rect: Rect,
        focused: bool,
        accent: Color,
        editor: Option<&TermPane>,
        images: Option<&mut Vec<ImageSlot>>,
    ) -> Option<Cursor> {
        let (list, sep, preview) = self.areas(rect);
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let frame_style = |active: bool| if active { bold.fg(accent) } else { Style::default() };
        let list_active = focused && !self.focus_preview && self.editor.is_none() && self.grid.is_none();
        if list.width > 0 {
            if self.frames {
                let title = self.list_title(usize::from(list.width.saturating_sub(4)));
                draw_frame(buf, list, &title, "", frame_style(list_active));
            } else {
                let title = format!(" {} ", self.list_title(usize::from(list.width.saturating_sub(2))));
                put_str(buf, list.x, list.y, &title, bold.fg(Color::Blue), list.width);
            }
            self.render_list(buf, self.entries_rect(list), list_active);
        }
        if let Some(sep) = sep {
            fill(buf, sep, "│", Style::default().fg(Color::DarkGray));
        }
        if preview.width == 0 {
            return None;
        }
        let header = Rect::new(preview.x, preview.y, preview.width, 1);
        let body = self.body_rect(rect);
        let name = self
            .editing
            .as_ref()
            .or(self.preview_path.as_ref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let preview_active = focused && (self.focus_preview || self.editor.is_some() || self.grid.is_some());
        if let Some(g) = &self.grid {
            let name = g.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if self.frames {
                draw_frame(buf, preview, &format!("▦ {name}"), &g.info(), frame_style(preview_active));
            } else {
                let label = format!(" ▦ {name} ");
                let w = put_str(buf, header.x, header.y, &label, bold.fg(Color::Black).bg(Color::Yellow), header.width);
                let info = format!("  {}", g.info());
                put_str(buf, header.x + w, header.y, &info, Style::default().fg(Color::DarkGray), header.width - w);
            }
            g.render(buf, body, focused);
            return None;
        }
        if let Some(editor) = editor {
            let hint = "Alt+s save · Alt+q close";
            if self.frames {
                draw_frame(buf, preview, &format!("✎ {name}"), hint, frame_style(preview_active));
            } else {
                let label = format!(" ✎ {name} ");
                let w = put_str(buf, header.x, header.y, &label, bold.fg(Color::Black).bg(Color::Yellow), header.width);
                let hint = format!("  {hint}");
                put_str(buf, header.x + w, header.y, &hint, Style::default().fg(Color::DarkGray), header.width - w);
            }
            let decor = TermDecor { show_cursor: focused, ..TermDecor::default() };
            return draw_term(buf, body, editor, &decor);
        }
        let info = self.view.as_ref().map(|v| v.info.as_str()).unwrap_or_default();
        if self.frames {
            draw_frame(buf, preview, &name, info, frame_style(preview_active));
        } else {
            let head_style = if preview_active { bold.fg(Color::Black).bg(Color::Cyan) } else { bold };
            let w = put_str(buf, header.x, header.y, &format!(" {name} "), head_style, header.width);
            if !info.is_empty() {
                let info = format!(" {info}");
                put_str(buf, header.x + w, header.y, &info, Style::default().fg(Color::DarkGray), header.width - w);
            }
        }
        let Some(view) = &self.view else {
            if self.preview_path.is_some() {
                put_str(buf, body.x + 1, body.y, "…", Style::default().fg(Color::DarkGray), body.width);
            }
            return None;
        };
        match &view.body {
            Body::Lines(lines) => {
                let max = lines.len().saturating_sub(usize::from(body.height));
                for (row, line) in lines.iter().skip(self.scroll.min(max)).take(usize::from(body.height)).enumerate() {
                    buf.set_line(body.x + 1, body.y + row as u16, line, body.width.saturating_sub(1));
                }
            }
            Body::Image(img, id) => {
                let size = (body.width.saturating_sub(2), body.height);
                if let Some(slots) = images {
                    let area = Rect::new(body.x + 1, body.y, size.0, size.1);
                    slots.push(ImageSlot { id: *id, img: img.clone(), area });
                    return None;
                }
                let mut cache = self.image_cache.borrow_mut();
                if cache.as_ref().map(|c| c.0) != Some(size) {
                    *cache = Some((size, view::image_cells(img, size.0.max(1), size.1.max(1))));
                }
                let cells = &cache.as_ref().unwrap().1;
                draw_blocks(buf, cells, Rect::new(body.x + 1, body.y, size.0, size.1));
            }
        }
        None
    }

    /// The list's title: the folder, or the filter being typed. Long paths keep their end visible.
    fn list_title(&self, max: usize) -> String {
        let title = if self.filtering || !self.filter.is_empty() {
            format!("/{}{}", self.filter, if self.filtering { "▏" } else { "" })
        } else {
            home_relative(&self.dir)
        };
        let count = title.chars().count();
        if count > max && max > 1 {
            format!("…{}", title.chars().skip(count - max + 1).collect::<String>())
        } else {
            title
        }
    }

    /// Draws the entries into `area` (inside the list box, or below the title line).
    fn render_list(&self, buf: &mut Buffer, area: Rect, active: bool) {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        if let Some(msg) = &self.message {
            put_str(buf, area.x + 1, area.y, msg, Style::default().fg(Color::Red), area.width.saturating_sub(1));
            return;
        }
        if self.visible.is_empty() {
            let text = if self.filter.is_empty() { "(empty)" } else { "(no match)" };
            put_str(buf, area.x + 1, area.y, text, Style::default().fg(Color::DarkGray), area.width.saturating_sub(1));
            return;
        }
        for (row, &i) in self.visible.iter().skip(self.list_top).take(usize::from(area.height)).enumerate() {
            let e = &self.entries[i];
            let y = area.y + row as u16;
            let mut style = if e.is_dir {
                bold.fg(Color::Blue)
            } else if e.is_symlink {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default()
            };
            if self.list_top + row == self.selected {
                style = if active { style.add_modifier(Modifier::REVERSED) } else { style.bg(Color::DarkGray) };
                fill(buf, Rect::new(area.x, y, area.width, 1), " ", style);
            }
            let name = if e.is_dir { format!(" {}/", e.name) } else { format!(" {}", e.name) };
            // Inside a box, keep a column free before the right edge.
            let max = if self.frames { area.width.saturating_sub(1) } else { area.width };
            put_str(buf, area.x, y, &name, style, max);
        }
    }

    /// The finder as plain text, for `tb dump-screen`.
    pub fn text(&self, editor: Option<&TermPane>) -> String {
        let area = Rect::new(0, 0, self.rect.width, self.rect.height);
        let mut buf = Buffer::empty(area);
        self.render(&mut buf, area, false, Color::Green, editor, None);
        let mut out = Vec::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push(line.trim_end().to_owned());
        }
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out.join("\n")
    }
}
