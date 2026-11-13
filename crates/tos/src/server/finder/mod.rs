//! The built-in finder: two panes drawn by the server, the file list (`Pane::Finder`, which owns this state)
//! and the preview (`Pane::Preview`, which points at it). Enter opens Neovim in an embedded terminal inside the
//! preview pane. Plan: docs/plans/2026-11-05-finder-divider.md.

mod grid;
mod highlight;
mod menu;
mod pdf;
pub mod view;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use finder_core::link::Target;
use finder_core::pdf::PdfInfo;
use finder_core::tool::ToolError;
use finder_core::{Entry, Kind, Limits, Preview, human_size};
use menu::Popup;
use notify::{EventKind, RecursiveMode, Watcher};
use pdf::{Merge, PageImage, PdfJob, PdfResult, PdfView, Zoom};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::Ev;
use super::editor::Editor;
use super::layout::inner;
use super::pane::TermPane;
use super::render::{Cursor, TermDecor, draw_frame, draw_term, fill, frame_title_room, put_str};
use crate::protocol::PaneId;
use view::{Body, View};

/// Below this width a finder tab shows one of its panes at a time: the focused one.
pub const NARROW: u16 = 60;

/// The file list's share of a new finder tab's width, as in the zellij version.
pub const LIST_SHARE: f32 = 0.35;

/// How many files Backspace can go back through after following links.
const HISTORY: usize = 100;

/// Half-block colours for an image: (top, bottom) per cell.
type ImageCells = Vec<Vec<(Color, Color)>>;

/// Blocks for a picture, by its id and size in cells.
type BlockCache = Option<((u64, (u16, u16)), ImageCells)>;

/// The part of a PDF page on screen: which page image and rectangle it was cut from, the picture, and its id.
type Crop = ((u64, (u32, u32, u32, u32)), Arc<image::RgbImage>, u64);

/// What a finder saves for a restore (step 4).
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, PartialEq)]
pub struct FinderState {
    pub dir: PathBuf,
    pub selected: Option<String>,
    /// Hidden files (names starting with `.`) are left out (`.`). Saves from before 2026-11-07 have
    /// `show_hidden` instead, which is ignored, so those finders come back showing hidden files.
    #[serde(default)]
    pub hide_hidden: bool,
    pub scroll: usize,
    /// The file open in the editor.
    pub editing: Option<PathBuf>,
    /// PDFs show their pages instead of their text (`t`).
    #[serde(default)]
    pub pdf_pages: bool,
    /// How big PDF pages are drawn.
    #[serde(default)]
    pub pdf_zoom: Zoom,
}

pub enum FinderEvent {
    View {
        path: PathBuf,
        width: u16,
        view: View,
    },
    /// A PDF job is done (or, for `PdfResult::Slow`, under way).
    Pdf {
        path: PathBuf,
        job: PdfJob,
        info: Option<Arc<PdfInfo>>,
        result: PdfResult,
    },
    Changed(Vec<PathBuf>),
}

/// An image the server should draw for the client in `area`: as a real image if the client's terminal can,
/// else as blocks.
pub struct ImageSlot {
    pub id: u64,
    pub img: Arc<image::RgbImage>,
    pub area: Rect,
    /// Keep showing the picture that's there until this one is encoded (a PDF page being scrolled), instead of
    /// drawing blocks meanwhile.
    pub keep: bool,
}

/// What the server should do after a finder key or click.
pub enum Action {
    None,
    /// Open Neovim on the file, in the preview pane, and focus it.
    OpenEditor(PathBuf),
    /// Show a short message on the status bar.
    Flash(String),
    /// Move focus to the finder's preview pane.
    FocusPreview,
    /// Move focus to the finder's file list.
    FocusFiles,
    /// Copy this text to the clipboard (the menu's Copy name and Copy path).
    Copy(String),
    /// Copy cells from the grid; `what` says how many, for the status bar ("3 rows × 2 columns").
    CopyCells {
        text: String,
        what: String,
    },
    /// Open this file or folder in its default app, on the screen of the client whose key it was.
    OpenApp(PathBuf),
}

struct Request {
    path: PathBuf,
    kind: Kind,
    size: Option<u64>,
    width: u16,
    job: Job,
    /// The request's number. The worker stops a PDF job once a newer request has been made.
    seq: u64,
}

enum Job {
    /// A file's preview, or a folder's listing, with hidden files if the list shows them.
    Preview {
        show_hidden: bool,
    },
    Pdf(PdfJob),
    /// A video's facts, then a frame: two views, so the header shows while the frame is read.
    Video,
    /// A sheet of a spreadsheet (0-based).
    Sheet(usize),
}

pub struct Finder {
    pub dir: PathBuf,
    entries: Vec<Entry>,
    visible: Vec<usize>,
    selected: usize,
    list_top: usize,
    show_hidden: bool,
    /// How many hidden files the listing left out, for the list's title.
    hidden: usize,
    filter: String,
    filtering: bool,
    preview_path: Option<PathBuf>,
    preview_kind: Option<Kind>,
    view: Option<View>,
    view_width: u16,
    scroll: usize,
    /// Blocks for the picture on screen, by picture id and size.
    image_cache: RefCell<BlockCache>,
    /// The PDF being previewed (instead of `view`).
    pdf: Option<PdfView>,
    /// PDFs show pages rather than text (`t`); this sticks while browsing.
    pdf_pages: bool,
    pdf_zoom: Zoom,
    /// The part of the PDF page on screen.
    crop: RefCell<Option<Crop>>,
    /// The active client's cell size in pixels, so PDF pages render at the pane's real width.
    cell_px: (u16, u16),
    /// The newest request's number, shared with the worker.
    seq: Arc<AtomicU64>,
    /// The embedded editor's pane, while editing.
    pub editor: Option<PaneId>,
    /// Which editor that is, and the caption naming its keys ("editing · Ctrl+s save · Ctrl+e close").
    editor_kind: Option<Editor>,
    editor_caption: String,
    /// The CSV grid editor, while editing a CSV file, or a spreadsheet shown in the grid read-only.
    grid: Option<grid::Grid>,
    /// The sheet a spreadsheet's preview shows (`[` and `]`), from 0. Back to 0 for each new file.
    sheet: usize,
    pub editing: Option<PathBuf>,
    /// The editor or grid was opened from the list, so focus goes back there when it closes.
    edit_from_list: bool,
    /// The file list's pane.
    list_rect: Rect,
    /// The preview's pane.
    preview_rect: Rect,
    /// Draw the list and the preview in boxes (zellij-style pane frames) instead of under a title line.
    pub frames: bool,
    requests: Sender<Request>,
    watcher: Option<notify::RecommendedWatcher>,
    pending_g: bool,
    last_click: Option<(Instant, usize)>,
    message: Option<String>,
    /// The files links were followed from, and their scroll positions, for Backspace in the preview.
    history: Vec<(PathBuf, usize)>,
    /// The menu, a rename, or the "delete for good?" question, over the file list.
    popup: Option<Popup>,
    /// The command that moves a file to the trash; the path is added at the end.
    trash: Vec<String>,
}

