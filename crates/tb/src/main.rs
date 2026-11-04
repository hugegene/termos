//! `tb`: a terminal workspace with persistent sessions, tabs, split panes and a built-in file finder.
//! ("tb" is a working name until the product name is chosen.)

mod client;
mod logger;
mod paths;
mod protocol;
mod server;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::bail;
use clap::{Parser, Subcommand, ValueEnum};

use protocol::{Command, Direction, FocusTarget, SplitDir};

#[derive(Parser)]
#[command(name = "tb", version, about = "Terminal workspace: persistent sessions, tabs, split panes and a file finder")]
struct Cli {
    /// Session name (default: the session you're in, else "main")
    #[arg(short, long, global = true)]
    session: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Attach to the session, starting it if needed (the default)
    Attach,
    /// Start the session without attaching
    Start,
    /// List running sessions
    Ls,
    /// End the session and every program in it
    Kill,
    /// Open a finder tab on DIR (default: the current folder)
    Finder { dir: Option<PathBuf> },
    /// Change the session from scripts: tabs, splits, focus and more
    Action {
        #[command(subcommand)]
        action: ActionCmd,
    },
    /// List the session's panes
    ListPanes {
        /// Print JSON
        #[arg(short, long)]
        json: bool,
    },
    /// Print a pane's text (default: the focused pane)
    DumpScreen {
        #[arg(short, long)]
        pane: Option<u32>,
        /// Include the scrollback
        #[arg(long)]
        full: bool,
    },
    #[command(name = "__server", hide = true)]
    Server { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum SplitArg {
    Right,
    Down,
}

#[derive(Clone, Copy, ValueEnum)]
enum DirArg {
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

#[derive(Subcommand)]
enum ActionCmd {
    /// Open a new tab running a shell, or COMMAND after `--`
    NewTab {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Split the focused pane
    Split {
        #[arg(value_enum)]
        dir: SplitArg,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// Focus a pane: left, right, up, down, next, prev, or a pane id
    Focus { target: String },
    /// Close a pane (default: the focused one)
    Close {
        #[arg(short, long)]
        pane: Option<u32>,
    },
    /// Zoom or unzoom the focused pane
    Zoom,
    /// Move the focused pane's nearest border
    Resize {
        #[arg(value_enum)]
        dir: DirArg,
        #[arg(default_value_t = 2)]
        amount: u16,
    },
    /// Go to tab N (1-based)
    GoToTab { index: usize },
    /// Rename the current tab
    RenameTab { name: String },
    /// Type text into a pane (default: the focused one)
    Write {
        #[arg(short, long)]
        pane: Option<u32>,
        text: String,
        /// Press Enter after the text
        #[arg(long)]
        enter: bool,
    },
    /// Detach every other client
    Detach,
}

fn session_name(cli: &Cli) -> anyhow::Result<String> {
    let name = cli.session.clone().or_else(|| std::env::var("TB_SESSION").ok()).unwrap_or_else(|| "main".into());
    if !paths::valid_session_name(&name) {
        bail!("session names may use letters, digits, '-', '_' and '.' (got '{name}')");
    }
    Ok(name)
}

fn absolute(p: PathBuf) -> anyhow::Result<PathBuf> {
    Ok(if p.is_absolute() { p } else { std::env::current_dir()?.join(p) })
}

fn print(text: &str) {
    if !text.is_empty() {
        println!("{text}");
    }
}

fn action(a: ActionCmd) -> anyhow::Result<Command> {
    Ok(match a {
        ActionCmd::NewTab { name, cwd, command } => {
            Command::NewTab { name, cwd: cwd.map(absolute).transpose()?, command }
        }
        ActionCmd::Split { dir, cwd, command } => Command::Split {
            dir: match dir {
                SplitArg::Right => SplitDir::Right,
                SplitArg::Down => SplitDir::Down,
            },
            cwd: cwd.map(absolute).transpose()?,
            command,
        },
        ActionCmd::Focus { target } => Command::Focus(match target.as_str() {
            "left" => FocusTarget::Dir(Direction::Left),
            "right" => FocusTarget::Dir(Direction::Right),
            "up" => FocusTarget::Dir(Direction::Up),
            "down" => FocusTarget::Dir(Direction::Down),
            "next" => FocusTarget::Next,
            "prev" => FocusTarget::Prev,
            id => match id.parse() {
                Ok(id) => FocusTarget::Pane(id),
                Err(_) => bail!("focus target must be left, right, up, down, next, prev or a pane id"),
            },
        }),
        ActionCmd::Close { pane } => Command::Close { pane },
        ActionCmd::Zoom => Command::Zoom,
        ActionCmd::Resize { dir, amount } => Command::Resize { dir: dir.into(), amount },
        ActionCmd::GoToTab { index } => Command::GoToTab(index),
        ActionCmd::RenameTab { name } => Command::RenameTab(name),
        ActionCmd::Write { pane, text, enter } => Command::Write { pane, text: if enter { text + "\r" } else { text } },
        ActionCmd::Detach => Command::DetachAll,
    })
}

fn format_panes(json: &str) -> anyhow::Result<String> {
    let panes: Vec<serde_json::Value> = serde_json::from_str(json)?;
    let mut out =
        vec![format!("{:>4}  {:>3}  {:<8}  {:>7}  {:<5}  {}", "PANE", "TAB", "KIND", "SIZE", "FOCUS", "TITLE")];
    for p in panes {
        out.push(format!(
            "{:>4}  {:>3}  {:<8}  {:>7}  {:<5}  {}",
            p["id"].to_string(),
            p["tab"].to_string(),
            p["kind"].as_str().unwrap_or(""),
            format!("{}x{}", p["cols"], p["rows"]),
            if p["focused"].as_bool() == Some(true) { "*" } else { "" },
            p["title"].as_str().unwrap_or(""),
        ));
    }
    Ok(out.join("\n"))
}

fn run(cli: Cli) -> anyhow::Result<()> {
    if let Some(Cmd::Server { name }) = &cli.cmd {
        // The server's stderr goes nowhere, so failures also go to its log.
        let result = server::run(name);
        if let Err(e) = &result {
            log::error!("{e:#}");
        }
        return result;
    }
    let session = session_name(&cli)?;
    match cli.cmd.unwrap_or(Cmd::Attach) {
        Cmd::Attach => client::attach(&session)?,
        Cmd::Start => {
            if client::ensure_server(&session)? {
                client::command(&session, Command::NewTab { name: None, cwd: None, command: Vec::new() })?;
            }
        }
        Cmd::Ls => client::list_sessions()?,
        Cmd::Kill => {
            client::command(&session, Command::Kill)?;
        }
        Cmd::Finder { dir } => {
            let dir = absolute(dir.unwrap_or(std::env::current_dir()?))?;
            let inside = std::env::var("TB_SESSION").is_ok_and(|s| s == session);
            client::ensure_server(&session)?;
            client::command(&session, Command::Finder { dir: Some(dir) })?;
            if !inside {
                client::attach(&session)?;
            }
        }
        Cmd::Action { action: a } => print(&client::command(&session, action(a)?)?),
        Cmd::ListPanes { json } => {
            let text = client::command(&session, Command::ListPanes)?;
            print(&if json { text } else { format_panes(&text)? });
        }
        Cmd::DumpScreen { pane, full } => print(&client::command(&session, Command::DumpScreen { pane, full })?),
        Cmd::Server { .. } => unreachable!(),
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tb: {e:#}");
            ExitCode::FAILURE
        }
    }
}
