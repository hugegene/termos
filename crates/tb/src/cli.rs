//! `tb action …`: zellij's actions with zellij's names, flags and output, plus `tb`'s own (rebuild step 6).
//!
//! The server answers queries with JSON (zellij's field names); this module prints zellij's text columns
//! unless `--json` is given. Formats: `docs/findings/2026-11-05-zellij-action-formats.md`.

use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Args, Subcommand, ValueEnum};
use serde_json::Value;

use crate::client;
use crate::keyspec;
use crate::protocol::{CliAction, Command, Direction, FocusTarget, PaneId, ScrollHow, SplitDir};

#[derive(Clone, Copy, ValueEnum)]
pub enum SplitArg {
    Right,
    Down,
}

impl From<SplitArg> for SplitDir {
    fn from(d: SplitArg) -> Self {
        match d {
            SplitArg::Right => SplitDir::Right,
            SplitArg::Down => SplitDir::Down,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum DirArg {
    Left,
    Right,
    Up,
    Down,
}

impl From<DirArg> for Direction {
    fn from(d: DirArg) -> Self {
        match d {
            DirArg::Left => Direction::Left,
            DirArg::Right => Direction::Right,
            DirArg::Up => Direction::Up,
            DirArg::Down => Direction::Down,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
pub enum LeftRight {
    Left,
    Right,
}

/// A pane id as zellij takes it: `3`, `terminal_3` (or `finder_3` in `tb`).
pub fn pane_id(s: &str) -> Result<PaneId, String> {
    if s.starts_with("plugin_") {
        return Err(format!("'{s}': tb has no plugin panes"));
    }
    let n = ["terminal_", "finder_", "preview_"].iter().find_map(|k| s.strip_prefix(k)).unwrap_or(s);
    n.parse().map_err(|_| format!("not a pane id: '{s}' (use 3 or terminal_3)"))
}

// `-p/--pane-id`, as zellij's pane actions take it. `--pane` is `tb`'s older name. (Plain comments on these
// option structs: a doc comment would become the help text of every command that has no text of its own.)
#[derive(Args, Default, Clone)]
pub struct PaneOpt {
    /// The pane, e.g. terminal_3 or 3 (default: the focused pane)
    #[arg(short, long = "pane-id", alias = "pane", value_parser = pane_id)]
    pub pane_id: Option<PaneId>,
}

// `-t/--tab-id`, as zellij's tab actions take it.
#[derive(Args, Default, Clone)]
pub struct TabOpt {
    /// The tab's id, as `list-tabs` shows it (default: the active tab)
    #[arg(short, long)]
    pub tab_id: Option<u32>,
}

// `new-pane` and `run`: where the pane goes and what runs in it.
#[derive(Args, Clone, Default)]
pub struct PaneArgs {
    /// Open beside the focused pane (default: where there's most room)
    #[arg(short, long, value_enum)]
    pub direction: Option<SplitArg>,
    /// Name the pane (shown in its frame)
    #[arg(short, long)]
    pub name: Option<String>,
    /// The folder to start in (default: the focused pane's)
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Type COMMAND at a shell's prompt without running it
    #[arg(short, long)]
    pub start_suspended: bool,
    /// Accepted for zellij: tb closes a pane when its command exits
    #[arg(short, long)]
    #[allow(dead_code)]
    pub close_on_exit: bool,
    /// The command to run (default: a shell)
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

#[derive(Args, Clone, Default)]
pub struct ListTabsArgs {
    /// Print JSON
    #[arg(short, long)]
    pub json: bool,
    /// Every column
    #[arg(short, long)]
    pub all: bool,
    /// Size columns
    #[arg(short, long)]
    pub dimensions: bool,
    /// The swap-layout column
    #[arg(short, long)]
    pub layout: bool,
    /// The pane-count column
    #[arg(short, long)]
    pub panes: bool,
    /// Active, fullscreen and sync columns
    #[arg(short, long)]
    pub state: bool,
}

#[derive(Args, Clone, Default)]
pub struct ListPanesArgs {
    /// Print JSON
    #[arg(short, long)]
    pub json: bool,
    /// Every column
    #[arg(short, long)]
    pub all: bool,
    /// Command and folder columns
    #[arg(short, long)]
    pub command: bool,
    /// Position and size columns
    #[arg(short, long)]
    pub geometry: bool,
    /// Focused, floating and exited columns
    #[arg(short, long)]
    pub state: bool,
    /// Tab columns
    #[arg(short, long)]
    pub tab: bool,
}

#[derive(Args, Clone, Default)]
pub struct DumpScreenArgs {
    #[command(flatten)]
    pub p: PaneOpt,
    /// Include the scrollback
    #[arg(short, long)]
    pub full: bool,
    /// Keep colours and styles (ANSI escape sequences)
    #[arg(short, long)]
    pub ansi: bool,
    /// Write to this file instead of printing
    #[arg(long)]
    pub path: Option<PathBuf>,
}

/// `tb action`: zellij's actions (same names and flags), and `tb`'s own. Commands that create a tab or a
/// pane print its id.
#[derive(Subcommand)]
pub enum ActionCmd {
    // ----- tabs -----
    /// Open a new tab running a shell, or COMMAND; prints the tab's id
    #[command(
        after_long_help = "Examples:\n  tb action new-tab -n build -c ~/project -- make watch\n  tb action new-tab --start-suspended -- make deploy   # typed, not run"
    )]
    NewTab {
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        cwd: Option<PathBuf>,
        /// Type COMMAND at a shell's prompt without running it
        #[arg(long)]
        start_suspended: bool,
        /// Accepted for zellij: tb closes a pane when its command exits
        #[arg(long)]
        #[allow(dead_code)]
        close_on_exit: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Close a tab and everything in it
    CloseTab {
        #[command(flatten)]
        t: TabOpt,
    },
    /// Close the tab with this id
    CloseTabById { id: u32 },
    /// Go to tab N (counting from 1)
    GoToTab { index: usize },
    /// Go to the tab with this id
    GoToTabById { id: u32 },
    /// Go to the tab with this name
    #[command(
        after_long_help = "Examples:\n  tb action go-to-tab-name logs\n  tb action go-to-tab-name -c scratch   # create it if it's missing"
    )]
    GoToTabName {
        name: String,
        /// Create it if there's none
        #[arg(short, long)]
        create: bool,
    },
    /// Go to the next tab (wrapping around)
    GoToNextTab,
    /// Go to the previous tab (wrapping around)
    GoToPreviousTab,
    /// Rename a tab
    RenameTab {
        name: String,
        #[command(flatten)]
        t: TabOpt,
    },
    /// Rename the tab with this id
    RenameTabById { id: u32, name: String },
    /// Remove a tab's name, so it follows its folder again
    UndoRenameTab {
        #[command(flatten)]
        t: TabOpt,
    },
    /// Move a tab one place left or right
    MoveTab {
        #[arg(value_enum)]
        direction: LeftRight,
        #[command(flatten)]
        t: TabOpt,
    },
    /// Type into every pane of a tab at once, or stop
    ToggleActiveSyncTab {
        #[command(flatten)]
        t: TabOpt,
    },
    /// List tabs
    #[command(
        after_long_help = "Examples:\n  tb action list-tabs -s\n  tb action list-tabs -j | jq '.[] | select(.active) | .tab_id'"
    )]
    ListTabs(ListTabsArgs),
    /// Print each tab's name
    QueryTabNames,
    /// The active tab
    CurrentTabInfo {
        #[arg(short, long)]
        json: bool,
    },

    // ----- panes -----
    /// Open a pane running a shell, or COMMAND; prints the pane's id
    #[command(
        after_long_help = "Examples:\n  tb action new-pane -d right -- htop\n  tb action new-pane -n logs -- tail -f /var/log/syslog"
    )]
    NewPane(PaneArgs),
    /// Split the focused pane (tb's form of new-pane)
    Split {
        #[arg(value_enum)]
        dir: SplitArg,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Open FILE in the editor in a new pane; prints the pane's id
    Edit(EditArgs),
    /// Close a pane
    #[command(visible_alias = "close")]
    ClosePane {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Focus a pane by id
    FocusPaneId {
        #[arg(value_parser = pane_id)]
        pane_id: PaneId,
    },
    /// Focus the next pane in the tab
    FocusNextPane,
    /// Focus the previous pane in the tab
    FocusPreviousPane,
    /// Focus the pane in a direction
    MoveFocus {
        #[arg(value_enum)]
        direction: DirArg,
    },
    /// Focus the pane in a direction, or the next tab at the edge
    MoveFocusOrTab {
        #[arg(value_enum)]
        direction: DirArg,
    },
    /// Focus a pane: left, right, up, down, next, prev, or an id (tb's form)
    Focus { target: String },
    /// Swap a pane with its neighbour in a direction, or with the next pane
    MovePane {
        #[arg(value_enum)]
        direction: Option<DirArg>,
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Swap a pane with the previous pane
    MovePaneBackwards {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Name a pane (shown in its frame)
    RenamePane {
        name: String,
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Remove a pane's name
    UndoRenamePane {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Show a pane alone in its tab, or stop
    #[command(visible_alias = "zoom")]
    ToggleFullscreen {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Turn pane frames on or off
    TogglePaneFrames,
    /// Resize a pane: `increase|decrease [left|right|up|down]` (zellij), or `DIRECTION [CELLS]` (tb)
    #[command(
        after_long_help = "Examples:\n  tb action resize increase left\n  tb action resize decrease          # every side\n  tb action resize down 5            # tb's form: move the border 5 cells"
    )]
    Resize {
        how: String,
        side: Option<String>,
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Clear a pane's screen and scrollback
    Clear {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Open a pane's scrollback in the editor
    EditScrollback {
        #[command(flatten)]
        p: PaneOpt,
        /// Accepted for zellij; the editor gets plain text
        #[arg(short, long)]
        #[allow(dead_code)]
        ansi: bool,
    },
    /// Set a pane's default colours ("#00e000" or "rgb:00/e0/00"); default: the pane this runs in
    #[command(
        after_long_help = "Examples:\n  tb action set-pane-color --bg \"#3a0000\"   # tint the pane this runs in\n  tb action set-pane-color -p 3 --reset"
    )]
    SetPaneColor {
        #[command(flatten)]
        p: PaneOpt,
        #[arg(long)]
        fg: Option<String>,
        #[arg(long)]
        bg: Option<String>,
        /// Back to the terminal's colours
        #[arg(long)]
        reset: bool,
    },
    /// Scroll a pane up one line
    ScrollUp {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane down one line
    ScrollDown {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane up a page
    PageScrollUp {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane down a page
    PageScrollDown {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane up half a page
    HalfPageScrollUp {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane down half a page
    HalfPageScrollDown {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane to the top of its scrollback
    ScrollToTop {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Scroll a pane back to the bottom
    ScrollToBottom {
        #[command(flatten)]
        p: PaneOpt,
    },
    /// List panes
    #[command(
        after_long_help = "Examples:\n  tb action list-panes -t -c\n  tb action list-panes -j | jq '.[] | select(.is_focused) | .id'"
    )]
    ListPanes(ListPanesArgs),
    /// Print a pane's text
    #[command(
        after_long_help = "Examples:\n  tb action dump-screen -f > scrollback.txt\n  tb action dump-screen -p 3 --ansi --path pane3.txt"
    )]
    DumpScreen(DumpScreenArgs),
    /// Print the session's layout (tb's saved-session JSON)
    DumpLayout,
    /// List attached clients
    ListClients,

    // ----- input -----
    /// Send bytes, given as numbers (e.g. 27 91 65)
    Write {
        #[arg(required = true)]
        bytes: Vec<u8>,
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Type text into a pane
    #[command(
        after_long_help = "Examples:\n  tb action write-chars --enter \"make test\"\n  tb action write-chars -p terminal_3 q"
    )]
    WriteChars {
        chars: String,
        #[command(flatten)]
        p: PaneOpt,
        /// Press Enter after the text (tb)
        #[arg(long)]
        enter: bool,
    },
    /// Paste text (bracketed, if the program asked for it)
    Paste {
        chars: String,
        #[command(flatten)]
        p: PaneOpt,
    },
    /// Send keys, e.g. "Ctrl a" "F1" "Alt Shift b" Enter
    #[command(after_long_help = "Examples:\n  tb action send-keys \"Ctrl c\"\n  tb action send-keys -p 3 Up Enter")]
    SendKeys {
        #[arg(required = true)]
        keys: Vec<String>,
        #[command(flatten)]
        p: PaneOpt,
    },

    // ----- session -----
    /// Switch every client to a mode: normal, locked, pane, tab, resize, move, scroll, search, session, tmux
    SwitchMode { mode: String },
    /// Rename the session
    RenameSession { name: String },
    /// Save the session's layout now
    SaveSession,
    /// Detach every attached client
    Detach,
    /// Move your client to another session, starting it if needed
    SwitchSession { name: String },
    /// Show TEXT in the status bar (tb). Each --id is its own segment; clicks reach `tb subscribe`
    #[command(
        after_long_help = "Examples:\n  tb action set-status --id ci \"build ✓\"\n  tb action set-status --id prbot --ttl 60 \"PR #487 needs review\"\n  tb action clear-status --id ci"
    )]
    SetStatus {
        text: String,
        /// The segment to set (one per extension)
        #[arg(long, default_value = "default")]
        id: String,
        /// Remove the text after this many seconds
        #[arg(long)]
        ttl: Option<u64>,
    },
    /// Remove a status segment, or all of them (tb)
    ClearStatus {
        #[arg(long)]
        id: Option<String>,
    },
    /// Read ~/.config/tb/config.kdl again and print what it skipped (tb)
    ReloadConfig,
}

#[derive(Args, Clone)]
pub struct EditArgs {
    pub file: PathBuf,
    /// Open at this line
    #[arg(short, long)]
    pub line_number: Option<u32>,
    #[arg(short, long, value_enum)]
    pub direction: Option<SplitArg>,
    #[arg(long)]
    pub cwd: Option<PathBuf>,
}

fn absolute(p: PathBuf) -> anyhow::Result<PathBuf> {
    Ok(if p.is_absolute() { p } else { std::env::current_dir()?.join(p) })
}

/// What to do with the server's answer.
enum Output {
    /// Print it as it is (if not empty).
    Plain,
    Tabs(ListTabsArgs),
    TabNames,
    CurrentTab {
        json: bool,
    },
    Panes(ListPanesArgs),
    Clients,
    Screen(Option<PathBuf>),
}

/// The pane `tb` runs in, if it runs in this session: zellij's `set-pane-color` defaults to it.
fn own_pane(session: &str) -> Option<PaneId> {
    (std::env::var("TB_SESSION").ok()? == session).then(|| std::env::var("TB_PANE_ID").ok()?.parse().ok())?
}

fn resize(how: &str, side: Option<&str>, pane: Option<PaneId>) -> anyhow::Result<CliAction> {
    let dir = |s: &str| -> anyhow::Result<Direction> {
        Ok(match s.to_ascii_lowercase().as_str() {
            "left" => Direction::Left,
            "right" => Direction::Right,
            "up" => Direction::Up,
            "down" => Direction::Down,
            _ => bail!("not a direction: '{s}' (left, right, up, down)"),
        })
    };
    let grow = match how.to_ascii_lowercase().as_str() {
        "increase" | "+" => Some(true),
        "decrease" | "-" => Some(false),
        _ => None,
    };
    Ok(match grow {
        Some(grow) => CliAction::Resize { pane, side: side.map(dir).transpose()?, grow },
        None => {
            let amount = match side {
                Some(n) => n.parse().with_context(|| format!("not a number of cells: '{n}'"))?,
                None => 2,
            };
            CliAction::MoveBorder { pane, dir: dir(how)?, amount }
        }
    })
}

fn scroll(p: PaneOpt, how: ScrollHow) -> CliAction {
    CliAction::Scroll { pane: p.pane_id, how }
}

/// Turns a `tb action` command into the request for the server and how to print the answer.
fn request(session: &str, cmd: ActionCmd) -> anyhow::Result<(CliAction, Output)> {
    use CliAction as A;
    let plain = |a| Ok((a, Output::Plain));
    match cmd {
        ActionCmd::NewTab { name, cwd, start_suspended, command, .. } => {
            plain(A::NewTab { name, cwd: cwd.map(absolute).transpose()?, command, start_suspended })
        }
        ActionCmd::CloseTab { t } => plain(A::CloseTab { tab: t.tab_id }),
        ActionCmd::CloseTabById { id } => plain(A::CloseTab { tab: Some(id) }),
        ActionCmd::GoToTab { index } => plain(A::GoToTab(index)),
        ActionCmd::GoToTabById { id } => plain(A::GoToTabId(id)),
        ActionCmd::GoToTabName { name, create } => plain(A::GoToTabName { name, create }),
        ActionCmd::GoToNextTab => plain(A::CycleTab { next: true }),
        ActionCmd::GoToPreviousTab => plain(A::CycleTab { next: false }),
        ActionCmd::RenameTab { name, t } => plain(A::RenameTab { tab: t.tab_id, name }),
        ActionCmd::RenameTabById { id, name } => plain(A::RenameTab { tab: Some(id), name }),
        ActionCmd::UndoRenameTab { t } => plain(A::UndoRenameTab { tab: t.tab_id }),
        ActionCmd::MoveTab { direction, t } => {
            plain(A::MoveTab { tab: t.tab_id, right: matches!(direction, LeftRight::Right) })
        }
        ActionCmd::ToggleActiveSyncTab { t } => plain(A::ToggleSync { tab: t.tab_id }),
        ActionCmd::ListTabs(args) => Ok((A::ListTabs, Output::Tabs(args))),
        ActionCmd::QueryTabNames => Ok((A::ListTabs, Output::TabNames)),
        ActionCmd::CurrentTabInfo { json } => Ok((A::CurrentTabInfo, Output::CurrentTab { json })),
        ActionCmd::NewPane(a) => plain(new_pane(a)?),
        ActionCmd::Split { dir, cwd, command } => plain(A::NewPane {
            dir: Some(dir.into()),
            name: None,
            cwd: cwd.map(absolute).transpose()?,
            command,
            start_suspended: false,
        }),
        ActionCmd::Edit(a) => plain(edit(a)?),
        ActionCmd::ClosePane { p } => plain(A::ClosePane { pane: p.pane_id }),
        ActionCmd::FocusPaneId { pane_id } => plain(A::Focus(FocusTarget::Pane(pane_id))),
        ActionCmd::FocusNextPane => plain(A::Focus(FocusTarget::Next)),
        ActionCmd::FocusPreviousPane => plain(A::Focus(FocusTarget::Prev)),
        ActionCmd::MoveFocus { direction } => plain(A::Focus(FocusTarget::Dir(direction.into()))),
        ActionCmd::MoveFocusOrTab { direction } => plain(A::FocusOrTab(direction.into())),
        ActionCmd::Focus { target } => plain(A::Focus(match target.as_str() {
            "left" => FocusTarget::Dir(Direction::Left),
            "right" => FocusTarget::Dir(Direction::Right),
            "up" => FocusTarget::Dir(Direction::Up),
            "down" => FocusTarget::Dir(Direction::Down),
            "next" => FocusTarget::Next,
            "prev" => FocusTarget::Prev,
            id => match pane_id(id) {
                Ok(id) => FocusTarget::Pane(id),
                Err(_) => bail!("focus target must be left, right, up, down, next, prev or a pane id"),
            },
        })),
        ActionCmd::MovePane { direction, p } => plain(A::MovePane { pane: p.pane_id, dir: direction.map(Into::into) }),
        ActionCmd::MovePaneBackwards { p } => plain(A::MovePaneBackwards { pane: p.pane_id }),
        ActionCmd::RenamePane { name, p } => plain(A::RenamePane { pane: p.pane_id, name }),
        ActionCmd::UndoRenamePane { p } => plain(A::UndoRenamePane { pane: p.pane_id }),
        ActionCmd::ToggleFullscreen { p } => plain(A::ToggleFullscreen { pane: p.pane_id }),
        ActionCmd::TogglePaneFrames => plain(A::ToggleFrames),
        ActionCmd::Resize { how, side, p } => plain(resize(&how, side.as_deref(), p.pane_id)?),
        ActionCmd::Clear { p } => plain(A::Clear { pane: p.pane_id }),
        ActionCmd::EditScrollback { p, .. } => plain(A::EditScrollback { pane: p.pane_id }),
        ActionCmd::SetPaneColor { p, fg, bg, reset } => {
            if fg.is_none() && bg.is_none() && !reset {
                bail!("give --fg, --bg or --reset");
            }
            plain(A::SetPaneColor { pane: p.pane_id.or_else(|| own_pane(session)), fg, bg, reset })
        }
        ActionCmd::ScrollUp { p } => plain(scroll(p, ScrollHow::Up)),
        ActionCmd::ScrollDown { p } => plain(scroll(p, ScrollHow::Down)),
        ActionCmd::PageScrollUp { p } => plain(scroll(p, ScrollHow::PageUp)),
        ActionCmd::PageScrollDown { p } => plain(scroll(p, ScrollHow::PageDown)),
        ActionCmd::HalfPageScrollUp { p } => plain(scroll(p, ScrollHow::HalfPageUp)),
        ActionCmd::HalfPageScrollDown { p } => plain(scroll(p, ScrollHow::HalfPageDown)),
        ActionCmd::ScrollToTop { p } => plain(scroll(p, ScrollHow::Top)),
        ActionCmd::ScrollToBottom { p } => plain(scroll(p, ScrollHow::Bottom)),
        ActionCmd::ListPanes(args) => Ok((A::ListPanes, Output::Panes(args))),
        ActionCmd::DumpScreen(a) => {
            let path = a.path.map(absolute).transpose()?;
            Ok((A::DumpScreen { pane: a.p.pane_id, full: a.full, ansi: a.ansi }, Output::Screen(path)))
        }
        ActionCmd::DumpLayout => plain(A::DumpLayout),
        ActionCmd::ListClients => Ok((A::ListClients, Output::Clients)),
        ActionCmd::Write { bytes, p } => plain(A::Write { pane: p.pane_id, bytes }),
        ActionCmd::WriteChars { chars, p, enter } => {
            let text = if enter { chars + "\r" } else { chars };
            plain(A::Write { pane: p.pane_id, bytes: text.into_bytes() })
        }
        ActionCmd::Paste { chars, p } => plain(A::Paste { pane: p.pane_id, text: chars }),
        ActionCmd::SendKeys { keys, p } => {
            let keys =
                keys.iter().map(|k| keyspec::parse(k)).collect::<Result<Vec<_>, _>>().map_err(anyhow::Error::msg)?;
            plain(A::SendKeys { pane: p.pane_id, keys })
        }
        ActionCmd::SwitchMode { mode } => plain(A::SwitchMode(mode)),
        ActionCmd::RenameSession { name } => plain(A::RenameSession(name)),
        ActionCmd::SaveSession => plain(A::SaveSession),
        ActionCmd::Detach => plain(A::Detach),
        ActionCmd::SwitchSession { name } => plain(A::SwitchSession(name)),
        ActionCmd::SetStatus { text, id, ttl } => plain(A::SetStatus { id, text, ttl }),
        ActionCmd::ClearStatus { id } => plain(A::ClearStatus { id }),
        ActionCmd::ReloadConfig => plain(A::ReloadConfig),
    }
}

pub fn new_pane(a: PaneArgs) -> anyhow::Result<CliAction> {
    Ok(CliAction::NewPane {
        dir: a.direction.map(Into::into),
        name: a.name,
        cwd: a.cwd.map(absolute).transpose()?,
        command: a.command,
        start_suspended: a.start_suspended,
    })
}

pub fn edit(a: EditArgs) -> anyhow::Result<CliAction> {
    Ok(CliAction::Edit {
        file: absolute(a.file)?,
        line: a.line_number,
        dir: a.direction.map(Into::into),
        cwd: a.cwd.map(absolute).transpose()?,
    })
}

/// Runs a `tb action` command against `session` and prints the answer.
pub fn run(session: &str, cmd: ActionCmd) -> anyhow::Result<()> {
    let (action, output) = request(session, cmd)?;
    send(session, action, output)
}

/// Sends one action and prints the answer the way `output` says.
fn send(session: &str, action: CliAction, output: Output) -> anyhow::Result<()> {
    let text = client::command(session, Command::Action(action))?;
    let json =
        |text: &str| -> anyhow::Result<Value> { serde_json::from_str(text).context("reading the session's answer") };
    let out = match output {
        Output::Plain => text,
        Output::Tabs(args) if args.json => text,
        Output::Tabs(args) => format_tabs(&json(&text)?, &args),
        Output::TabNames => json(&text)?
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| t["name"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Output::CurrentTab { json: true } => text,
        Output::CurrentTab { json: false } => format_tabs(&Value::Array(vec![json(&text)?]), &ListTabsArgs::default()),
        Output::Panes(args) if args.json => text,
        Output::Panes(args) => format_panes(&json(&text)?, &args),
        Output::Clients => format_clients(&json(&text)?),
        Output::Screen(Some(path)) => {
            std::fs::write(&path, format!("{text}\n")).with_context(|| format!("writing {}", path.display()))?;
            String::new()
        }
        Output::Screen(None) => text,
    };
    if !out.is_empty() {
        println!("{out}");
    }
    Ok(())
}

/// Runs a pane-opening command (`tb run`, `tb edit`) and prints the new pane's id.
pub fn send_plain(session: &str, action: CliAction) -> anyhow::Result<()> {
    send(session, action, Output::Plain)
}

/// Prints `tb list-panes` (the top-level command): zellij's columns, or JSON.
pub fn list_panes(session: &str, args: ListPanesArgs) -> anyhow::Result<()> {
    send(session, CliAction::ListPanes, Output::Panes(args))
}

/// Prints `tb dump-screen` (the top-level command).
pub fn dump_screen(session: &str, args: DumpScreenArgs) -> anyhow::Result<()> {
    let path = args.path.map(absolute).transpose()?;
    send(
        session,
        CliAction::DumpScreen { pane: args.p.pane_id, full: args.full, ansi: args.ansi },
        Output::Screen(path),
    )
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => "-".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Rows of cells joined by two spaces, as zellij prints them.
fn table(header: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut out = vec![header.join("  ")];
    out.extend(rows.into_iter().map(|r| r.join("  ")));
    out.join("\n")
}

fn format_tabs(tabs: &Value, a: &ListTabsArgs) -> String {
    let mut header = vec!["TAB_ID", "POSITION", "NAME"];
    let (state, panes, dims, layout) = (a.all || a.state, a.all || a.panes, a.all || a.dimensions, a.all || a.layout);
    if state {
        header.extend(["ACTIVE", "FULLSCREEN", "SYNC"]);
    }
    if panes {
        header.push("PANES");
    }
    if dims {
        header.extend(["COLUMNS", "ROWS"]);
    }
    if layout {
        header.push("SWAP_LAYOUT");
    }
    let rows = tabs
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let mut r = vec![cell(&t["tab_id"]), cell(&t["position"]), cell(&t["name"])];
            if state {
                r.extend([cell(&t["active"]), cell(&t["is_fullscreen_active"]), cell(&t["is_sync_panes_active"])]);
            }
            if panes {
                r.push(cell(&t["selectable_tiled_panes_count"]));
            }
            if dims {
                r.extend([cell(&t["viewport_columns"]), cell(&t["viewport_rows"])]);
            }
            if layout {
                r.push(cell(&t["active_swap_layout_name"]));
            }
            r
        })
        .collect();
    table(&header, rows)
}

fn pane_label(p: &Value) -> String {
    format!("{}_{}", p["kind"].as_str().unwrap_or("terminal"), cell(&p["id"]))
}

fn format_panes(panes: &Value, a: &ListPanesArgs) -> String {
    let (tab, command, state, geometry) = (a.all || a.tab, a.all || a.command, a.all || a.state, a.all || a.geometry);
    let mut header = Vec::new();
    if tab {
        header.extend(["TAB_ID", "TAB_POS", "TAB_NAME"]);
    }
    header.extend(["PANE_ID", "TYPE", "TITLE"]);
    if command {
        header.extend(["COMMAND", "CWD"]);
    }
    if state {
        header.extend(["FOCUSED", "FLOATING", "EXITED"]);
    }
    if geometry {
        header.extend(["X", "Y", "COLUMNS", "ROWS"]);
    }
    let rows = panes
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| {
            let mut r = Vec::new();
            if tab {
                r.extend([cell(&p["tab_id"]), cell(&p["tab_position"]), cell(&p["tab_name"])]);
            }
            r.extend([pane_label(p), cell(&p["kind"]), cell(&p["title"])]);
            if command {
                r.extend([cell(&p["pane_command"]), cell(&p["pane_cwd"])]);
            }
            if state {
                r.extend([cell(&p["is_focused"]), cell(&p["is_floating"]), cell(&p["exited"])]);
            }
            if geometry {
                r.extend([cell(&p["pane_x"]), cell(&p["pane_y"]), cell(&p["pane_columns"]), cell(&p["pane_rows"])]);
            }
            r
        })
        .collect();
    table(&header, rows)
}

fn format_clients(clients: &Value) -> String {
    let rows = clients
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let pane = if c["pane_id"].is_null() { "-".into() } else { format!("terminal_{}", cell(&c["pane_id"])) };
            vec![cell(&c["client_id"]), pane, cell(&c["running_command"])]
        })
        .collect();
    table(&["CLIENT_ID", "PANE_ID", "RUNNING_COMMAND"], rows)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn pane_ids_take_zellij_forms() {
        assert_eq!(pane_id("3"), Ok(3));
        assert_eq!(pane_id("terminal_3"), Ok(3));
        assert_eq!(pane_id("finder_4"), Ok(4));
        assert_eq!(pane_id("preview_5"), Ok(5));
        assert!(pane_id("plugin_1").is_err());
        assert!(pane_id("x").is_err());
    }

    #[test]
    fn resize_takes_both_forms() {
        assert!(matches!(
            resize("increase", Some("left"), None).unwrap(),
            CliAction::Resize { side: Some(Direction::Left), grow: true, .. }
        ));
        assert!(matches!(resize("-", None, None).unwrap(), CliAction::Resize { side: None, grow: false, .. }));
        assert!(matches!(
            resize("left", Some("4"), Some(2)).unwrap(),
            CliAction::MoveBorder { dir: Direction::Left, amount: 4, pane: Some(2) }
        ));
        assert!(resize("sideways", None, None).is_err());
    }

    #[test]
    fn lists_print_zellij_columns() {
        let tabs = json!([{"tab_id": 0, "position": 0, "name": "Tab #1"}, {"tab_id": 1, "position": 1, "name": "two"}]);
        assert_eq!(format_tabs(&tabs, &ListTabsArgs::default()), "TAB_ID  POSITION  NAME\n0  0  Tab #1\n1  1  two");
        let panes = json!([{"id": 0, "kind": "terminal", "title": "bash", "tab_id": 0, "tab_position": 0,
            "tab_name": "Tab #1", "pane_command": "/bin/bash", "pane_cwd": "/tmp", "is_focused": true,
            "is_floating": false, "exited": false}]);
        assert_eq!(format_panes(&panes, &ListPanesArgs::default()), "PANE_ID  TYPE  TITLE\nterminal_0  terminal  bash");
        let all = ListPanesArgs { tab: true, command: true, state: true, ..ListPanesArgs::default() };
        assert_eq!(
            format_panes(&panes, &all),
            "TAB_ID  TAB_POS  TAB_NAME  PANE_ID  TYPE  TITLE  COMMAND  CWD  FOCUSED  FLOATING  EXITED\n\
             0  0  Tab #1  terminal_0  terminal  bash  /bin/bash  /tmp  true  false  false"
        );
    }
}
