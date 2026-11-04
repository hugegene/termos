//! The session server: owns the panes and tabs, talks to clients over a Unix socket, and draws each
//! client's screen. One server process runs per session.

mod copy;
mod editor;
mod finder;
mod images;
mod input;
mod keys;
pub mod layout;
mod pane;
mod persist;
mod render;
mod scroll;

use std::collections::HashMap;
use std::fs;
use std::net::Shutdown;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::Event as TermEvent;
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::TermMode;
use anyhow::{Context, bail};
use base64::Engine;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::protocol::{
    self, ClientKind, ClientMsg, Command, Direction, FocusTarget, ImageProtocol, InputEvent, PROTOCOL_VERSION, PaneId,
    ServerMsg, SplitDir,
};
use crate::{logger, paths};
use copy::CopyState;
use finder::{Action, Finder, FinderEvent};
use layout::{Layout, Node};
use pane::{SpawnOptions, TermPane};
use render::{Cursor, TermDecor, draw_frame, draw_separators, draw_term, encode_frame, fill, put_str};

/// Everything the main loop reacts to.
pub enum Ev {
    Conn(UnixStream),
    Client(ClientId, ClientMsg),
    Gone(ClientId),
    Pane(PaneId, TermEvent),
    Finder(PaneId, FinderEvent),
    /// An encoded image is ready (`None`: the job was skipped).
    Image(images::ImageKey, Option<std::sync::Arc<Vec<u8>>>),
}

pub type ClientId = u32;

/// A tab bar click area: (start x, end x, tab index or `None` for the "+" button).
type TabZone = (u16, u16, Option<usize>);

/// Shortest time between two frames sent to a client.
const FRAME: Duration = Duration::from_millis(8);
const MESSAGE_TIME: Duration = Duration::from_secs(3);
/// How often to redraw anyway, so names that follow the running program stay current. Starting a program
/// prints nothing, and a redraw that changes nothing sends nothing.
const REFRESH: Duration = Duration::from_millis(500);
/// How often the session is saved for restoring, if it changed.
const SAVE_EVERY: Duration = Duration::from_secs(2);

struct Client {
    tx: SyncSender<ServerMsg>,
    attached: bool,
    cols: u16,
    rows: u16,
    /// What the client's screen shows now; `None` forces a full redraw.
    prev: Option<Buffer>,
    prev_cursor: Option<Cursor>,
    /// Tab bar click areas: (start x, end x, tab index or `None` for the "+" button).
    tab_zones: Vec<TabZone>,
    /// How this client's terminal shows images.
    graphics: protocol::Graphics,
    /// The images this client's screen shows now.
    shown: Vec<images::Placed>,
}

/// One client's screen: what `compose` builds.
struct Frame {
    buf: Buffer,
    cursor: Option<Cursor>,
    zones: Vec<TabZone>,
    /// Real images to show after the text.
    images: Vec<images::Placed>,
    /// Encodings this frame wanted but the cache didn't have; blocks stand in until they're ready.
    jobs: Vec<images::Job>,
}

enum Pane {
    Term(TermPane),
    Finder(Box<Finder>),
}

struct Tab {
    /// Stays the same when tabs move, unlike the index.
    id: u32,
    name: String,
    /// Name the tab after its focused program until the user renames it.
    auto_name: bool,
    root: Node,
    focus: PaneId,
    zoomed: bool,
    /// Typing goes to every terminal pane in the tab.
    sync: bool,
}

/// zellij's input modes. See `keys.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Modal {
    Normal,
    Locked,
    Pane,
    Tab,
    Resize,
    Move,
    Scroll,
    Search,
    Session,
    Tmux,
}

enum Mode {
    Modal(Modal),
    /// Typing a search term (scroll mode, then `s`).
    EnterSearch(String),
    /// Renaming the active tab. The name changes as you type; Esc puts the old one back.
    RenameTab {
        old: String,
        old_auto: bool,
    },
    /// Renaming a pane, likewise.
    RenamePane {
        pane: PaneId,
        old: Option<String>,
    },
    /// Selecting with vi keys (scroll mode, then `v`).
    Select(CopyState),
    ConfirmQuit,
    Help,
}

const NORMAL: Mode = Mode::Modal(Modal::Normal);

enum Drag {
    Border { path: Vec<bool>, area: Rect, vertical: bool },
    Select { pane: PaneId, rect: Rect, start: Point, started: bool },
    Forward { pane: PaneId, rect: Rect },
}

pub struct Server {
    name: String,
    tx: Sender<Ev>,
    clients: HashMap<ClientId, Client>,
    next_client: ClientId,
    panes: HashMap<PaneId, Pane>,
    next_pane: PaneId,
    tabs: Vec<Tab>,
    active: usize,
    /// Size of the client that typed last; the layout follows it.
    size: (u16, u16),
    active_client: Option<ClientId>,
    mode: Mode,
    drag: Option<Drag>,
    dirty: bool,
    quit: bool,
    had_tabs: bool,
    message: Option<(String, Instant)>,
    last_click: Option<(Instant, PaneId, Point, u8)>,
    started: Instant,
    /// Draw a box around each pane (zellij's pane frames); off draws separator lines between panes.
    frames: bool,
    /// Names given with "rename pane".
    pane_names: HashMap<PaneId, String>,
    next_tab_id: u32,
    /// The tab shown before the current one (by id), for "last tab".
    last_tab: Option<u32>,
    search: Option<scroll::Search>,
    /// Scrollback editors: editor pane → (the pane it stands in for, the temporary file).
    scrollback: HashMap<PaneId, (PaneId, PathBuf)>,
    /// Restored commands waiting to be typed at their shell's prompt.
    pending_input: HashMap<PaneId, persist::PendingInput>,
    /// The last saved session, to skip writing when nothing changed.
    last_saved: Option<String>,
    /// Delete the saved session when the server ends (its last pane exited, or `tb delete`).
    forget: bool,
    images: images::Images,
}

fn peer_is_us(stream: &UnixStream) -> bool {
    rustix::net::sockopt::socket_peercred(stream).is_ok_and(|c| c.uid == rustix::process::getuid())
}

/// Runs the server for session `name` until the session ends.
/// Runs a session's server. `run_commands`: when restoring, run the commands that were running instead of
/// typing them at the prompt (zellij's `attach --force-run-commands`).
pub fn run(name: &str, run_commands: bool) -> anyhow::Result<()> {
    // Panes must not think they run inside another multiplexer. Safe: no other threads exist yet.
    for var in ["ZELLIJ", "ZELLIJ_SESSION_NAME", "ZELLIJ_PANE_ID", "TMUX", "TMUX_PANE", "TB_SESSION", "TB_PANE_ID"] {
        unsafe { std::env::remove_var(var) };
    }
    paths::ensure_private_dir(&paths::state_dir())?;
    logger::init(&paths::log_path(name))?;
    paths::ensure_private_dir(&paths::socket_dir())?;
    let sock = paths::socket_path(name);
    if sock.exists() {
        if UnixStream::connect(&sock).is_ok() {
            bail!("session '{name}' is already running");
        }
        let _ = fs::remove_file(&sock);
    }
    let listener = UnixListener::bind(&sock).with_context(|| format!("binding {}", sock.display()))?;
    fs::set_permissions(&sock, fs::Permissions::from_mode(0o600))?;
    log::info!("session {name} started (pid {})", std::process::id());

    let (tx, rx) = mpsc::channel();
    let accept_tx = tx.clone();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if !peer_is_us(&stream) {
                log::warn!("rejected a connection from another user");
                continue;
            }
            if accept_tx.send(Ev::Conn(stream)).is_err() {
                break;
            }
        }
    });

    let mut server = Server::new(name, tx);
    if let Some(saved) = persist::load(name) {
        server.restore(saved, run_commands);
    }
    server.main_loop(rx);
    let _ = fs::remove_file(&sock);
    log::info!("session {name} ended");
    Ok(())
}