/// What the worker remembers about a PDF: when the file last changed, what `pdfinfo` said, and whether its
/// pages render with anti-aliasing (false once one was too slow).
struct Doc {
    stamp: (u64, Option<SystemTime>),
    info: Arc<PdfInfo>,
    smooth: bool,
}

fn spawn_worker(id: PaneId, tx: Sender<Ev>, latest: Arc<AtomicU64>) -> Sender<Request> {
    let (rtx, rrx) = mpsc::channel::<Request>();
    thread::spawn(move || {
        let mut docs = HashMap::new();
        while let Ok(mut req) = rrx.recv() {
            // Skip requests that were overtaken while we were busy.
            while let Ok(newer) = rrx.try_recv() {
                req = newer;
            }
            let ev = match req.job {
                Job::Preview { show_hidden } => {
                    let preview = match req.kind {
                        // A folder's preview lists what the file list would show inside it.
                        Kind::Dir => finder_core::list_dir(&req.path, show_hidden)
                            .map_or_else(|e| Preview::Error(e.to_string()), Preview::Dir),
                        _ => finder_core::load_preview(&req.path, &req.kind, Limits::default()),
                    };
                    let view = view::build(preview, &req.kind, req.size, req.width);
                    Some(FinderEvent::View { path: req.path, width: req.width, view })
                }
                Job::Pdf(job) => {
                    let send = |ev| {
                        let _ = tx.send(Ev::Finder(id, ev));
                    };
                    pdf_job(&req.path, job, req.seq, &latest, &mut docs, &send)
                }
                Job::Video => {
                    let send = |ev| {
                        let _ = tx.send(Ev::Finder(id, ev));
                    };
                    video_job(&req, &latest, &send)
                }
                Job::Sheet(index) => {
                    let preview = finder_core::sheet::read(&req.path, index, Limits::default().max_rows)
                        .map_or_else(Preview::Error, Preview::Sheet);
                    let view = view::build(preview, &req.kind, req.size, req.width);
                    Some(FinderEvent::View { path: req.path, width: req.width, view })
                }
            };
            if let Some(ev) = ev
                && tx.send(Ev::Finder(id, ev)).is_err()
            {
                break;
            }
        }
    });
    rtx
}

/// Runs one PDF job with poppler's tools. Returns `None` if it was cancelled because a newer request came in.
/// `send` passes on the news that a page is slow while it's drawn again.
fn pdf_job(
    path: &Path,
    job: PdfJob,
    seq: u64,
    latest: &AtomicU64,
    docs: &mut HashMap<PathBuf, Doc>,
    send: &dyn Fn(FinderEvent),
) -> Option<FinderEvent> {
    let limits = Limits::default();
    let cancel = || latest.load(Ordering::Relaxed) != seq;
    let event = |info, result| FinderEvent::Pdf { path: path.to_owned(), job, info, result };
    let failed = |e: ToolError| match e {
        ToolError::Cancelled => None,
        e => Some(event(None, PdfResult::Error(e.to_string()))),
    };
    let stamp = std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()));
    let info = match docs.get(path).filter(|d| Some(d.stamp) == stamp) {
        Some(doc) => doc.info.clone(),
        None => match finder_core::pdf::info(path, &cancel) {
            Ok(info) => {
                let info = Arc::new(info);
                if docs.len() >= 32 {
                    docs.clear();
                }
                if let Some(stamp) = stamp {
                    docs.insert(path.to_owned(), Doc { stamp, info: info.clone(), smooth: true });
                }
                info
            }
            Err(e) => return failed(e),
        },
    };
    let result = match job {
        PdfJob::Text { first, .. } if first > info.pages => PdfResult::Text { pages: Vec::new(), truncated: false },
        PdfJob::Text { first, last } => {
            match finder_core::pdf::text(path, first, last.min(info.pages), limits.max_bytes, &cancel) {
                Ok((pages, truncated)) => PdfResult::Text { pages, truncated },
                Err(e) => return failed(e),
            }
        }
        PdfJob::Page { page, zoom, viewport } => {
            let width = finder_core::pdf::render_width(&info, page, zoom.width(viewport, info.aspect(page)), &limits);
            let smooth = docs.get(path).is_none_or(|d| d.smooth);
            let on_slow = || send(event(None, PdfResult::Slow));
            match finder_core::pdf::page(path, page, width, smooth, &limits, &cancel, &on_slow) {
                Ok(rendered) => {
                    if !rendered.smooth
                        && let Some(doc) = docs.get_mut(path)
                    {
                        doc.smooth = false;
                    }
                    PdfResult::Page { image: Arc::new(rendered.image) }
                }
                Err(ToolError::TimedOut) => {
                    PdfResult::Error(format!("page {page} was too slow to render · t for text"))
                }
                Err(e) => return failed(e),
            }
        }
    };
    Some(event(Some(info), result))
}

/// Reads a video's facts with ffprobe and sends them (the body says it's loading the frame), then returns the
/// view with the frame. Returns `None` if a newer request stopped it.
fn video_job(req: &Request, latest: &AtomicU64, send: &dyn Fn(FinderEvent)) -> Option<FinderEvent> {
    let cancel = || latest.load(Ordering::Relaxed) != req.seq;
    let view = |preview| FinderEvent::View {
        path: req.path.clone(),
        width: req.width,
        view: view::build(preview, &Kind::Video, req.size, req.width),
    };
    let info = match finder_core::video::info(&req.path, &cancel) {
        Ok(info) => info,
        Err(ToolError::Cancelled) => return None,
        Err(e) => return Some(view(Preview::Error(e.to_string()))),
    };
    send(view(Preview::Video { info: info.clone(), frame: None }));
    let frame = match finder_core::video::frame(&req.path, &info, &Limits::default(), &cancel) {
        Err(ToolError::Cancelled) => return None,
        other => other.map_err(|e| e.to_string()),
    };
    Some(view(Preview::Video { info, frame: Some(frame) }))
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
    if let Some(rest) = crate::paths::home_dirs().iter().find_map(|home| p.strip_prefix(home).ok()) {
        return if rest.as_os_str().is_empty() { "~".into() } else { format!("~/{}", rest.display()) };
    }
    p.display().to_string()
}

