//! Messages between the client (or a CLI command) and the session server.
//!
//! Every message is a frame: a 4-byte little-endian length, then the message encoded with `postcard`.
//! The first message on a connection is always `ClientMsg::Hello`, and its first field is the protocol
//! version, so a mismatched client and server can still tell each other apart.

use std::io::{self, Read, Write};
use std::path::PathBuf;

use crossterm::event::{KeyEvent, MouseEvent};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 3;
const MAX_FRAME: usize = 64 << 20;

pub type PaneId = u32;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientMsg {
    Hello { version: u32, kind: ClientKind },
    Input(InputEvent),
    Resize { cols: u16, rows: u16 },
    Detach,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ClientKind {
    /// An interactive client showing the session.
    Attach { cols: u16, rows: u16, graphics: Graphics },
    /// A one-shot CLI command; the server replies once.
    Command(Command),
    /// `tb subscribe`: events as JSON lines until the session ends. `panes` also send their text when it
    /// changes; `scrollback` adds history to the first delivery (`Some(None)`: all of it).
    Subscribe { panes: Vec<PaneId>, scrollback: Option<Option<usize>>, ansi: bool },
    /// `tb watch`: like `Attach`, but read-only. Its keys are ignored and its size never changes the layout.
    Watch { cols: u16, rows: u16, graphics: Graphics },
}

/// How a client's terminal shows images.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ImageProtocol {
    /// Coloured half-blocks: works everywhere.
    #[default]
    Blocks,
    Kitty,
    Iterm2,
    Sixel,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Graphics {
    pub protocol: ImageProtocol,
    /// The size of one cell in pixels, if the terminal told us.
    pub cell_px: Option<(u16, u16)>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum InputEvent {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Paste(String),
    FocusGained,
    FocusLost,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ServerMsg {
    /// Terminal output for an attached client to write as-is.
    Output(Vec<u8>),
    /// The answer to a `Command`.
    Reply { ok: bool, text: String },
    /// The client should restore its terminal and exit.
    Exit { reason: String },
    /// One event for a subscriber: a JSON object.
    Event(String),
    /// The client should attach to another session (`switch-session`).
    Switch { session: String },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    Right,
    Down,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub enum FocusTarget {
    Dir(Direction),
    Next,
    Prev,
    Pane(PaneId),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Command {
    /// Session summary for `tb ls`.
    Info,
    /// End the session. `forget` also deletes its saved layout; otherwise it can be restored.
    Kill {
        forget: bool,
    },
    /// Open a first tab if the session has none (after a restore it usually has).
    EnsureTab,
    Finder {
        dir: Option<PathBuf>,
    },
    /// `tb action …`: zellij's actions and `tb`'s own (step 6, the extension API).
    Action(CliAction),
}

/// What `tb action` asks the server to do. Queries answer with JSON; the CLI formats it as text unless
/// asked for JSON. `pane: None` means the focused pane, `tab: None` the active tab.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CliAction {
    // Queries.
    ListTabs,
    CurrentTabInfo,
    ListPanes,
    ListClients,
    DumpScreen {
        pane: Option<PaneId>,
        full: bool,
        ansi: bool,
    },
    DumpLayout,
    // Tabs.
    NewTab {
        name: Option<String>,
        cwd: Option<PathBuf>,
        command: Vec<String>,
        start_suspended: bool,
    },
    CloseTab {
        tab: Option<u32>,
    },
    GoToTab(usize),
    GoToTabId(u32),
    GoToTabName {
        name: String,
        create: bool,
    },
    CycleTab {
        next: bool,
    },
    RenameTab {
        tab: Option<u32>,
        name: String,
    },
    UndoRenameTab {
        tab: Option<u32>,
    },
    MoveTab {
        tab: Option<u32>,
        right: bool,
    },
    ToggleSync {
        tab: Option<u32>,
    },
    // Panes.
    NewPane {
        dir: Option<SplitDir>,
        name: Option<String>,
        cwd: Option<PathBuf>,
        command: Vec<String>,
        start_suspended: bool,
    },
    Edit {
        file: PathBuf,
        line: Option<u32>,
        dir: Option<SplitDir>,
        cwd: Option<PathBuf>,
    },
    ClosePane {
        pane: Option<PaneId>,
    },
    Focus(FocusTarget),
    FocusOrTab(Direction),
    /// Swap the pane with its neighbour in `dir`, or with the next pane (`None`).
    MovePane {
        pane: Option<PaneId>,
        dir: Option<Direction>,
    },
    MovePaneBackwards {
        pane: Option<PaneId>,
    },
    RenamePane {
        pane: Option<PaneId>,
        name: String,
    },
    UndoRenamePane {
        pane: Option<PaneId>,
    },
    ToggleFullscreen {
        pane: Option<PaneId>,
    },
    ToggleFrames,
    /// zellij's resize: grow or shrink on one side (or all sides with `None`).
    Resize {
        pane: Option<PaneId>,
        side: Option<Direction>,
        grow: bool,
    },
    /// `tb`'s resize: move the nearest border in `dir` by `amount` cells.
    MoveBorder {
        pane: Option<PaneId>,
        dir: Direction,
        amount: u16,
    },
    Clear {
        pane: Option<PaneId>,
    },
    EditScrollback {
        pane: Option<PaneId>,
    },
    SetPaneColor {
        pane: Option<PaneId>,
        fg: Option<String>,
        bg: Option<String>,
        reset: bool,
    },
    Scroll {
        pane: Option<PaneId>,
        how: ScrollHow,
    },
    // Input.
    Write {
        pane: Option<PaneId>,
        bytes: Vec<u8>,
    },
    Paste {
        pane: Option<PaneId>,
        text: String,
    },
    SendKeys {
        pane: Option<PaneId>,
        keys: Vec<KeyEvent>,
    },
    // Session.
    SwitchMode(String),
    RenameSession(String),
    SaveSession,
    /// Detach every attached client.
    Detach,
    /// Move the active client to another session (starting it if needed).
    SwitchSession(String),
    /// Show `text` in the status bar as segment `id` (replacing that segment's text); `ttl` in seconds.
    SetStatus {
        id: String,
        text: String,
        ttl: Option<u64>,
    },
    /// Remove segment `id`, or every segment.
    ClearStatus {
        id: Option<String>,
    },
    /// Read config.kdl again; answers with the warnings.
    ReloadConfig,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollHow {
    Up,
    Down,
    PageUp,
    PageDown,
    HalfPageUp,
    HalfPageDown,
    Top,
    Bottom,
}

pub fn write_frame<W: Write, T: Serialize>(w: &mut W, msg: &T) -> io::Result<()> {
    let bytes = postcard::to_stdvec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(&bytes)?;
    w.flush()
}

pub fn read_frame<R: Read, T: DeserializeOwned>(r: &mut R) -> io::Result<T> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0; len];
    r.read_exact(&mut buf)?;
    postcard::from_bytes(&buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};

    #[test]
    fn frames_round_trip() {
        let msg = ClientMsg::Input(InputEvent::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)));
        let mut buf = Vec::new();
        write_frame(&mut buf, &msg).unwrap();
        write_frame(&mut buf, &ServerMsg::Output(b"hi".to_vec())).unwrap();
        let mut r = &buf[..];
        match read_frame::<_, ClientMsg>(&mut r).unwrap() {
            ClientMsg::Input(InputEvent::Key(k)) => {
                assert_eq!(k.code, KeyCode::Char('b'));
                assert_eq!(k.modifiers, KeyModifiers::CONTROL);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(read_frame::<_, ServerMsg>(&mut r).unwrap(), ServerMsg::Output(b) if b == b"hi"));
    }

    #[test]
    fn hello_starts_with_the_version() {
        let hello = ClientMsg::Hello { version: PROTOCOL_VERSION, kind: ClientKind::Command(Command::Info) };
        let bytes = postcard::to_stdvec(&hello).unwrap();
        // Variant 0, then the version as a varint.
        assert_eq!(&bytes[..2], &[0, PROTOCOL_VERSION as u8]);
    }
}