impl Server {
    fn new(name: &str, tx: Sender<Ev>) -> Self {
        let encoder = images::Images::new(tx.clone());
        Server {
            name: name.to_owned(),
            tx,
            clients: HashMap::new(),
            next_client: 1,
            panes: HashMap::new(),
            next_pane: 1,
            tabs: Vec::new(),
            active: 0,
            size: (80, 24),
            active_client: None,
            mode: NORMAL,
            drag: None,
            dirty: false,
            quit: false,
            had_tabs: false,
            message: None,
            last_click: None,
            started: Instant::now(),
            frames: true,
            pane_names: HashMap::new(),
            next_tab_id: 0,
            last_tab: None,
            search: None,
            scrollback: HashMap::new(),
            pending_input: HashMap::new(),
            last_saved: None,
            forget: false,
            images: encoder,
        }
    }

    fn main_loop(&mut self, rx: Receiver<Ev>) {
        let mut last_render = Instant::now() - FRAME;
        let mut last_refresh = Instant::now();
        let mut last_save = Instant::now();
        loop {
            let timeout =
                if self.dirty { FRAME.saturating_sub(last_render.elapsed()) } else { Duration::from_millis(500) };
            match rx.recv_timeout(timeout) {
                Ok(ev) => self.handle(ev),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            // Handle what's queued, but keep drawing even under a flood of output.
            for ev in rx.try_iter().take(2000) {
                self.handle(ev);
            }
            if self.message.as_ref().is_some_and(|(_, t)| t.elapsed() > MESSAGE_TIME) {
                self.message = None;
                self.dirty = true;
            }
            // A server nobody used (for example after a failed start) goes away on its own.
            if !self.had_tabs && self.clients.is_empty() && self.started.elapsed() > Duration::from_secs(30) {
                self.quit = true;
            }
            if self.quit {
                break;
            }
            if last_refresh.elapsed() >= REFRESH {
                last_refresh = Instant::now();
                self.dirty = true;
                self.flush_pending_input();
            }
            if last_save.elapsed() >= SAVE_EVERY {
                last_save = Instant::now();
                self.autosave();
            }
            if self.dirty && last_render.elapsed() >= FRAME {
                self.render_all();
                last_render = Instant::now();
            }
        }
        // Save before the programs go, so their folders can still be read.
        if self.forget {
            persist::delete(&self.name);
        } else {
            self.autosave();
        }
        for client in self.clients.values() {
            let _ = client.tx.try_send(ServerMsg::Exit { reason: "session ended".into() });
        }
        self.clients.clear();
        for pane in self.panes.values() {
            if let Pane::Term(p) = pane {
                p.shutdown();
            }
        }
        // Give writer threads a moment to deliver the exit message.
        thread::sleep(Duration::from_millis(100));
    }

    fn handle(&mut self, ev: Ev) {
        match ev {
            Ev::Conn(stream) => self.add_connection(stream),
            Ev::Client(id, msg) => self.client_msg(id, msg),
            Ev::Gone(id) => self.remove_client(id),
            Ev::Pane(id, e) => self.pane_event(id, e),
            Ev::Finder(id, e) => {
                if let Some(Pane::Finder(f)) = self.panes.get_mut(&id) {
                    f.on_event(e);
                    self.dirty = true;
                }
            }
            Ev::Image(key, bytes) => {
                self.images.done(key, bytes);
                self.dirty = true;
            }
        }
    }

    // ----- clients -----

    fn add_connection(&mut self, stream: UnixStream) {
        let id = self.next_client;
        self.next_client += 1;
        let Ok(mut reader) = stream.try_clone() else { return };
        let (wtx, wrx) = mpsc::sync_channel::<ServerMsg>(256);
        let mut writer = stream;
        thread::spawn(move || {
            for msg in wrx {
                if protocol::write_frame(&mut writer, &msg).is_err() {
                    break;
                }
            }
            let _ = writer.shutdown(Shutdown::Both);
        });
        let tx = self.tx.clone();
        thread::spawn(move || {
            loop {
                match protocol::read_frame::<_, ClientMsg>(&mut reader) {
                    Ok(msg) => {
                        if tx.send(Ev::Client(id, msg)).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(Ev::Gone(id));
                        break;
                    }
                }
            }
        });
        self.clients.insert(
            id,
            Client {
                tx: wtx,
                attached: false,
                cols: 80,
                rows: 24,
                prev: None,
                prev_cursor: None,
                tab_zones: Vec::new(),
                graphics: protocol::Graphics::default(),
                shown: Vec::new(),
            },
        );
    }

    fn send(&self, id: ClientId, msg: ServerMsg) {
        if let Some(c) = self.clients.get(&id) {
            let _ = c.tx.try_send(msg);
        }
    }

    fn remove_client(&mut self, id: ClientId) {
        if self.clients.remove(&id).is_none() {
            return;
        }
        if self.active_client == Some(id) {
            self.active_client = self.clients.iter().find(|(_, c)| c.attached).map(|(id, _)| *id);
            if let Some(c) = self.active_client.and_then(|i| self.clients.get(&i)) {
                let (cols, rows) = (c.cols, c.rows);
                self.set_size(cols, rows);
            }
        }
        self.drag = None;
    }

    fn detach(&mut self, id: ClientId) {
        self.send(id, ServerMsg::Exit { reason: "detached".into() });
        self.remove_client(id);
    }

    fn client_msg(&mut self, id: ClientId, msg: ClientMsg) {
        match msg {
            ClientMsg::Hello { version, kind } => {
                if version != PROTOCOL_VERSION {
                    let text = format!(
                        "this session uses protocol {PROTOCOL_VERSION} but this tb uses {version}; \
                         end the session with the matching tb (tb kill) and start it again"
                    );
                    self.send(id, ServerMsg::Reply { ok: false, text: text.clone() });
                    self.send(id, ServerMsg::Exit { reason: text });
                    self.remove_client(id);
                    return;
                }
                match kind {
                    ClientKind::Attach { cols, rows, graphics } => {
                        if let Some(c) = self.clients.get_mut(&id) {
                            c.attached = true;
                            c.cols = cols;
                            c.rows = rows;
                            c.graphics = graphics;
                        }
                        log::info!("client {id} shows images with {:?}", graphics);
                        self.active_client = Some(id);
                        self.set_size(cols, rows);
                        if self.tabs.is_empty()
                            && let Err(e) = self.new_tab(None, SpawnOptions::default())
                        {
                            self.send(id, ServerMsg::Exit { reason: format!("can't start a shell: {e}") });
                        }
                        log::info!("client {id} attached ({cols}x{rows})");
                    }
                    ClientKind::Command(cmd) => {
                        let (ok, text) = match self.command(cmd, id) {
                            Ok(text) => (true, text),
                            Err(e) => (false, format!("{e:#}")),
                        };
                        self.send(id, ServerMsg::Reply { ok, text });
                        self.remove_client(id);
                    }
                }
                self.dirty = true;
            }
            ClientMsg::Input(ev) => {
                if self.clients.get(&id).is_some_and(|c| c.attached) {
                    self.make_active(id);
                    self.input(id, ev);
                }
            }
            ClientMsg::Resize { cols, rows } => {
                if let Some(c) = self.clients.get_mut(&id) {
                    c.cols = cols;
                    c.rows = rows;
                    c.prev = None;
                }
                if self.active_client == Some(id) {
                    self.set_size(cols, rows);
                }
                self.dirty = true;
            }
            ClientMsg::Detach => self.detach(id),
        }
    }

    fn make_active(&mut self, id: ClientId) {
        if self.active_client != Some(id) {
            self.active_client = Some(id);
            if let Some(c) = self.clients.get(&id) {
                let (cols, rows) = (c.cols, c.rows);
                self.set_size(cols, rows);
            }
        }
    }

    fn flash(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), Instant::now()));
        self.dirty = true;
    }

    /// Sends text to the clipboard of `client` (or the active client) with OSC 52.
    fn copy_to_clipboard(&mut self, client: Option<ClientId>, text: &str) {
        if text.is_empty() {
            return;
        }
        let encoded = base64::engine::general_purpose::STANDARD.encode(text);
        let seq = format!("\x1b]52;c;{encoded}\x07").into_bytes();
        if let Some(id) = client.or(self.active_client) {
            self.send(id, ServerMsg::Output(seq));
        }
        self.flash(format!("copied {} characters", text.chars().count()));
    }

    // ----- panes and tabs -----

    fn content_area(&self) -> Rect {
        Rect::new(0, 1, self.size.0, self.size.1.saturating_sub(2))
    }

    fn set_size(&mut self, cols: u16, rows: u16) {
        if self.size != (cols, rows) {
            self.size = (cols, rows);
            self.apply_layout();
        }
        self.dirty = true;
    }

    fn alloc_pane(&mut self) -> PaneId {
        let id = self.next_pane;
        self.next_pane += 1;
        id
    }

    /// The tab layout as shown: a zoomed tab shows only its focused pane.
    fn tab_layout(&self, tab: &Tab) -> Layout {
        let area = self.content_area();
        if tab.zoomed {
            Layout { panes: vec![(tab.focus, area)], separators: Vec::new() }
        } else {
            tab.root.layout(area, self.frames)
        }
    }

    /// Where a terminal pane's program draws inside the pane's `rect`: inside its frame, if any.
    fn body(&self, rect: Rect) -> Rect {
        if self.frames { layout::inner(rect) } else { rect }
    }

    /// Gives every pane in every tab the size its layout says.
    fn apply_layout(&mut self) {
        let area = self.content_area();
        let jobs: Vec<(PaneId, Rect)> = self
            .tabs
            .iter()
            .flat_map(|t| {
                let mut panes = t.root.layout(area, self.frames).panes;
                if t.zoomed {
                    for (id, r) in &mut panes {
                        if *id == t.focus {
                            *r = area;
                        }
                    }
                }
                panes
            })
            .collect();
        for (id, rect) in jobs {
            self.resize_pane(id, rect);
        }
        self.dirty = true;
    }

    fn resize_pane(&mut self, id: PaneId, rect: Rect) {
        let (frames, body) = (self.frames, self.body(rect));
        let editor = match self.panes.get_mut(&id) {
            Some(Pane::Term(p)) => {
                p.resize(body.width, body.height);
                None
            }
            Some(Pane::Finder(f)) => {
                f.frames = frames;
                f.resize(rect);
                f.editor.map(|e| (e, f.body_rect(rect)))
            }
            None => None,
        };
        if let Some((eid, body)) = editor
            && let Some(Pane::Term(p)) = self.panes.get_mut(&eid)
        {
            p.resize(body.width, body.height);
        }
    }

    fn focused_pane(&self) -> Option<PaneId> {
        self.tabs.get(self.active).map(|t| t.focus)
    }

    /// The terminal that receives typing: the focused terminal pane, or the editor in a focused finder.
    fn typing_target(&self) -> Option<PaneId> {
        let id = self.focused_pane()?;
        match self.panes.get(&id)? {
            Pane::Term(_) => Some(id),
            Pane::Finder(f) => f.editor,
        }
    }

    fn term(&self, id: PaneId) -> Option<&TermPane> {
        match self.panes.get(&id) {
            Some(Pane::Term(p)) => Some(p),
            _ => None,
        }
    }

    fn focused_cwd(&self) -> Option<PathBuf> {
        let id = self.focused_pane()?;
        match self.panes.get(&id)? {
            Pane::Term(p) => p.cwd(),
            Pane::Finder(f) => Some(f.dir.clone()),
        }
    }

    fn spawn_term(&mut self, opts: SpawnOptions) -> std::io::Result<PaneId> {
        let id = self.alloc_pane();
        let area = self.content_area();
        let pane = TermPane::spawn(id, area.width, area.height, opts, &self.name, self.tx.clone())?;
        self.panes.insert(id, Pane::Term(pane));
        Ok(id)
    }

    fn make_tab(&mut self, name: Option<String>, id: PaneId) -> Tab {
        self.next_tab_id += 1;
        Tab {
            id: self.next_tab_id,
            auto_name: name.is_none(),
            name: name.unwrap_or_else(|| "shell".into()),
            root: Node::Leaf(id),
            focus: id,
            zoomed: false,
            sync: false,
        }
    }

    /// A finder keeps its name when it moves to a tab of its own.
    fn tab_name_for(&self, id: PaneId) -> Option<String> {
        matches!(self.panes.get(&id), Some(Pane::Finder(_))).then(|| "finder".to_owned())
    }

    fn remember_tab(&mut self) {
        if let Some(t) = self.tabs.get(self.active) {
            self.last_tab = Some(t.id);
        }
    }

    fn push_tab(&mut self, name: Option<String>, id: PaneId) {
        let tab = self.make_tab(name, id);
        self.tabs.push(tab);
        let old = self.focused_pane();
        self.remember_tab();
        self.active = self.tabs.len() - 1;
        self.had_tabs = true;
        self.focus_changed(old, Some(id));
        self.apply_layout();
    }

    fn new_tab(&mut self, name: Option<String>, opts: SpawnOptions) -> std::io::Result<PaneId> {
        let id = self.spawn_term(opts)?;
        self.push_tab(name, id);
        Ok(id)
    }

    fn new_finder_tab(&mut self, dir: PathBuf) -> PaneId {
        let id = self.alloc_pane();
        let finder = Finder::new(id, dir, self.content_area(), self.frames, self.tx.clone());
        self.panes.insert(id, Pane::Finder(Box::new(finder)));
        self.push_tab(Some("finder".into()), id);
        id
    }

    fn split(&mut self, dir: SplitDir, opts: SpawnOptions) -> std::io::Result<PaneId> {
        let Some(target) = self.focused_pane() else { return self.new_tab(None, opts) };
        self.split_at(target, dir, opts)
    }

    /// Splits pane `target` in the active tab; the new pane gets focus.
    fn split_at(&mut self, target: PaneId, dir: SplitDir, opts: SpawnOptions) -> std::io::Result<PaneId> {
        let rect = self.tabs[self.active].root.layout(self.content_area(), self.frames).rect_of(target);
        if !rect.is_some_and(|r| layout::can_split(r, dir, self.frames)) {
            return Err(std::io::Error::other("not enough room to split this pane"));
        }
        let id = self.spawn_term(opts)?;
        let tab = &mut self.tabs[self.active];
        tab.root.split(target, id, dir);
        let old = std::mem::replace(&mut tab.focus, id);
        tab.zoomed = false;
        self.focus_changed(Some(old), Some(id));
        self.apply_layout();
        Ok(id)
    }

    fn cwd_opts(&self) -> SpawnOptions {
        SpawnOptions { cwd: self.focused_cwd(), ..SpawnOptions::default() }
    }

    /// Splits the focused pane (keys), showing any error on the status bar.
    fn split_here(&mut self, dir: SplitDir) {
        if let Err(e) = self.split(dir, self.cwd_opts()) {
            self.flash(format!("{e}"));
        }
    }

    /// Where a new pane fits in tab `ti`: its biggest pane, split along its longer side.
    fn room_in_tab(&self, ti: usize) -> Option<(PaneId, SplitDir)> {
        let tab = self.tabs.get(ti)?;
        let mut best: Option<(u32, PaneId, Rect)> = None;
        for (id, r) in tab.root.layout(self.content_area(), self.frames).panes {
            let area = u32::from(r.width) * u32::from(r.height);
            let better = match best {
                None => true,
                // Ties go to the focused pane.
                Some((a, _, _)) => area > a || (area == a && id == tab.focus),
            };
            if better {
                best = Some((area, id, r));
            }
        }
        let (_, id, r) = best?;
        // Cells are about twice as tall as they are wide.
        let wide = u32::from(r.width) > 2 * u32::from(r.height);
        let order = if wide { [SplitDir::Right, SplitDir::Down] } else { [SplitDir::Down, SplitDir::Right] };
        order.into_iter().find(|d| layout::can_split(r, *d, self.frames)).map(|d| (id, d))
    }

    /// zellij's "new pane": splits wherever there's the most room.
    fn new_pane(&mut self) {
        if self.tabs.is_empty() {
            return;
        }
        if self.tabs[self.active].zoomed {
            self.tabs[self.active].zoomed = false;
            self.apply_layout();
        }
        let Some((target, dir)) = self.room_in_tab(self.active) else {
            self.flash("not enough room for a new pane");
            return;
        };
        if let Err(e) = self.split_at(target, dir, self.cwd_opts()) {
            self.flash(format!("failed: {e}"));
        }
    }

    fn new_tab_here(&mut self) {
        if let Err(e) = self.new_tab(None, self.cwd_opts()) {
            self.flash(format!("failed: {e}"));
        }
    }

    fn open_finder_tab(&mut self) {
        let dir = self.focused_cwd().or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| "/".into());
        self.new_finder_tab(dir);
    }

    fn close_focused(&mut self) {
        if let Some(id) = self.focused_pane() {
            self.close_pane(id);
        }
    }

    fn editor_owner(&self, id: PaneId) -> Option<PaneId> {
        self.panes.iter().find_map(|(fid, p)| match p {
            Pane::Finder(f) if f.editor == Some(id) => Some(*fid),
            _ => None,
        })
    }

    fn drop_pane(&mut self, id: PaneId) {
        self.pending_input.remove(&id);
        match self.panes.remove(&id) {
            Some(Pane::Term(p)) => p.shutdown(),
            Some(Pane::Finder(f)) => {
                if let Some(Pane::Term(e)) = f.editor.and_then(|e| self.panes.remove(&e)) {
                    e.shutdown();
                }
            }
            None => {}
        }
        self.pane_names.remove(&id);
        let selecting = matches!(&self.mode, Mode::Select(c) if c.pane == id);
        let searching = self.search.as_ref().is_some_and(|s| s.pane == id);
        if selecting || searching {
            self.search = None;
            self.mode = NORMAL;
        }
    }

    fn close_pane(&mut self, id: PaneId) {
        if !self.panes.contains_key(&id) {
            return;
        }
        self.dirty = true;
        if let Some(owner) = self.editor_owner(id) {
            self.drop_pane(id);
            if let Some(Pane::Finder(f)) = self.panes.get_mut(&owner) {
                f.editor_closed();
            }
            self.apply_layout();
            return;
        }
        // A scrollback editor gives its place back to the pane it stood in for.
        if let Some((orig, path)) = self.scrollback.remove(&id) {
            let _ = fs::remove_file(&path);
            if self.panes.contains_key(&orig)
                && let Some(tab) = self.tabs.iter_mut().find(|t| t.root.contains(id))
            {
                tab.root.replace(id, orig);
                if tab.focus == id {
                    tab.focus = orig;
                }
                self.drop_pane(id);
                let focus = self.focused_pane();
                self.focus_changed(None, focus);
                self.apply_layout();
                return;
            }
        }
        let tab_index = self.tabs.iter().position(|t| t.root.contains(id));
        self.drop_pane(id);
        let Some(ti) = tab_index else { return };
        if self.tabs[ti].root == Node::Leaf(id) {
            self.tabs.remove(ti);
            if self.tabs.is_empty() {
                // Nothing is left to restore.
                self.quit = true;
                self.forget = true;
                return;
            }
            if ti < self.active || self.active >= self.tabs.len() {
                self.active = self.active.saturating_sub(1);
            }
            let focus = self.focused_pane();
            self.focus_changed(None, focus);
        } else {
            let tab = &mut self.tabs[ti];
            tab.root.remove(id);
            tab.zoomed = false;
            if tab.focus == id {
                tab.focus = tab.root.panes()[0];
                let focus = tab.focus;
                self.focus_changed(None, Some(focus));
            }
        }
        self.apply_layout();
    }

    /// Tells programs that asked (focus reporting) that they lost or gained focus.
    fn focus_changed(&mut self, old: Option<PaneId>, new: Option<PaneId>) {
        if old == new {
            return;
        }
        for (id, gained) in [(old, false), (new, true)] {
            let target = id.and_then(|id| match self.panes.get(&id)? {
                Pane::Term(_) => Some(id),
                Pane::Finder(f) => f.editor,
            });
            if let Some(p) = target.and_then(|t| self.term(t)) {
                let mode = *p.term.lock().mode();
                if let Some(bytes) = input::encode_focus(gained, mode) {
                    p.write(bytes);
                }
            }
        }
    }

    fn focus_pane(&mut self, id: PaneId) {
        let Some(ti) = self.tabs.iter().position(|t| t.root.contains(id)) else { return };
        let old = self.focused_pane();
        self.active = ti;
        self.tabs[ti].focus = id;
        self.focus_changed(old, Some(id));
        self.dirty = true;
    }

    fn focus(&mut self, target: FocusTarget) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let current = tab.focus;
        let next = match target {
            FocusTarget::Pane(id) => Some(id),
            FocusTarget::Dir(d) => {
                layout::neighbor(&tab.root.layout(self.content_area(), self.frames).panes, current, d)
            }
            FocusTarget::Next | FocusTarget::Prev => {
                let panes = tab.root.panes();
                let i = panes.iter().position(|p| *p == current).unwrap_or(0);
                let n = panes.len();
                let j = if target == FocusTarget::Next { (i + 1) % n } else { (i + n - 1) % n };
                Some(panes[j])
            }
        };
        if let Some(id) = next {
            if self.tabs[self.active].zoomed && id != current {
                self.tabs[self.active].zoomed = false;
                self.apply_layout();
            }
            self.focus_pane(id);
        }
    }

    /// Moves focus left or right, or to the neighbouring tab at the edge (zellij's MoveFocusOrTab).
    fn focus_or_tab(&mut self, dir: Direction) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let panes = tab.root.layout(self.content_area(), self.frames).panes;
        if layout::neighbor(&panes, tab.focus, dir).is_some() {
            self.focus(FocusTarget::Dir(dir));
        } else if dir == Direction::Left && self.active > 0 {
            self.go_to_tab(self.active - 1);
        } else if dir == Direction::Right {
            self.go_to_tab(self.active + 1);
        }
    }

    /// Next or previous tab, wrapping around.
    fn cycle_tab(&mut self, next: bool) {
        let n = self.tabs.len().max(1);
        self.go_to_tab(if next { (self.active + 1) % n } else { (self.active + n - 1) % n });
    }

    fn toggle_last_tab(&mut self) {
        if let Some(i) = self.last_tab.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
            self.go_to_tab(i);
        }
    }

    /// Moves the active tab one place left or right, wrapping around the ends.
    fn move_tab(&mut self, right: bool) {
        let n = self.tabs.len();
        if n < 2 {
            return;
        }
        let i = self.active;
        if right && i + 1 < n {
            self.tabs.swap(i, i + 1);
            self.active = i + 1;
        } else if right {
            let t = self.tabs.remove(i);
            self.tabs.insert(0, t);
            self.active = 0;
        } else if i > 0 {
            self.tabs.swap(i, i - 1);
            self.active = i - 1;
        } else {
            let t = self.tabs.remove(0);
            self.tabs.push(t);
            self.active = n - 1;
        }
        self.dirty = true;
    }

    fn go_to_tab(&mut self, index: usize) {
        if index < self.tabs.len() && index != self.active {
            let old = self.focused_pane();
            self.remember_tab();
            self.active = index;
            let new = self.focused_pane();
            self.focus_changed(old, new);
            self.dirty = true;
        }
    }

    fn toggle_zoom(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.zoomed = !tab.zoomed && tab.root.panes().len() > 1;
            self.apply_layout();
        }
    }

    fn move_border(&mut self, dir: Direction, cells: u16) {
        let (area, frames) = (self.content_area(), self.frames);
        if let Some(tab) = self.tabs.get_mut(self.active)
            && !tab.zoomed
            && tab.root.move_border(tab.focus, dir, cells, area, frames)
        {
            self.apply_layout();
        }
    }

    /// zellij's resize: grows or shrinks the focused pane on one side, or on every side with `None`. A
    /// step is 5% of the screen.
    fn resize_focused(&mut self, side: Option<Direction>, grow: bool) {
        let (area, frames) = (self.content_area(), self.frames);
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        if tab.zoomed {
            return;
        }
        let sides = match side {
            Some(d) => vec![d],
            None => vec![Direction::Left, Direction::Right, Direction::Up, Direction::Down],
        };
        let mut changed = false;
        for d in sides {
            let cells = match d {
                Direction::Left | Direction::Right => (area.width / 20).max(1),
                Direction::Up | Direction::Down => (area.height / 20).max(1),
            };
            changed |= tab.root.resize_side(tab.focus, d, grow, cells, area, frames);
        }
        if changed {
            self.apply_layout();
        }
    }

    /// Swaps the focused pane with its neighbour in a direction, or the next or previous pane.
    fn move_pane(&mut self, target: FocusTarget) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let current = tab.focus;
        let other = match target {
            FocusTarget::Dir(d) => {
                layout::neighbor(&tab.root.layout(self.content_area(), self.frames).panes, current, d)
            }
            FocusTarget::Next | FocusTarget::Prev => {
                let panes = tab.root.panes();
                let i = panes.iter().position(|p| *p == current).unwrap_or(0);
                let n = panes.len();
                let j = if target == FocusTarget::Next { (i + 1) % n } else { (i + n - 1) % n };
                Some(panes[j])
            }
            FocusTarget::Pane(id) => Some(id),
        };
        if let Some(other) = other.filter(|o| *o != current) {
            let tab = &mut self.tabs[self.active];
            tab.root.swap(current, other);
            tab.zoomed = false;
            self.apply_layout();
        }
    }

    /// Takes the focused pane out of its tab. Returns it, or `None` if it's the tab's only pane.
    fn take_focused(&mut self) -> Option<PaneId> {
        let tab = self.tabs.get_mut(self.active)?;
        if tab.root.panes().len() < 2 {
            return None;
        }
        let id = tab.focus;
        tab.root.remove(id);
        tab.zoomed = false;
        tab.focus = tab.root.panes()[0];
        Some(id)
    }

    /// Moves the focused pane to a new tab of its own.
    fn break_pane(&mut self) {
        if let Some(id) = self.take_focused() {
            let name = self.tab_name_for(id);
            self.push_tab(name, id);
        }
    }

    /// Moves the focused pane to the tab on the right or left, or to a new tab there if there's none.
    fn break_pane_to(&mut self, right: bool) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let id = tab.focus;
        let alone = tab.root.panes().len() == 1;
        let target = if right { self.active + 1 } else { self.active.wrapping_sub(1) };
        let exists = target < self.tabs.len();
        if alone && !exists {
            return;
        }
        let old = self.focused_pane();
        self.remember_tab();
        if alone {
            self.tabs.remove(self.active);
        } else {
            self.take_focused();
        }
        let target = if alone && right { target - 1 } else { target };
        let room = if exists { self.room_in_tab(target) } else { None };
        if let Some((big, dir)) = room {
            let t = &mut self.tabs[target];
            t.root.split(big, id, dir);
            t.focus = id;
            t.zoomed = false;
            self.active = target;
        } else {
            // No tab on that side, or no room in it: a new tab on that side.
            let name = self.tab_name_for(id);
            let tab = self.make_tab(name, id);
            let at = match (right, exists) {
                (true, true) => target,
                (true, false) => self.tabs.len(),
                (false, true) => target + 1,
                (false, false) => 0,
            };
            self.tabs.insert(at, tab);
            self.active = at;
        }
        self.focus_changed(old, Some(id));
        self.apply_layout();
    }

    /// Closes the active tab and everything in it.
    fn close_tab(&mut self) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let ids = tab.root.panes();
        // Panes hidden behind a scrollback editor go too.
        for id in &ids {
            if let Some((orig, path)) = self.scrollback.remove(id) {
                let _ = fs::remove_file(path);
                self.drop_pane(orig);
            }
        }
        for id in ids {
            self.close_pane(id);
        }
    }

    fn toggle_sync(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.sync = !tab.sync;
            let state = if tab.sync { "typing goes to every pane in this tab" } else { "sync off" };
            self.flash(state);
        }
    }

    fn toggle_frames(&mut self) {
        self.frames = !self.frames;
        self.apply_layout();
    }

    /// Does what a finder asked for after a key or click.
    fn finder_action(&mut self, finder_id: PaneId, action: Action) {
        match action {
            Action::None => {}
            Action::OpenEditor(path) => self.open_editor(finder_id, path),
            Action::Flash(text) => self.flash(text),
        }
    }

    fn open_editor(&mut self, finder_id: PaneId, path: PathBuf) {
        let opts = editor::command(&path);
        let id = self.alloc_pane();
        match TermPane::spawn(id, 80, 24, opts, &self.name, self.tx.clone()) {
            Ok(p) => {
                self.panes.insert(id, Pane::Term(p));
                if let Some(Pane::Finder(f)) = self.panes.get_mut(&finder_id) {
                    f.editor = Some(id);
                    f.editing = Some(path);
                }
                self.apply_layout();
            }
            Err(e) => self.flash(format!("can't start the editor: {e}")),
        }
    }

    fn pane_event(&mut self, id: PaneId, event: TermEvent) {
        match event {
            TermEvent::Wakeup | TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange => {
                self.dirty = true;
                if let Some(p) = self.pending_input.get_mut(&id) {
                    p.output_seen = true;
                }
            }
            TermEvent::PtyWrite(text) => {
                if let Some(p) = self.term(id) {
                    p.write(text.into_bytes());
                }
            }
            TermEvent::Title(title) => {
                if let Some(Pane::Term(p)) = self.panes.get_mut(&id) {
                    p.title = Some(title);
                    self.dirty = true;
                }
            }
            TermEvent::ResetTitle => {
                if let Some(Pane::Term(p)) = self.panes.get_mut(&id) {
                    p.title = None;
                    self.dirty = true;
                }
            }
            TermEvent::ClipboardStore(_, text) => self.copy_to_clipboard(None, &text),
            // Programs may not read the clipboard; colour queries go unanswered (programs fall back).
            TermEvent::ClipboardLoad(..) | TermEvent::ColorRequest(..) => {}
            TermEvent::TextAreaSizeRequest(format) => {
                if let Some(p) = self.term(id) {
                    let size = alacritty_terminal::event::WindowSize {
                        num_cols: p.cols,
                        num_lines: p.rows,
                        cell_width: 8,
                        cell_height: 16,
                    };
                    p.write(format(size).into_bytes());
                }
            }
            TermEvent::Bell => {
                if let Some(c) = self.active_client {
                    self.send(c, ServerMsg::Output(b"\x07".to_vec()));
                }
            }
            TermEvent::ChildExit(_) | TermEvent::Exit => self.close_pane(id),
        }
    }

    // ----- input -----

    fn input(&mut self, client: ClientId, ev: InputEvent) {
        self.dirty = true;
        match ev {
            InputEvent::Key(key) => self.key(client, key),
            InputEvent::Mouse(m) => self.mouse(client, m),
            InputEvent::Paste(text) => {
                for p in self.typing_targets().iter().filter_map(|id| self.term(*id)) {
                    let mode = *p.term.lock().mode();
                    p.write(input::encode_paste(&text, mode));
                }
            }
            InputEvent::FocusGained | InputEvent::FocusLost => {
                let gained = matches!(ev, InputEvent::FocusGained);
                if let Some(p) = self.typing_target().and_then(|id| self.term(id)) {
                    let mode = *p.term.lock().mode();
                    if let Some(bytes) = input::encode_focus(gained, mode) {
                        p.write(bytes);
                    }
                }
            }
        }
    }

    /// Where typing goes: the typing target, or every terminal pane in a synced tab.
    fn typing_targets(&self) -> Vec<PaneId> {
        let synced = self.tabs.get(self.active).filter(|t| t.sync && self.term(t.focus).is_some());
        match synced {
            Some(tab) => tab.root.panes().into_iter().filter(|id| self.term(*id).is_some()).collect(),
            None => self.typing_target().into_iter().collect(),
        }
    }

    /// Sends a key to whatever has focus: a terminal, a finder, or a finder's editor.
    fn send_key(&mut self, key: &KeyEvent) {
        let Some(id) = self.focused_pane() else { return };
        if self.tabs.get(self.active).is_some_and(|t| t.sync) && self.term(id).is_some() {
            for target in self.typing_targets() {
                self.type_into(target, key);
            }
            return;
        }
        let action = match self.panes.get_mut(&id) {
            Some(Pane::Finder(f)) => match f.editor {
                Some(editor) => {
                    self.type_into(editor, key);
                    return;
                }
                None => f.handle_key(key),
            },
            Some(Pane::Term(_)) => {
                self.type_into(id, key);
                return;
            }
            None => return,
        };
        self.finder_action(id, action);
    }

    fn type_into(&self, id: PaneId, key: &KeyEvent) {
        let Some(p) = self.term(id) else { return };
        let mode = {
            let mut term = p.term.lock();
            if term.grid().display_offset() > 0 {
                term.scroll_display(Scroll::Bottom);
            }
            term.selection = None;
            *term.mode()
        };
        if let Some(bytes) = input::encode_key(key, mode) {
            p.write(bytes);
        }
    }

    fn mouse(&mut self, client: ClientId, ev: MouseEvent) {
        let (x, y) = (ev.column, ev.row);
        if self.mouse_drag(client, &ev) {
            return;
        }
        if y == 0 {
            if let MouseEventKind::Down(MouseButton::Left) = ev.kind {
                let zone =
                    self.clients.get(&client).and_then(|c| c.tab_zones.iter().find(|z| x >= z.0 && x < z.1).copied());
                match zone {
                    Some((_, _, Some(i))) => self.go_to_tab(i),
                    Some((_, _, None)) => self.new_tab_here(),
                    None => {}
                }
            }
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else { return };
        let layout = self.tab_layout(tab);
        let focus = tab.focus;
        if let MouseEventKind::Down(MouseButton::Left) = ev.kind
            && let Some(sep) = layout.separator_at(x, y)
        {
            self.drag = Some(Drag::Border { path: sep.path.clone(), area: sep.split_area, vertical: sep.vertical });
            return;
        }
        let Some((id, rect)) = layout.pane_at(x, y) else { return };
        if matches!(ev.kind, MouseEventKind::Down(_)) && id != focus {
            self.focus_pane(id);
        }
        match self.panes.get_mut(&id) {
            Some(Pane::Finder(f)) => {
                if let Some(editor) = f.editor {
                    let body = f.body_rect(rect);
                    if layout::contains(body, x, y) {
                        self.term_mouse(client, editor, body, &ev);
                    }
                    return;
                }
                let action = f.handle_mouse(&ev);
                self.finder_action(id, action);
            }
            Some(Pane::Term(_)) => {
                // A click on the frame only focuses.
                let body = self.body(rect);
                if layout::contains(body, x, y) {
                    self.term_mouse(client, id, body, &ev);
                }
            }
            None => {}
        }
    }

    /// Continues a drag that started earlier. Returns true if the event was used.
    fn mouse_drag(&mut self, client: ClientId, ev: &MouseEvent) -> bool {
        let Some(drag) = self.drag.take() else { return false };
        let done = matches!(ev.kind, MouseEventKind::Up(_));
        let moving = matches!(ev.kind, MouseEventKind::Drag(_));
        if !done && !moving {
            self.drag = Some(drag);
            return false;
        }
        match drag {
            Drag::Border { path, area, vertical } => {
                if moving {
                    let pos = if vertical { ev.column } else { ev.row };
                    let ratio = layout::drag_ratio(pos, area, vertical, self.frames);
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.root.set_ratio(&path, ratio);
                    }
                    self.apply_layout();
                    self.drag = Some(Drag::Border { path, area, vertical });
                }
            }
            Drag::Select { pane, rect, start, mut started } => {
                let mut copied = None;
                if let Some(p) = self.term(pane) {
                    let mut term = p.term.lock();
                    let point = viewport_point(&term, rect, ev.column, ev.row);
                    if moving {
                        if !started {
                            term.selection = Some(Selection::new(SelectionType::Simple, start, Side::Left));
                            started = true;
                        }
                        let side = if point >= start { Side::Right } else { Side::Left };
                        if let Some(sel) = term.selection.as_mut() {
                            sel.update(point, side);
                        }
                    } else if started {
                        copied = term.selection_to_string();
                    }
                }
                if moving {
                    self.drag = Some(Drag::Select { pane, rect, start, started });
                } else if let Some(text) = copied {
                    self.copy_to_clipboard(Some(client), &text);
                }
            }
            Drag::Forward { pane, rect } => {
                if let Some(p) = self.term(pane) {
                    let mode = *p.term.lock().mode();
                    let col = ev.column.clamp(rect.x, rect.x + rect.width.saturating_sub(1)) - rect.x;
                    let row = ev.row.clamp(rect.y, rect.y + rect.height.saturating_sub(1)) - rect.y;
                    if let Some(bytes) = input::encode_mouse(ev, col, row, mode) {
                        p.write(bytes);
                    }
                }
                if moving {
                    self.drag = Some(Drag::Forward { pane, rect });
                }
            }
        }
        self.dirty = true;
        true
    }

    fn term_mouse(&mut self, client: ClientId, id: PaneId, rect: Rect, ev: &MouseEvent) {
        let Some(term_ref) = self.term(id).map(|p| p.term.clone()) else { return };
        let (col, row) = (ev.column - rect.x, ev.row - rect.y);
        let mut out = Vec::new();
        let mut copied = None;
        {
            let mut term = term_ref.lock();
            let mode = *term.mode();
            // Shift bypasses the program's mouse handling, so you can always select text.
            let app_mouse = input::wants_mouse(mode) && !ev.modifiers.contains(KeyModifiers::SHIFT);
            match ev.kind {
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    let up = ev.kind == MouseEventKind::ScrollUp;
                    if app_mouse {
                        out.extend(input::encode_mouse(ev, col, row, mode).unwrap_or_default());
                    } else if mode.contains(TermMode::ALT_SCREEN) {
                        if mode.contains(TermMode::ALTERNATE_SCROLL) {
                            let arrow = KeyEvent::new(if up { KeyCode::Up } else { KeyCode::Down }, KeyModifiers::NONE);
                            out.extend(input::encode_key(&arrow, mode).unwrap_or_default().repeat(3));
                        }
                    } else {
                        term.scroll_display(Scroll::Delta(if up { 3 } else { -3 }));
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if !app_mouse => {
                    let point = viewport_point(&term, rect, ev.column, ev.row);
                    let count = match self.last_click {
                        Some((t, pid, pt, n))
                            if pid == id && pt == point && t.elapsed() < Duration::from_millis(400) =>
                        {
                            n % 3 + 1
                        }
                        _ => 1,
                    };
                    self.last_click = Some((Instant::now(), id, point, count));
                    term.selection = None;
                    match count {
                        1 => self.drag = Some(Drag::Select { pane: id, rect, start: point, started: false }),
                        n => {
                            let ty = if n == 2 { SelectionType::Semantic } else { SelectionType::Lines };
                            term.selection = Some(Selection::new(ty, point, Side::Left));
                            copied = term.selection_to_string();
                        }
                    }
                }
                _ if app_mouse => {
                    out.extend(input::encode_mouse(ev, col, row, mode).unwrap_or_default());
                    if matches!(ev.kind, MouseEventKind::Down(_)) {
                        self.drag = Some(Drag::Forward { pane: id, rect });
                    }
                }
                _ => {}
            }
        }
        if let Some(p) = self.term(id) {
            p.write(out);
        }
        if let Some(text) = copied {
            self.copy_to_clipboard(Some(client), &text);
        }
    }

    // ----- CLI commands -----

    fn command(&mut self, cmd: Command, from: ClientId) -> anyhow::Result<String> {
        let opts =
            |cwd: Option<PathBuf>, command: Vec<String>| SpawnOptions { cwd, command, ..SpawnOptions::default() };
        Ok(match cmd {
            Command::Info => {
                let attached = self.clients.values().filter(|c| c.attached).count();
                format!("{} tabs, {} attached", self.tabs.len(), attached)
            }
            Command::ListPanes => serde_json::to_string(&self.pane_list())?,
            Command::DumpScreen { pane, full } => {
                let id = pane.or_else(|| self.focused_pane()).context("no pane")?;
                match self.panes.get(&id).context("no such pane")? {
                    Pane::Term(p) => p.text(full),
                    Pane::Finder(f) => f.text(f.editor.and_then(|e| self.term(e))),
                }
            }
            Command::Kill { forget } => {
                self.quit = true;
                self.forget = forget;
                String::new()
            }
            Command::EnsureTab => {
                if self.tabs.is_empty() {
                    self.new_tab(None, SpawnOptions::default())?;
                }
                String::new()
            }
            Command::NewTab { name, cwd, command } => {
                let cwd = cwd.or_else(|| self.focused_cwd());
                self.new_tab(name, opts(cwd, command))?.to_string()
            }
            Command::Split { dir, cwd, command } => {
                let cwd = cwd.or_else(|| self.focused_cwd());
                self.split(dir, opts(cwd, command))?.to_string()
            }
            Command::Focus(target) => {
                if let FocusTarget::Pane(id) = target
                    && !self.tabs.iter().any(|t| t.root.contains(id))
                {
                    bail!("no pane {id}");
                }
                self.focus(target);
                String::new()
            }
            Command::Close { pane } => {
                let id = pane.or_else(|| self.focused_pane()).context("no pane")?;
                if !self.panes.contains_key(&id) {
                    bail!("no pane {id}");
                }
                self.close_pane(id);
                String::new()
            }
            Command::Zoom => {
                self.toggle_zoom();
                String::new()
            }
            Command::Resize { dir, amount } => {
                self.move_border(dir, amount);
                String::new()
            }
            Command::GoToTab(n) => {
                if n == 0 || n > self.tabs.len() {
                    bail!("no tab {n} (there are {})", self.tabs.len());
                }
                self.go_to_tab(n - 1);
                String::new()
            }
            Command::RenameTab(name) => {
                let tab = self.tabs.get_mut(self.active).context("no tab")?;
                tab.name = name;
                tab.auto_name = false;
                self.dirty = true;
                String::new()
            }
            Command::Write { pane, text } => {
                let id = match pane {
                    Some(id) => id,
                    None => self.focused_pane().context("no pane")?,
                };
                match self.panes.get(&id).context("no such pane")? {
                    Pane::Term(p) => p.write(text.into_bytes()),
                    Pane::Finder(f) => {
                        if let Some(editor) = f.editor.and_then(|e| self.term(e)) {
                            editor.write(text.into_bytes());
                        } else {
                            // Text for a finder is typed as keys, one character at a time.
                            for c in text.chars() {
                                let code = match c {
                                    '\r' | '\n' => KeyCode::Enter,
                                    '\x1b' => KeyCode::Esc,
                                    '\t' => KeyCode::Tab,
                                    '\x7f' => KeyCode::Backspace,
                                    c => KeyCode::Char(c),
                                };
                                let action = match self.panes.get_mut(&id) {
                                    Some(Pane::Finder(f)) => f.handle_key(&KeyEvent::new(code, KeyModifiers::NONE)),
                                    _ => break,
                                };
                                self.finder_action(id, action);
                            }
                        }
                    }
                }
                self.dirty = true;
                String::new()
            }
            Command::Finder { dir } => {
                let dir = match dir {
                    Some(d) => d,
                    None => self.focused_cwd().context("no folder given")?,
                };
                if !dir.is_dir() {
                    bail!("{} is not a folder", dir.display());
                }
                self.new_finder_tab(dir).to_string()
            }
            Command::DetachAll => {
                let ids: Vec<ClientId> =
                    self.clients.iter().filter(|(i, c)| c.attached && **i != from).map(|(i, _)| *i).collect();
                for id in ids {
                    self.detach(id);
                }
                String::new()
            }
        })
    }

    fn pane_list(&self) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        for (ti, tab) in self.tabs.iter().enumerate() {
            let layout = self.tab_layout(tab);
            for id in tab.root.panes() {
                let mut rect = layout.rect_of(id).unwrap_or_default();
                if matches!(self.panes.get(&id), Some(Pane::Term(_))) {
                    rect = self.body(rect);
                }
                let focused = ti == self.active && tab.focus == id;
                let (kind, title, cwd, editor) = match self.panes.get(&id) {
                    Some(Pane::Term(p)) => ("terminal", p.display_title(), p.cwd(), None),
                    Some(Pane::Finder(f)) => ("finder", "finder".to_owned(), Some(f.dir.clone()), f.editor),
                    None => continue,
                };
                out.push(serde_json::json!({
                    "id": id,
                    "tab": ti + 1,
                    "tab_name": self.tab_label(tab),
                    "kind": kind,
                    "title": title,
                    "name": self.pane_names.get(&id),
                    "focused": focused,
                    "visible": ti == self.active && (!tab.zoomed || tab.focus == id),
                    "x": rect.x, "y": rect.y, "cols": rect.width, "rows": rect.height,
                    "cwd": cwd,
                    "editor_pane": editor,
                }));
            }
        }
        out
    }

    // ----- drawing -----

    fn tab_label(&self, tab: &Tab) -> String {
        if !tab.auto_name {
            return tab.name.clone();
        }
        match self.panes.get(&tab.focus) {
            Some(Pane::Term(p)) => p.foreground_name().unwrap_or_else(|| tab.name.clone()),
            _ => tab.name.clone(),
        }
    }

    fn render_all(&mut self) {
        self.dirty = false;
        let ids: Vec<ClientId> = self.clients.iter().filter(|(_, c)| c.attached).map(|(id, _)| *id).collect();
        for id in ids {
            let (cols, rows, graphics) = match self.clients.get(&id) {
                Some(c) => (c.cols, c.rows, c.graphics),
                None => continue,
            };
            let frame = self.compose(cols, rows, graphics);
            for job in frame.jobs {
                self.images.request(job);
            }
            let Some(client) = self.clients.get_mut(&id) else { continue };
            client.tab_zones = frame.zones;
            let buf = frame.buf;
            let clear = client.prev.is_none();
            let mut prev = client.prev.take().unwrap_or_else(|| Buffer::empty(buf.area));
            let prev_cursor = if clear { None } else { client.prev_cursor };
            // A full redraw starts from a blank screen: every image is drawn again.
            let shown = if clear { Vec::new() } else { std::mem::take(&mut client.shown) };
            let touched: Vec<(u16, u16)> = prev.diff(&buf).iter().map(|(x, y, _)| (*x, *y)).collect();
            let changes =
                images::changes(&shown, &frame.images, |r| touched.iter().any(|(x, y)| layout::contains(r, *x, *y)));
            let mut overlay = Vec::new();
            if clear && graphics.protocol == ImageProtocol::Kitty {
                overlay.extend_from_slice(b"\x1b_Ga=d,d=A,q=2\x1b\\");
            }
            for p in &changes.remove {
                if p.key.protocol == ImageProtocol::Kitty {
                    overlay.extend(images::kitty_delete(p.key.kitty_id()));
                } else {
                    // Sixel and iTerm2 pictures are erased by drawing the cells under them again.
                    invalidate(&mut prev, p.rect);
                }
            }
            for p in &changes.add {
                overlay.extend(format!("\x1b[{};{}H", p.rect.y + 1, p.rect.x + 1).into_bytes());
                overlay.extend_from_slice(&p.bytes);
            }
            let bytes = encode_frame(&prev, &buf, prev_cursor, frame.cursor, clear, &overlay);
            if bytes.is_empty() {
                client.prev = Some(buf);
                client.shown = frame.images;
                continue;
            }
            match client.tx.try_send(ServerMsg::Output(bytes)) {
                Ok(()) => {
                    client.prev = Some(buf);
                    client.prev_cursor = frame.cursor;
                    client.shown = frame.images;
                }
                // The client is behind: skip this frame and redraw everything when it catches up.
                Err(TrySendError::Full(_)) => self.dirty = true,
                Err(TrySendError::Disconnected(_)) => {}
            }
        }
    }

    /// The full screen for a client of `cols` × `rows` whose terminal shows images as `graphics` says.
    fn compose(&self, cols: u16, rows: u16, graphics: protocol::Graphics) -> Frame {
        let mut buf = Buffer::empty(Rect::new(0, 0, cols, rows));
        let zones = self.draw_tab_bar(&mut buf);
        let content = self.content_area();
        // A client bigger than the layout sees dots outside it.
        if cols > content.width || rows.saturating_sub(2) > content.height {
            fill(&mut buf, Rect::new(0, 1, cols, rows.saturating_sub(2)), "·", Style::default().fg(Color::DarkGray));
            fill(&mut buf, content, " ", Style::default());
        }
        let mut cursor = None;
        let cursor_allowed = match &self.mode {
            Mode::Modal(m) => !matches!(m, Modal::Scroll | Modal::Search),
            Mode::Select(_) => true,
            _ => false,
        };
        // zellij colours the focused frame green, and differently while a mode is active.
        let accent = match self.mode {
            Mode::Modal(Modal::Normal | Modal::Locked) => Color::Green,
            _ => Color::Yellow,
        };
        // Real images need a protocol, no overlay on top, and for sixel the cell size in pixels.
        let protocol = graphics.protocol;
        let real = protocol != ImageProtocol::Blocks
            && !matches!(self.mode, Mode::Help)
            && (protocol != ImageProtocol::Sixel || graphics.cell_px.is_some());
        let mut slots = Vec::new();
        if let Some(tab) = self.tabs.get(self.active) {
            let layout = self.tab_layout(tab);
            for (id, rect) in &layout.panes {
                let focused = *id == tab.focus;
                let look = PaneLook { focused, show_cursor: focused && cursor_allowed, accent };
                if let Some(c) = self.draw_pane(&mut buf, *id, *rect, look, real.then_some(&mut slots)) {
                    cursor = Some(c);
                }
            }
            if !self.frames {
                draw_separators(&mut buf, &layout.separators, layout.rect_of(tab.focus));
            }
        }
        let mut placed = Vec::new();
        let mut jobs = Vec::new();
        for slot in slots {
            let cell = graphics.cell_px.unwrap_or(images::DEFAULT_CELL);
            let (rect, px) = images::fit(slot.img.width(), slot.img.height(), slot.area, cell, protocol);
            let key = images::ImageKey { id: slot.id, protocol, cells: (rect.width, rect.height), px };
            let on_screen = rect.right() <= cols && rect.bottom() <= rows.saturating_sub(1);
            match self.images.get(&key).filter(|_| on_screen) {
                Some(bytes) => placed.push(images::Placed { key, rect, bytes }),
                None => {
                    if on_screen {
                        jobs.push(images::Job { key, img: slot.img.clone() });
                    }
                    let (w, h) = (slot.area.width.max(1), slot.area.height.max(1));
                    finder::draw_blocks(&mut buf, &finder::view::image_cells(&slot.img, w, h), slot.area);
                }
            }
        }
        if rows >= 2 {
            self.draw_status(&mut buf, rows - 1);
        }
        if matches!(self.mode, Mode::Help) {
            self.draw_help(&mut buf);
            cursor = None;
        }
        // Never place the cursor on the tab or status bar.
        let cursor = cursor.filter(|c| c.y >= 1 && c.y + 1 < rows && c.x < cols);
        Frame { buf, cursor, zones, images: placed, jobs }
    }

    /// A pane's frame title: its name if it was renamed, otherwise its title or program.
    fn pane_title(&self, id: PaneId) -> String {
        if let Mode::RenamePane { pane, .. } = &self.mode
            && *pane == id
        {
            return format!("{}▏", self.pane_names.get(&id).map_or("", String::as_str));
        }
        if let Some(name) = self.pane_names.get(&id).filter(|n| !n.is_empty()) {
            return name.clone();
        }
        match self.panes.get(&id) {
            Some(Pane::Term(p)) => p.display_title(),
            Some(Pane::Finder(f)) => finder::home_relative(&f.dir),
            None => String::new(),
        }
    }

    fn draw_pane(
        &self,
        buf: &mut Buffer,
        id: PaneId,
        rect: Rect,
        look: PaneLook,
        slots: Option<&mut Vec<finder::ImageSlot>>,
    ) -> Option<Cursor> {
        let PaneLook { focused, show_cursor, accent } = look;
        match self.panes.get(&id)? {
            Pane::Term(p) => {
                let body = self.body(rect);
                let (scroll, others) = {
                    let term = p.term.lock();
                    let offset = term.grid().display_offset();
                    let scroll = (offset > 0).then(|| format!("SCROLL: {offset}/{}", term.grid().history_size()));
                    let others = match &self.search {
                        Some(s) if s.pane == id => s.visible(&term),
                        _ => Vec::new(),
                    };
                    (scroll, others)
                };
                if self.frames {
                    let style = if focused {
                        Style::default().fg(accent).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    draw_frame(buf, rect, &self.pane_title(id), scroll.as_deref().unwrap_or(""), style);
                }
                let search_match = match &self.mode {
                    Mode::Select(c) if c.pane == id => c.last_match.as_ref(),
                    _ => self.search.as_ref().filter(|s| s.pane == id).and_then(|s| s.current.as_ref()),
                };
                let decor =
                    TermDecor { show_cursor, search_match, other_matches: &others, scroll_overlay: !self.frames };
                draw_term(buf, body, p, &decor)
            }
            Pane::Finder(f) => {
                let editor = f.editor.and_then(|e| self.term(e));
                f.render(buf, rect, focused, accent, editor, slots).filter(|_| show_cursor)
            }
        }
    }

    fn draw_tab_bar(&self, buf: &mut Buffer) -> Vec<TabZone> {
        let width = buf.area.width;
        let mut zones = Vec::new();
        let mut x = 0u16;
        let normal = Style::default().fg(Color::White).bg(Color::DarkGray);
        let active = Style::default().fg(Color::Black).bg(Color::Green).add_modifier(Modifier::BOLD);
        let session = format!(" {} ", self.name);
        let right = width.saturating_sub(session.len() as u16);
        let renaming = matches!(self.mode, Mode::RenameTab { .. });
        for (i, tab) in self.tabs.iter().enumerate() {
            let zoom = if tab.zoomed { " (FULLSCREEN)" } else { "" };
            let sync = if tab.sync { " (SYNC)" } else { "" };
            let name = if renaming && i == self.active { format!("{}▏", tab.name) } else { self.tab_label(tab) };
            let label = format!(" {} {name}{zoom}{sync} ", i + 1);
            let style = if i == self.active { active } else { normal };
            let w = put_str(buf, x, 0, &label, style, right.saturating_sub(x));
            zones.push((x, x + w, Some(i)));
            x += w + 1;
            if x >= right {
                break;
            }
        }
        if x + 3 <= right {
            let w = put_str(buf, x, 0, " + ", Style::default().fg(Color::DarkGray), 3);
            zones.push((x, x + w, None));
        }
        put_str(buf, right, 0, &session, Style::default().fg(Color::Black).bg(Color::Blue), width - right);
        zones
    }
}

/// How to draw a pane: whether it has focus, shows the program's cursor, and its frame colour.
#[derive(Clone, Copy)]
struct PaneLook {
    focused: bool,
    show_cursor: bool,
    accent: Color,
}

/// Makes the cells in `rect` look changed, so the next frame draws them again (erasing a picture there).
fn invalidate(buf: &mut Buffer, rect: Rect) {
    let rect = rect.intersection(buf.area);
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol("\u{1}");
            }
        }
    }
}

/// The grid point under screen position (x, y) inside a pane drawn at `rect`.
fn viewport_point<T>(term: &alacritty_terminal::Term<T>, rect: Rect, x: u16, y: u16) -> Point {
    let col = usize::from(x.clamp(rect.x, rect.x + rect.width.saturating_sub(1)) - rect.x);
    let row = i32::from(y.clamp(rect.y, rect.y + rect.height.saturating_sub(1)) - rect.y);
    let offset = term.grid().display_offset() as i32;
    let col = col.min(term.columns().saturating_sub(1));
    Point::new(Line(row - offset), Column(col))
}