impl Finder {
    pub fn new(id: PaneId, dir: PathBuf, rect: Rect, frames: bool, tx: Sender<Ev>) -> Self {
        let seq = Arc::new(AtomicU64::new(0));
        let mut f = Finder {
            dir: dir.clone(),
            entries: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            list_top: 0,
            show_hidden: true,
            hidden: 0,
            filter: String::new(),
            filtering: false,
            preview_path: None,
            preview_kind: None,
            view: None,
            view_width: 0,
            scroll: 0,
            image_cache: RefCell::new(None),
            pdf: None,
            pdf_pages: false,
            pdf_zoom: Zoom::default(),
            crop: RefCell::new(None),
            cell_px: super::images::DEFAULT_CELL,
            seq: seq.clone(),
            editor: None,
            editor_kind: None,
            editor_caption: String::new(),
            grid: None,
            sheet: 0,
            editing: None,
            edit_from_list: false,
            list_rect: rect,
            preview_rect: rect,
            frames,
            requests: spawn_worker(id, tx.clone(), seq),
            watcher: watcher(id, tx),
            pending_g: false,
            last_click: None,
            message: None,
            history: Vec::new(),
            popup: None,
            trash: menu::trash_command(),
        };
        f.cd(dir, None);
        f
    }

    pub fn state(&self) -> FinderState {
        FinderState {
            dir: self.dir.clone(),
            selected: self.current().map(|e| e.name.clone()),
            hide_hidden: !self.show_hidden,
            scroll: self.scroll,
            editing: self.editing.clone(),
            pdf_pages: self.pdf_pages,
            pdf_zoom: self.pdf_zoom,
        }
    }

    /// Puts back a saved state (the folder comes from `new`).
    pub fn restore_state(&mut self, state: &FinderState) {
        self.show_hidden = !state.hide_hidden;
        self.pdf_pages = state.pdf_pages;
        self.pdf_zoom = state.pdf_zoom;
        let dir = self.dir.clone();
        self.cd(dir, state.selected.clone());
        self.scroll = state.scroll;
    }

    // ----- layout -----

    /// Where preview content (and the editor and grid) goes in a preview pane at `p`: inside its box, or below
    /// its header line.
    fn body_of(&self, p: Rect) -> Rect {
        if self.frames { inner(p) } else { Rect::new(p.x, p.y + 1, p.width, p.height.saturating_sub(1)) }
    }

    /// Where preview content (and the editor and grid) goes in the preview pane.
    pub fn body_rect(&self) -> Rect {
        self.body_of(self.preview_rect)
    }

    /// Where the list's entries go inside the list area: inside its box, or below the title line.
    fn entries_rect(&self, list: Rect) -> Rect {
        if self.frames { inner(list) } else { Rect::new(list.x, list.y + 1, list.width, list.height.saturating_sub(1)) }
    }

    fn list_rows(&self) -> usize {
        usize::from(self.list_rect.height.saturating_sub(if self.frames { 2 } else { 1 })).max(1)
    }

    fn preview_rows(&self) -> usize {
        usize::from(self.body_rect().height).max(1)
    }

    fn preview_width(&self) -> u16 {
        // One column of left padding.
        self.body_rect().width.saturating_sub(1).max(10)
    }

    /// The page view's size in pixels: the picture area of the preview, at the active client's cell size.
    fn viewport_px(&self) -> (u32, u32) {
        let body = self.body_rect();
        let (cw, ch) = self.cell_px;
        (u32::from(body.width.saturating_sub(2)) * u32::from(cw), u32::from(body.height) * u32::from(ch))
    }

    /// Called when the file list's pane is resized.
    pub fn resize_list(&mut self, rect: Rect) {
        self.list_rect = rect;
        self.ensure_visible();
    }

    /// Called when the preview's pane is resized, or the active client's cell size (in pixels) changes.
    pub fn resize_preview(&mut self, rect: Rect, cell: (u16, u16)) {
        self.preview_rect = rect;
        self.cell_px = cell;
        let body = self.body_rect();
        if let Some(g) = self.grid.as_mut() {
            g.resize(body.width, body.height);
        }
        let width = self.preview_width();
        if let Some(p) = self.pdf.as_mut() {
            // A PDF wraps its text again and renders its page again; it isn't reloaded.
            self.view_width = width;
            p.relayout(usize::from(width));
            self.pdf_request();
        } else if width != self.view_width {
            self.request_preview(true);
        }
    }

    // ----- listing -----

    fn current(&self) -> Option<&Entry> {
        self.visible.get(self.selected).and_then(|i| self.entries.get(*i))
    }

