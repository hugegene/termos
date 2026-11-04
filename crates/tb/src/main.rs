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
use clap::{Args, Parser, Subcommand, ValueEnum};

use protocol::{Command, Direction, FocusTarget, SplitDir};

#[derive(Parser)]
#[command(name = "tb", version, about = "Terminal workspace: persistent sessions, tabs, split panes and a file finder")]
struct Cli {
    /// Session name (default: the session you're in, else "main")
    #[arg(short, long)]
    session: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

/// `-s NAME` on each command that works on one session. (A single global `-s` would clash with `ls -s`.)
#[derive(Args, Default)]
struct SessionOpt {
    /// Session name (default: the session you're in, else "main")
    #[arg(short, long, global = true)]
    session: Option<String>,
}

/// zellij's `attach`, with its flags.
#[derive(Args, Default)]
struct AttachArgs {
    /// Session name (default: the session you're in, else "main")
    name: Option<String>,
    #[command(flatten)]
    s: SessionOpt,
    /// Create the session if it doesn't exist (always the case in tb; accepted for zellij's habit)
    #[arg(short, long)]
    #[allow(dead_code)]
    create: bool,
    /// Start the session in the background without attaching
    #[arg(short = 'b', long)]
    create_background: bool,
    /// Forget the saved layout first, so the session starts fresh
    #[arg(long)]
    forget: bool,
    /// When restoring, run the commands that were running instead of waiting for Enter
    #[arg(short, long)]
    force_run_commands: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Attach to a session, starting it if needed (the default)
    #[command(visible_alias = "a")]
    Attach(AttachArgs),
    /// Start a session without attaching (same as `attach -b`)
    Start {
        name: Option<String>,
        #[command(flatten)]
        s: SessionOpt,
    },
    /// List sessions, including exited ones that attaching restores
    #[command(visible_alias = "list-sessions")]
    Ls {
        /// Print just the session names
        #[arg(short, long)]
        short: bool,
        /// List in reverse order
        #[arg(short, long)]
        reverse: bool,
        /// No colours or formatting (tb prints none; accepted for zellij's habit)
        #[arg(short, long)]
        #[allow(dead_code)]
        no_formatting: bool,
    },
    /// End a session and every program in it (attach again to restore its layout)
    #[command(visible_aliases = ["kill-session", "k"])]
    Kill {
        name: Option<String>,
        #[command(flatten)]
        s: SessionOpt,
    },
    /// End every running session (attach again to restore their layouts)
    #[command(visible_aliases = ["kill-all-sessions", "ka"])]
    KillAll {
        /// Don't ask first
        #[arg(short, long)]
        yes: bool,
    },
    /// Forget a saved session so it isn't restored (default: the current session name)
    #[command(visible_aliases = ["delete-session", "d"])]
    Delete {
        name: Option<String>,
        #[command(flatten)]
        s: SessionOpt,
        /// Also end it if it's running
        #[arg(short, long)]
        force: bool,
    },
    /// Forget every exited session
    #[command(visible_aliases = ["delete-all-sessions", "da"])]
    DeleteAll {
        /// Don't ask first
        #[arg(short, long)]
        yes: bool,
        /// Also end and forget the running sessions
        #[arg(short, long)]
        force: bool,
    },
    /// Open a finder tab on DIR (default: the current folder)
    Finder {
        dir: Option<PathBuf>,
        #[command(flatten)]
        s: SessionOpt,
    },
    /// Change the session from scripts: tabs, splits, focus and more
    #[command(visible_alias = "ac")]
    Action {
        #[command(flatten)]
        s: SessionOpt,
        #[command(subcommand)]
        action: ActionCmd,
    },
    /// List the session's panes
    ListPanes {
        #[command(flatten)]
        s: SessionOpt,
        /// Print JSON
        #[arg(short, long)]
        json: bool,
    },
    /// Print a pane's text (default: the focused pane)
    DumpScreen {
        #[command(flatten)]
        s: SessionOpt,
        #[arg(short, long)]
        pane: Option<u32>,
        /// Include the scrollback
        #[arg(long)]
        full: bool,
    },
    #[command(name = "__server", hide = true)]
    Server {
        name: String,
        #[arg(long)]
        run_commands: bool,
    },
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
    #[command(visible_alias = "move-focus")]
    Focus { target: String },
    /// Close a pane (default: the focused one)
    #[command(visible_alias = "close-pane")]
    Close {
        #[arg(short, long)]
        pane: Option<u32>,
    },
    /// Zoom or unzoom the focused pane
    #[command(visible_alias = "toggle-fullscreen")]
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
    #[command(visible_alias = "write-chars")]
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

/// The session a command works on: a name given after the command, then `-s` after the command, then `-s`
/// before it, then the session you're in, else "main".
fn pick_session(given: [Option<String>; 3], inside: Option<String>) -> anyhow::Result<String> {
    let name = given.into_iter().flatten().next().or(inside).unwrap_or_else(|| "main".into());
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
    let Cli { session: top, cmd } = cli;
    let session = |name: Option<String>, s: SessionOpt| {
        pick_session([name, s.session, top.clone()], std::env::var("TB_SESSION").ok())
    };
    match cmd.unwrap_or(Cmd::Attach(AttachArgs::default())) {
        Cmd::Server { name, run_commands } => {
            // The server's stderr goes nowhere, so failures also go to its log.
            let result = server::run(&name, run_commands);
            if let Err(e) = &result {
                log::error!("{e:#}");
            }
            return result;
        }
        Cmd::Attach(a) => {
            let session = session(a.name, a.s)?;
            if !a.create_background {
                client::ensure_outside()?;
            }
            if a.forget {
                client::forget_saved(&session)?;
            }
            if a.create_background {
                start(&session, a.force_run_commands)?;
            } else {
                client::attach(&session, a.force_run_commands)?;
            }
        }
        Cmd::Start { name, s } => start(&session(name, s)?, false)?,
        Cmd::Ls { short, reverse, .. } => client::list_sessions(short, reverse)?,
        Cmd::Kill { name, s } => client::kill_session(&session(name, s)?, false)?,
        Cmd::KillAll { yes } => client::kill_all_sessions(yes)?,
        Cmd::Delete { name, s, force } => client::delete_session(&session(name, s)?, force)?,
        Cmd::DeleteAll { yes, force } => client::delete_all_sessions(yes, force)?,
        Cmd::Finder { dir, s } => {
            let session = session(None, s)?;
            let dir = absolute(dir.unwrap_or(std::env::current_dir()?))?;
            let inside = std::env::var("TB_SESSION").is_ok_and(|s| s == session);
            client::ensure_server(&session, false)?;
            client::command(&session, Command::Finder { dir: Some(dir) })?;
            if !inside {
                client::attach(&session, false)?;
            }
        }
        Cmd::Action { s, action: a } => print(&client::command(&session(None, s)?, action(a)?)?),
        Cmd::ListPanes { s, json } => {
            let text = client::command(&session(None, s)?, Command::ListPanes)?;
            print(&if json { text } else { format_panes(&text)? });
        }
        Cmd::DumpScreen { s, pane, full } => {
            print(&client::command(&session(None, s)?, Command::DumpScreen { pane, full })?)
        }
    }
    Ok(())
}

/// Starts a session without attaching.
fn start(session: &str, run_commands: bool) -> anyhow::Result<()> {
    client::ensure_server(session, run_commands)?;
    // A restored session already has tabs.
    client::command(session, Command::EnsureTab)?;
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

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("tb").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn the_command_line_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn zellij_session_commands_parse() {
        let attach = |args: &[&str]| match parse(args).cmd {
            Some(Cmd::Attach(a)) => a,
            _ => panic!("{args:?} isn't attach"),
        };
        assert_eq!(attach(&["attach", "work"]).name.as_deref(), Some("work"));
        assert_eq!(attach(&["a", "-c", "work"]).name.as_deref(), Some("work"));
        assert_eq!(attach(&["attach", "-s", "work"]).s.session.as_deref(), Some("work"));
        let a = attach(&["attach", "-b", "--forget", "-f", "work"]);
        assert!(a.create_background && a.forget && a.force_run_commands);
        assert!(matches!(parse(&["ls", "-s", "-r", "-n"]).cmd, Some(Cmd::Ls { short: true, reverse: true, .. })));
        let cli = parse(&["-s", "x", "list-sessions", "-s"]);
        assert_eq!(cli.session.as_deref(), Some("x"));
        assert!(matches!(cli.cmd, Some(Cmd::Ls { short: true, .. })));
        assert!(matches!(parse(&["k", "work"]).cmd, Some(Cmd::Kill { name: Some(n), .. }) if n == "work"));
        assert!(matches!(parse(&["kill-session", "work"]).cmd, Some(Cmd::Kill { name: Some(n), .. }) if n == "work"));
        assert!(matches!(parse(&["d", "-f", "work"]).cmd, Some(Cmd::Delete { force: true, .. })));
        assert!(matches!(parse(&["ka", "-y"]).cmd, Some(Cmd::KillAll { yes: true })));
        assert!(matches!(parse(&["da", "-y", "-f"]).cmd, Some(Cmd::DeleteAll { yes: true, force: true })));
        assert!(matches!(parse(&["kill-all-sessions"]).cmd, Some(Cmd::KillAll { yes: false })));
        assert!(matches!(parse(&["delete-all-sessions"]).cmd, Some(Cmd::DeleteAll { .. })));
        // `-s` works before and after `action`'s own subcommand.
        for args in [&["ac", "-s", "w", "go-to-tab", "2"][..], &["action", "go-to-tab", "2", "-s", "w"]] {
            match parse(args).cmd {
                Some(Cmd::Action { s, .. }) => assert_eq!(s.session.as_deref(), Some("w"), "{args:?}"),
                _ => panic!("{args:?} isn't action"),
            }
        }
    }

    #[test]
    fn the_session_is_picked_in_order() {
        let some = |s: &str| Some(s.to_owned());
        assert_eq!(pick_session([some("a"), some("b"), some("c")], some("d")).unwrap(), "a");
        assert_eq!(pick_session([None, some("b"), some("c")], some("d")).unwrap(), "b");
        assert_eq!(pick_session([None, None, some("c")], some("d")).unwrap(), "c");
        assert_eq!(pick_session([None, None, None], some("d")).unwrap(), "d");
        assert_eq!(pick_session([None, None, None], None).unwrap(), "main");
        assert!(pick_session([some("../x"), None, None], None).is_err());
    }
}