    fn reload(&mut self) {
        let keep = self.current().map(|e| e.name.clone());
        // Hidden files are listed and then left out, so the title can say how many there are.
        match finder_core::list_dir(&self.dir, true) {
            Ok(mut entries) => {
                let all = entries.len();
                if !self.show_hidden {
                    entries.retain(|e| !e.name.starts_with('.'));
                }
                self.hidden = all - entries.len();
                self.entries = entries;
                self.message = None;
            }
            Err(e) => {
                self.hidden = 0;
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
        self.popup = None;
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
            self.pdf = None;
            return;
        };
        let width = self.preview_width();
        if !force && self.preview_path.as_ref() == Some(&entry.path) && self.view_width == width {
            return;
        }
        if self.preview_path.as_ref() != Some(&entry.path) {
            self.scroll = 0;
            self.view = None;
            self.sheet = 0;
        }
        let kind = if entry.is_dir { Kind::Dir } else { finder_core::detect(&entry.path) };
        self.preview_path = Some(entry.path.clone());
        self.preview_kind = Some(kind.clone());
        self.view_width = width;
        if kind == Kind::Pdf {
            self.view = None;
            self.pdf = Some(PdfView::new(entry.path, usize::from(width)));
            self.pdf_request();
            return;
        }
        self.pdf = None;
        let size = (!entry.is_dir).then_some(entry.size);
        let seq = self.next_seq();
        let job = match kind {
            Kind::Video => Job::Video,
            Kind::Sheet(_) => Job::Sheet(self.sheet),
            _ => Job::Preview { show_hidden: self.show_hidden },
        };
        let _ = self.requests.send(Request { path: entry.path, kind, size, width, job, seq });
    }

    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// The PDF shows its pages: chosen with `t`, or because it has no text.
    fn pdf_pages_view(&self) -> bool {
        self.pdf.as_ref().is_some_and(|p| self.pdf_pages || p.no_text)
    }

    /// The text view's first line on screen.
    fn text_top(&self) -> usize {
        self.scroll.min(self.line_count().saturating_sub(self.preview_rows()))
    }

    /// Asks the worker for whatever the PDF view is missing: a chunk of text, or the page at the current zoom.
    fn pdf_request(&mut self) {
        let (zoom, viewport, scroll, rows) = (self.pdf_zoom, self.viewport_px(), self.text_top(), self.preview_rows());
        let pages = self.pdf_pages;
        let Some(p) = self.pdf.as_mut() else { return };
        let Some(job) = p.wants(pages, zoom, viewport, scroll, rows, Limits::default().pdf_text_pages) else { return };
        if p.sent == Some(job) {
            return;
        }
        p.sent = Some(job);
        let path = p.path.clone();
        let seq = self.next_seq();
        let width = self.view_width;
        let _ = self.requests.send(Request { path, kind: Kind::Pdf, size: None, width, job: Job::Pdf(job), seq });
    }

    /// `t`: switches a PDF between its text and its pages, at the same page.
    fn toggle_pdf_view(&mut self) -> Action {
        let top = self.text_top();
        let Some(p) = self.pdf.as_mut() else { return Action::None };
        if p.no_text {
            return Action::Flash("this PDF has no text".into());
        }
        self.pdf_pages = !self.pdf_pages;
        p.error = None;
        if self.pdf_pages {
            p.go_to(p.page_at(top));
            p.top = 0;
        } else if let Some(line) = p.start_of(p.page) {
            self.scroll = line;
        } else {
            p.restart_text(p.page);
            self.scroll = 0;
        }
        Action::None
    }

    /// `]` and `[` in a PDF's text: the start of the next or previous page.
    fn pdf_text_jump(&mut self, forward: bool) {
        let top = self.text_top();
        let max = self.line_count().saturating_sub(self.preview_rows());
        let Some(p) = &self.pdf else { return };
        self.scroll = match p.next_start(top, forward) {
            Some(line) => line.min(max),
            None if forward => max,
            None => 0,
        };
    }

    /// Keys in a PDF's page view.
    fn pdf_page_key(&mut self, key: &KeyEvent, ctrl: bool, g_pending: bool) -> Action {
        let zoom = self.pdf_zoom;
        let (vw, vh) = self.viewport_px();
        let row = i64::from(self.cell_px.1.max(1));
        let screen = i64::from(vh.max(1));
        let pan = i64::from(vw / 4).max(1);
        let Some(p) = self.pdf.as_mut() else { return Action::None };
        let zoom_to = |new: Zoom, p: &mut PdfView| {
            p.rezoom(zoom, new, (vw, vh));
            new
        };
        let mut action = Action::None;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => p.scroll(3 * row, zoom, vh),
            KeyCode::Char('k') | KeyCode::Up => p.scroll(-3 * row, zoom, vh),
            KeyCode::Char('d') if ctrl => p.scroll(screen / 2, zoom, vh),
            KeyCode::Char('u') if ctrl => p.scroll(-screen / 2, zoom, vh),
            KeyCode::Char(' ') | KeyCode::PageDown => p.scroll(screen, zoom, vh),
            KeyCode::Char('b') | KeyCode::PageUp => p.scroll(-screen, zoom, vh),
            KeyCode::Char(']') => p.go_to(p.page + 1),
            KeyCode::Char('[') => p.go_to(p.page.saturating_sub(1)),
            KeyCode::Char('g') if g_pending => p.go_to(1),
            KeyCode::Char('g') => self.pending_g = true,
            KeyCode::Home => p.go_to(1),
            KeyCode::Char('G') | KeyCode::End => p.go_to(u32::MAX),
            KeyCode::Char('+' | '=') => self.pdf_zoom = zoom_to(zoom.zoom_in(), p),
            KeyCode::Char('-') => self.pdf_zoom = zoom_to(zoom.zoom_out(), p),
            KeyCode::Char('0') => self.pdf_zoom = zoom_to(Zoom::FitWidth, p),
            KeyCode::Char('l') | KeyCode::Right => {
                p.pan(pan, vw);
            }
            KeyCode::Char('h') | KeyCode::Left => {
                // Panning when the page is wider than the pane; otherwise back to the list.
                if !p.pan(-pan, vw) {
                    action = Action::FocusFiles;
                }
            }
            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Tab => action = Action::FocusFiles,
            _ => {}
        }
        action
    }

    /// The mouse wheel over the preview: scrolls text, or a PDF page.
    fn preview_wheel(&mut self, rows: isize) {
        if self.pdf_pages_view() {
            let (zoom, (_, vh), row) = (self.pdf_zoom, self.viewport_px(), i64::from(self.cell_px.1.max(1)));
            if let Some(p) = self.pdf.as_mut() {
                p.scroll(rows as i64 * row, zoom, vh);
            }
        } else {
            self.scroll_by(rows);
        }
    }

    pub fn on_event(&mut self, ev: FinderEvent) {
        match ev {
            FinderEvent::View { path, width, view } => {
                if self.preview_path.as_ref() == Some(&path) && self.view_width == width && self.pdf.is_none() {
                    self.view = Some(view);
                    self.image_cache.replace(None);
                }
            }
            FinderEvent::Pdf { path, job, info, result } => {
                let Some(p) = self.pdf.as_mut().filter(|p| p.path == path) else { return };
                if let Some(info) = info {
                    p.info = Some(info);
                }
                match (job, result) {
                    (PdfJob::Text { first, .. }, PdfResult::Text { pages, truncated }) => {
                        if let Merge::Prepended(lines) = p.add_text(first, pages, truncated) {
                            self.scroll += lines;
                        }
                    }
                    (PdfJob::Page { page, zoom, viewport }, PdfResult::Page { image }) => {
                        if page == p.page {
                            p.slow = false;
                        }
                        p.image = Some(PageImage { page, zoom, viewport, img: image, id: view::next_image_id() });
                    }
                    (PdfJob::Page { page, .. }, PdfResult::Slow) if page == p.page => p.slow = true,
                    (job, PdfResult::Error(e)) if p.sent == Some(job) => p.error = Some(e),
                    _ => {}
                }
                self.pdf_request();
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

    /// The editor or grid was opened from the list (so focus goes back there when it closes).
    pub fn edit_from_list(&self) -> bool {
        self.edit_from_list
    }

    /// The editor started in pane `id` on `path`; `kind` says which, for the hints.
    pub fn editor_opened(&mut self, id: PaneId, path: PathBuf, kind: Editor) {
        self.editor = Some(id);
        self.editing = Some(path);
        self.editor_caption = format!("editing · {}", kind.keys());
        self.editor_kind = Some(kind);
    }

    /// The editor or the grid is open, so Ctrl+s saves rather than starting scroll mode.
    pub fn is_editing(&self) -> bool {
        self.editor.is_some() || self.grid.is_some()
    }

    /// The editor closed: show the (possibly changed) file again.
    pub fn editor_closed(&mut self) {
        self.editor = None;
        self.editor_kind = None;
        self.editing = None;
        self.reload();
        self.request_preview(true);
    }

    fn line_count(&self) -> usize {
        if let Some(p) = &self.pdf {
            return p.lines.len();
        }
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

    /// Enter / double-click in the list goes in, as `→` does: it opens a folder, or focuses a file's preview.
    /// Editing is Enter in the preview (`edit_current`).
    fn activate(&mut self) -> Action {
        let Some(entry) = self.current().cloned() else { return Action::None };
        if entry.is_dir {
            self.cd(entry.path, None);
            return Action::None;
        }
        Action::FocusPreview
    }

    /// `o`: the file the preview shows (or, in the list, the selected entry) in its default app.
    fn open_in_app(&self, preview: bool) -> Action {
        let path = if preview { self.preview_path.clone() } else { None };
        match path.or_else(|| self.current().map(|e| e.path.clone())) {
            Some(path) => Action::OpenApp(path),
            None => Action::None,
        }
    }

    /// The selected file, if it's a spreadsheet.
    fn selected_sheet(&self) -> Option<PathBuf> {
        let entry = self.current().filter(|e| !e.is_dir)?;
        matches!(finder_core::detect(&entry.path), Kind::Sheet(_)).then(|| entry.path.clone())
    }

    /// The spreadsheet preview's sheet and sheet count, once it has loaded.
    fn sheet_view(&self) -> Option<(usize, usize)> {
        self.view.as_ref().and_then(|v| v.sheets).filter(|_| matches!(self.preview_kind, Some(Kind::Sheet(_))))
    }

    /// `[` and `]` in a spreadsheet's preview: the previous or next sheet.
    fn switch_sheet(&mut self, forward: bool) -> Action {
        let Some((current, count)) = self.sheet_view() else { return Action::None };
        let next = if forward { current + 1 } else { current.wrapping_sub(1) };
        // Past the first or last sheet nothing happens, as at a PDF's ends: the title already says `sheet 2/2`.
        if next >= count {
            return Action::None;
        }
        self.sheet = next;
        self.scroll = 0;
        self.request_preview(true);
        Action::None
    }

    /// Opens sheet `index` of a spreadsheet in the grid, read-only, keeping the place in `keep`'s grid.
    fn open_sheet(&mut self, path: &Path, index: usize, keep: Option<&grid::Grid>) -> Action {
        match finder_core::sheet::read(path, index, grid::MAX_ROWS) {
            Ok(sheet) => {
                self.sheet = sheet.current;
                let mut g = grid::Grid::from_sheet(path, sheet);
                let body = self.body_rect();
                g.resize(body.width, body.height);
                if let Some(old) = keep {
                    g.keep_place(old);
                }
                self.grid = Some(g);
                Action::FocusPreview
            }
            Err(e) => Action::Flash(format!("can't open it: {e}")),
        }
    }

    /// Edits the selected file: CSV and TSV in the grid, other text in Neovim. Both show in the preview pane,
    /// which gets focus. `from_list` says focus goes back to the list afterwards.
    fn edit_current(&mut self, from_list: bool) -> Action {
        if let Some(path) = self.selected_sheet() {
            self.edit_from_list = from_list;
            let index = if self.preview_path.as_ref() == Some(&path) { self.sheet } else { 0 };
            return self.open_sheet(&path, index, None);
        }
        let Some(path) = self.editable() else { return self.cant_edit() };
        self.edit_from_list = from_list;
        let finder_core::Kind::Csv { delimiter } = finder_core::detect(&path) else {
            return Action::OpenEditor(path);
        };
        match grid::Grid::open(&path, delimiter) {
            Ok(g) => {
                self.grid = Some(g);
                let body = self.body_rect();
                if let Some(g) = self.grid.as_mut() {
                    g.resize(body.width, body.height);
                }
                Action::FocusPreview
            }
            Err(e) => Action::Flash(e),
        }
    }

    /// Enter on a file tos can't edit: say so, and point to `o`.
    fn cant_edit(&self) -> Action {
        let Some(entry) = self.current().filter(|e| !e.is_dir) else { return Action::None };
        let what = match entry.path.extension() {
            Some(ext) => format!(".{}", ext.to_string_lossy()),
            None => entry.name.clone(),
        };
        Action::Flash(format!("tos can't edit {what}; o opens it in its default app"))
    }

    // ----- links -----

    /// The link under a click in the preview, at absolute screen coordinates.
    fn link_at(&self, x: u16, y: u16) -> Option<String> {
        let view = self.view.as_ref().filter(|_| self.pdf.is_none())?;
        let body = self.body_rect();
        // Lines are drawn after one column of padding.
        if !super::layout::contains(body, x, y) || x == body.x {
            return None;
        }
        let (line, col) = (self.text_top() + usize::from(y - body.y), usize::from(x - body.x - 1));
        view.links.iter().find(|l| l.line == line && (l.start..l.end).contains(&col)).map(|l| l.url.clone())
    }

    /// A click on a link in the preview: shows its target, remembering where we were for Backspace.
    fn follow(&mut self, url: &str) -> Action {
        let Some(from) = self.preview_path.clone() else { return Action::None };
        let base = from.parent().unwrap_or(Path::new("/"));
        let target = match finder_core::link::resolve(url, base) {
            Some(Target::Path(p)) => p,
            Some(Target::Heading(_)) => return Action::Flash("links to headings aren't followed yet".into()),
            Some(Target::External(url)) => return Action::Flash(format!("not opened: {url}")),
            None => return Action::None,
        };
        if target == from {
            return Action::None;
        }
        if std::fs::metadata(&target).is_err() {
            return Action::Flash(format!("no such file: {url}"));
        }
        if self.history.len() >= HISTORY {
            self.history.remove(0);
        }
        self.history.push((from, self.scroll));
        self.show(&target)
    }

    /// Backspace in the preview: back to the file the last link was followed from.
    fn back(&mut self) -> Action {
        let Some((path, scroll)) = self.history.pop() else { return Action::None };
        let action = self.show(&path);
        // Kept until the view arrives, as after a restore.
        self.scroll = scroll;
        action
    }

    /// Lists the folder `path` is in and selects it, so the preview shows it.
    fn show(&mut self, path: &Path) -> Action {
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Action::None };
        let name = name.to_string_lossy().into_owned();
        if name.starts_with('.') {
            self.show_hidden = true;
        }
        self.cd(dir.to_path_buf(), Some(name));
        if self.current().map(|e| e.path.as_path()) != Some(path) {
            return Action::Flash(format!("can't show {}", home_relative(path)));
        }
        Action::None
    }

    fn up(&mut self) {
        if let Some(parent) = self.dir.parent().map(Path::to_path_buf) {
            let name = self.dir.file_name().map(|n| n.to_string_lossy().into_owned());
            self.cd(parent, name);
        }
    }

    // ----- input -----

    /// A key typed in the file list (`preview` false) or in the preview pane (`preview` true).
    pub fn handle_key(&mut self, key: &KeyEvent, preview: bool) -> Action {
        let action = self.handle_key_inner(key, preview);
        self.pdf_request();
        action
    }

    fn handle_key_inner(&mut self, key: &KeyEvent, preview: bool) -> Action {
        if let Some(g) = self.grid.as_mut() {
            match g.handle_key(key) {
                grid::Outcome::Stay => {}
                grid::Outcome::Close => {
                    self.grid = None;
                    self.reload();
                    self.request_preview(true);
                    if self.edit_from_list {
                        return Action::FocusFiles;
                    }
                }
                grid::Outcome::Sheet(index) => {
                    let old = self.grid.take();
                    let path = old.as_ref().map(|g| g.path.clone()).unwrap_or_default();
                    let action = self.open_sheet(&path, index, old.as_ref());
                    if self.grid.is_none() {
                        // It couldn't be read; keep the sheet that was there.
                        self.grid = old;
                    }
                    return action;
                }
                grid::Outcome::Copy { text, what } => return Action::CopyCells { text, what },
                grid::Outcome::OpenApp(path) => return Action::OpenApp(path),
            }
            return Action::None;
        }
        if let Some(action) = self.popup_key(key) {
            return action;
        }
        // E edits any text file in Neovim, CSV included.
        if key.code == KeyCode::Char('E')
            && let Some(p) = self.editable()
        {
            self.edit_from_list = !preview;
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
        if key.code == KeyCode::Char('o') && key.modifiers.is_empty() {
            return self.open_in_app(preview);
        }
        if key.code == KeyCode::Char('t') && !ctrl && self.pdf.is_some() {
            return self.toggle_pdf_view();
        }
        if preview && key.code == KeyCode::Backspace {
            return self.back();
        }
        if preview && self.pdf_pages_view() {
            return self.pdf_page_key(key, ctrl, g_pending);
        }
        if preview {
            let page = self.preview_rows() as isize;
            match key.code {
                KeyCode::Char(']') if self.pdf.is_some() => self.pdf_text_jump(true),
                KeyCode::Char('[') if self.pdf.is_some() => self.pdf_text_jump(false),
                KeyCode::Char(']') if self.sheet_view().is_some() => return self.switch_sheet(true),
                KeyCode::Char('[') if self.sheet_view().is_some() => return self.switch_sheet(false),
                // Everywhere else, [ ] page, as they turn a PDF's pages.
                KeyCode::Char(']') => self.scroll_by(page),
                KeyCode::Char('[') => self.scroll_by(-page),
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
                KeyCode::Enter | KeyCode::Char('e') => return self.edit_current(false),
                KeyCode::Char('h' | 'q') | KeyCode::Left | KeyCode::Esc | KeyCode::Tab => return Action::FocusFiles,
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
                return Action::FocusPreview;
            }
            KeyCode::Char('e') => return self.edit_current(true),
            KeyCode::Char('m') => self.open_menu_at_selection(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => self.up(),
            KeyCode::Char('.') => {
                self.show_hidden = !self.show_hidden;
                self.reload();
                // A folder's preview lists hidden files as the list does, so it's loaded again.
                if self.current().is_some_and(|e| e.is_dir) {
                    self.request_preview(true);
                }
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
            KeyCode::Tab => return Action::FocusPreview,
            _ => {}
        }
        Action::None
    }

    /// A mouse event at absolute screen coordinates, in the file list (`preview` false) or the preview pane.
    /// The server has already focused the pane that was clicked.
    pub fn handle_mouse(&mut self, ev: &MouseEvent, preview: bool) -> Action {
        let action = self.handle_mouse_inner(ev, preview);
        self.pdf_request();
        action
    }

    fn handle_mouse_inner(&mut self, ev: &MouseEvent, preview: bool) -> Action {
        if self.grid.is_some() {
            let body = self.body_rect();
            if let Some(g) = self.grid.as_mut()
                && preview
                && let grid::Outcome::Copy { text, what } = g.handle_mouse(ev, body)
            {
                return Action::CopyCells { text, what };
            }
            return Action::None;
        }
        if let Some(action) = self.popup_mouse(ev) {
            return action;
        }
        let (x, y) = (ev.column, ev.row);
        let in_list = !preview && super::layout::contains(self.list_rect, x, y);
        match ev.kind {
            MouseEventKind::ScrollDown if in_list => self.move_selection(3),
            MouseEventKind::ScrollUp if in_list => self.move_selection(-3),
            MouseEventKind::ScrollDown if preview => self.preview_wheel(3),
            MouseEventKind::ScrollUp if preview => self.preview_wheel(-3),
            MouseEventKind::Down(MouseButton::Left) if preview => {
                if let Some(url) = self.link_at(x, y) {
                    return self.follow(&url);
                }
            }
            MouseEventKind::Down(MouseButton::Right) if in_list => {
                if let Some(index) = self.entry_at(x, y) {
                    self.last_click = None;
                    self.open_menu(index, x);
                }
            }
            MouseEventKind::Down(MouseButton::Left) if in_list => {
                let Some(index) = self.entry_at(x, y) else { return Action::None };
                let double =
                    self.last_click.is_some_and(|(t, i)| i == index && t.elapsed() < Duration::from_millis(400));
                self.select(index);
                self.last_click = Some((Instant::now(), index));
                if double {
                    self.last_click = None;
                    return self.activate();
                }
            }
            _ => {}
        }
        Action::None
    }

    /// The status bar's caption, for the file list or (with `preview`) the preview pane. Only keys that work here, the most useful first, as the status bar drops hints
    /// from the end when there isn't room. No label, and nothing for keys every list and viewer has (arrows to move
    /// and scroll). Plan: docs/plans/2026-11-11-grid-copy-and-open-in-app.md (E, K).
    pub fn hint(&self, preview: bool) -> &str {
        if let Some(h) = self.popup_hint() {
            return h;
        } else if let Some(g) = &self.grid {
            return g.hint();
        } else if self.editor.is_some() {
            return &self.editor_caption;
        } else if self.filtering {
            return "filter: type to narrow · Enter keep · Esc clear";
        } else if !preview {
            return "o app · ←→ focus · / filter · . hidden · m menu";
        }
        let back = !self.history.is_empty();
        if self.pdf_pages_view() {
            let pans = self.pdf.as_ref().is_some_and(|p| p.pans(self.viewport_px().0));
            return if pans {
                "o app · [ ] page · +/- zoom · t text · ←→ pan"
            } else {
                "o app · [ ] page · +/- zoom · t text · ←→ focus"
            };
        }
        if self.pdf.is_some() {
            return "o app · [ ] page · t image · ←→ focus";
        }
        if self.sheet_view().is_some() {
            return "Enter grid · o app · [ ] sheet · ←→ focus";
        }
        if self.editable().is_some() {
            return if back {
                "Enter edit · o app · [ ] page · Backspace back · ←→ focus"
            } else {
                "Enter edit · o app · [ ] page · ←→ focus"
            };
        }
        // Text to scroll, or a picture, a video or nothing much.
        let pages = matches!(self.preview_kind, Some(Kind::Docx | Kind::Pptx | Kind::Dir));
        match (pages, back) {
            (true, false) => "o app · [ ] page · ←→ focus",
            (true, true) => "o app · [ ] page · Backspace back · ←→ focus",
            (false, false) => "o app · ←→ focus",
            (false, true) => "o app · Backspace back · ←→ focus",
        }
    }

    // ----- drawing -----

    /// Draws the file list into its pane at `list`. `accent` colours the frame when the pane has focus.
    pub fn render_list(&self, buf: &mut Buffer, list: Rect, focused: bool, accent: Color) {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let active = focused && self.editor.is_none() && self.grid.is_none();
        // While hidden files are left out, the title says how many, so a folder doesn't look emptier than it is.
        let hidden = if self.hidden > 0 { format!("{} hidden", self.hidden) } else { String::new() };
        if self.frames {
            // "┌ ~/dir ──── 3 hidden ┐", as the preview's frame shows the file's details.
            let title = self.list_title(usize::from(frame_title_room(list.width, &hidden)));
            let style = if active { bold.fg(accent) } else { Style::default() };
            draw_frame(buf, list, &title, &hidden, style);
        } else {
            // " ~/dir · 3 hidden", the count dim. It's dropped if it would leave the folder less than 8 columns.
            let room = usize::from(list.width.saturating_sub(2));
            let note = if hidden.is_empty() { String::new() } else { format!("· {hidden}") };
            let note = if note.chars().count() + 8 > room { String::new() } else { note };
            let title = format!(" {} ", self.list_title(room - note.chars().count()));
            let w = put_str(buf, list.x, list.y, &title, bold.fg(Color::Blue), list.width);
            put_str(buf, list.x + w, list.y, &note, Style::default().fg(Color::DarkGray), list.width - w);
        }
        self.render_entries(buf, self.entries_rect(list), active);
    }

    /// Draws the preview into its pane at `preview`: the file's preview, the editor or the CSV grid. With
    /// `images`, a picture is left blank and handed back for the server to draw; otherwise it's drawn as blocks.
    pub fn render_preview(
        &self,
        buf: &mut Buffer,
        preview: Rect,
        focused: bool,
        accent: Color,
        editor: Option<&TermPane>,
        images: Option<&mut Vec<ImageSlot>>,
    ) -> Option<Cursor> {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let frame_style = |active: bool| if active { bold.fg(accent) } else { Style::default() };
        let header = Rect::new(preview.x, preview.y, preview.width, 1);
        let body = self.body_of(preview);
        let name = self
            .editing
            .as_ref()
            .or(self.preview_path.as_ref())
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let preview_active = focused;
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
            let hint = self.editor_kind.as_ref().map(Editor::keys).unwrap_or_default();
            let hint = hint.as_str();
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
        let pdf_info = self.pdf.as_ref().map(|p| {
            let size =
                self.current().filter(|e| Some(&e.path) == self.preview_path.as_ref()).map(|e| human_size(e.size));
            p.header(self.pdf_pages, self.pdf_zoom, self.text_top(), &size.unwrap_or_default())
        });
        let info = pdf_info.or_else(|| self.view.as_ref().map(|v| v.info.clone())).unwrap_or_default();
        // A frame drops a label that doesn't fit, so cut it to keep its start (the kind, the page or the sheet),
        // leaving room for the name, or 20 columns of it.
        let name_room = name.chars().count().min(20) as u16;
        let max = usize::from(preview.width.saturating_sub(12 + name_room)).max(8);
        let info = if self.frames && info.chars().count() > max {
            format!("{}…", info.chars().take(max - 1).collect::<String>())
        } else {
            info
        };
        let info = info.as_str();
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
        if let Some(p) = &self.pdf {
            self.render_pdf(buf, p, body, images);
            return None;
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
                let area = Rect::new(body.x + 1, body.y, body.width.saturating_sub(2), body.height);
                match images {
                    Some(slots) => slots.push(ImageSlot { id: *id, img: img.clone(), area, keep: false }),
                    None => self.draw_image_blocks(buf, img, *id, area),
                }
            }
        }
        None
    }

    /// Draws a picture as half-blocks, keeping the blocks for the next frame.
    fn draw_image_blocks(&self, buf: &mut Buffer, img: &image::RgbImage, id: u64, area: Rect) {
        let key = (id, (area.width, area.height));
        let mut cache = self.image_cache.borrow_mut();
        if cache.as_ref().map(|c| c.0) != Some(key) {
            *cache = Some((key, view::image_cells(img, area.width.max(1), area.height.max(1))));
        }
        draw_blocks(buf, &cache.as_ref().unwrap().1, area);
    }

    /// The PDF preview's body: its text, or the visible part of its page.
    fn render_pdf(&self, buf: &mut Buffer, p: &PdfView, body: Rect, images: Option<&mut Vec<ImageSlot>>) {
        let dim = Style::default().fg(Color::DarkGray);
        let width = body.width.saturating_sub(1);
        if let Some(e) = &p.error {
            put_str(buf, body.x + 1, body.y, &format!("can't preview: {e}"), Style::default().fg(Color::Red), width);
            return;
        }
        if !self.pdf_pages_view() {
            if p.lines.is_empty() {
                put_str(buf, body.x + 1, body.y, "…", dim, width);
            }
            for (row, line) in p.lines.iter().skip(self.text_top()).take(usize::from(body.height)).enumerate() {
                buf.set_line(body.x + 1, body.y + row as u16, line, width);
            }
            return;
        }
        let Some(page) = &p.image else {
            put_str(buf, body.x + 1, body.y, &format!("rendering page {}…", p.page), dim, width);
            return;
        };
        let (img, id) = match p.crop(self.viewport_px()) {
            None => (page.img.clone(), page.id),
            Some(rect) => {
                let key = (page.id, rect);
                let mut crop = self.crop.borrow_mut();
                if crop.as_ref().map(|c| c.0) != Some(key) {
                    let (x, y, w, h) = rect;
                    let cut = image::imageops::crop_imm(page.img.as_ref(), x, y, w, h).to_image();
                    *crop = Some((key, Arc::new(cut), view::next_image_id()));
                }
                let c = crop.as_ref().unwrap();
                (c.1.clone(), c.2)
            }
        };
        let area = Rect::new(body.x + 1, body.y, body.width.saturating_sub(2), body.height);
        match images {
            Some(slots) => slots.push(ImageSlot { id, img, area, keep: true }),
            None => self.draw_image_blocks(buf, &img, id, area),
        }
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
    fn render_entries(&self, buf: &mut Buffer, area: Rect, active: bool) {
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
            // Inside a box, keep a column free before the right edge.
            let max = if self.frames { area.width.saturating_sub(1) } else { area.width };
            if let Some(edit) = self.renaming(&e.name) {
                // The row is the rename field.
                let field = Style::default().fg(Color::Black).bg(Color::Yellow);
                fill(buf, Rect::new(area.x, y, max, 1), " ", field);
                put_str(
                    buf,
                    area.x + 1,
                    y,
                    &edit.shown(usize::from(max.saturating_sub(2))),
                    field,
                    max.saturating_sub(1),
                );
                continue;
            }
            let name = if e.is_dir { format!(" {}/", e.name) } else { format!(" {}", e.name) };
            put_str(buf, area.x, y, &name, style, max);
        }
    }

    /// The file list as plain text, for `tos dump-screen`.
    pub fn list_text(&self) -> String {
        let area = Rect::new(0, 0, self.list_rect.width, self.list_rect.height);
        let mut buf = Buffer::empty(area);
        self.render_list(&mut buf, area, false, Color::Green);
        buffer_text(&buf)
    }

    /// The preview pane as plain text, for `tos dump-screen`.
    pub fn preview_text(&self, editor: Option<&TermPane>) -> String {
        let area = Rect::new(0, 0, self.preview_rect.width, self.preview_rect.height);
        let mut buf = Buffer::empty(area);
        self.render_preview(&mut buf, area, false, Color::Green, editor, None);
        buffer_text(&buf)
    }
}

/// A buffer's text, one line per row, without trailing spaces or blank lines at the end.
fn buffer_text(buf: &Buffer) -> String {
    let area = buf.area;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A finder on `dir` whose list is 30 columns wide, and the channel it reports to (kept open).
    fn finder(dir: &Path, frames: bool) -> (Finder, mpsc::Receiver<Ev>) {
        let (tx, rx) = mpsc::channel();
        (Finder::new(1, dir.to_path_buf(), Rect::new(0, 0, 30, 8), frames, tx), rx)
    }

    fn key(f: &mut Finder, c: char) {
        f.handle_key(&KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), false);
    }

    fn folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".hidden"), "h").unwrap();
        std::fs::write(dir.path().join("seen"), "s").unwrap();
        dir
    }

    #[test]
    fn hidden_files_are_shown_until_dot() {
        let dir = folder();
        let (mut f, _rx) = finder(dir.path(), true);
        let text = f.list_text();
        assert!(text.contains(".hidden") && text.contains("seen"), "{text}");
        assert!(!text.contains("1 hidden"), "{text}");

        key(&mut f, '.');
        let text = f.list_text();
        assert!(!text.contains(".hidden") && text.contains("seen"), "{text}");
        assert!(text.lines().next().unwrap().ends_with(" 1 hidden ┐"), "{text}");

        key(&mut f, '.');
        let text = f.list_text();
        assert!(text.contains(".hidden") && !text.contains("1 hidden"), "{text}");
    }

    #[test]
    fn the_count_follows_the_folder_without_frames() {
        let dir = folder();
        let (mut f, _rx) = finder(dir.path(), false);
        key(&mut f, '.');
        let text = f.list_text();
        assert!(text.lines().next().unwrap().ends_with(" · 1 hidden"), "{text}");
        // A folder with nothing hidden says nothing.
        std::fs::create_dir(dir.path().join("plain")).unwrap();
        f.cd(dir.path().join("plain"), None);
        assert!(!f.list_text().contains("hidden"), "{}", f.list_text());
    }

    #[test]
    fn long_folder_names_keep_their_end_and_the_count() {
        let dir = folder();
        let long = dir.path().join("a-folder-with-a-rather-long-name");
        std::fs::create_dir(&long).unwrap();
        std::fs::write(long.join(".hidden"), "h").unwrap();
        let (mut f, _rx) = finder(&long, true);
        let title = f.list_text().lines().next().unwrap().to_owned();
        assert!(title.starts_with("┌ …") && title.ends_with("-long-name ─┐"), "{title}");

        key(&mut f, '.');
        let title = f.list_text().lines().next().unwrap().to_owned();
        assert!(title.starts_with("┌ …") && title.ends_with("-name ─ 1 hidden ┐"), "{title}");
    }

    #[test]
    fn the_choice_is_saved_and_old_saves_show_hidden_files() {
        let dir = folder();
        let (mut f, _rx) = finder(dir.path(), true);
        assert!(!f.state().hide_hidden);
        key(&mut f, '.');
        let state = f.state();
        assert!(state.hide_hidden);

        let (mut g, _rx2) = finder(dir.path(), true);
        g.restore_state(&state);
        assert!(!g.list_text().contains(".hidden"), "{}", g.list_text());

        // Every finder saved before 2026-11-07 says `"show_hidden": false`, whatever the user chose.
        let old = format!(
            r#"{{"dir":{:?},"selected":"seen","show_hidden":false,"scroll":0,"editing":null}}"#,
            dir.path().display().to_string()
        );
        let old: FinderState = serde_json::from_str(&old).unwrap();
        assert!(!old.hide_hidden);
        let (mut h, _rx3) = finder(dir.path(), true);
        key(&mut h, '.');
        h.restore_state(&old);
        let text = h.list_text();
        assert!(text.contains(".hidden") && !text.contains("1 hidden"), "{text}");
    }
}
