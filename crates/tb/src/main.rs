//! `tb`: a terminal workspace with persistent sessions, tabs, split panes and a built-in file finder.
//! ("tb" is a working name until the product name is chosen.)

mod cli;
mod client;
mod kdl;
mod keyspec;
mod logger;
mod paths;
mod protocol;
mod server;

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::bail;
use clap::{Args, Parser, Subcommand};

use protocol::Command;

#[derive(Parser)]
// The extension API's version is in `--version`. Within a version, commands, flags, JSON fields and events
// are only ever added, never renamed, removed or changed in meaning (docs/cli.md).
#[command(
    name = "tb",
    version = concat!(env!("CARGO_PKG_VERSION"), " (extension API 1)"),
    about = "Terminal workspace: persistent sessions, tabs, split panes and a file finder"
)]
struct Cli {
    /// Session name (default: the session you're in, else "main")
    #[arg(short, long)]
    session: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

/// `-s NAME` on each command that works on one session. It's given before any further subcommand
/// (`tb action -s work new-tab`), because zellij's actions use `-s` for their own flags.
#[derive(Args, Default)]
struct SessionOpt {
    /// Session name (default: the session you're in, else "main")
    #[arg(short, long)]
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
    /// Change the session from scripts: zellij's actions (same names and flags) and tb's own
    #[command(visible_alias = "ac")]
    Action {
        #[command(flatten)]
        s: SessionOpt,
        #[command(subcommand)]
        action: cli::ActionCmd,
    },
    /// Run COMMAND in a new pane (zellij's `run`); prints the pane's id. The session comes from `tb -s`
    #[command(visible_alias = "r")]
    Run(cli::PaneArgs),
    /// Open FILE in the editor in a new pane (zellij's `edit`); prints the pane's id
    #[command(visible_alias = "e")]
    Edit(cli::EditArgs),
    /// Print events as JSON lines until the session ends: tabs and panes opening, closing, renaming and
    /// gaining focus, mode changes, clients. With -p, also those panes' text whenever it changes. The
    /// session comes from `tb -s`
    Subscribe {
        /// Panes whose text to print when it changes (e.g. terminal_1 or 1)
        #[arg(short, long = "pane-id", num_args = 1.., value_parser = cli::pane_id)]
        pane_id: Vec<protocol::PaneId>,
        /// raw: pane text as plain lines; json: every event as a JSON line
        #[arg(short, long, value_enum, default_value = "raw")]
        format: SubscribeFormat,
        /// Include the scrollback in the first delivery: all of it, or the last N lines
        #[arg(short, long, num_args = 0..=1, default_missing_value = "all")]
        scrollback: Option<String>,
        /// Keep colours and styles in pane text
        #[arg(long)]
        ansi: bool,
    },
    /// Show a running session read-only (zellij's `watch`); q or Ctrl+C stops watching
    #[command(visible_alias = "w")]
    Watch {
        /// Session name (default: the session you're in, else "main")
        name: Option<String>,
    },
    /// List the session's panes
    ListPanes {
        #[command(flatten)]
        s: SessionOpt,
        /// Print JSON
        #[arg(short, long)]
        json: bool,
        /// Every column
        #[arg(short, long)]
        all: bool,
    },
    /// Print a pane's text (default: the focused pane)
    DumpScreen {
        #[command(flatten)]
        s: SessionOpt,
        #[command(flatten)]
        a: cli::DumpScreenArgs,
    },
    #[command(name = "__server", hide = true)]
    Server {
        name: String,
        #[arg(long)]
        run_commands: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum SubscribeFormat {
    Raw,
    Json,
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
        Cmd::Action { s, action } => cli::run(&session(None, s)?, action)?,
        Cmd::Run(a) => cli::send_plain(&session(None, SessionOpt::default())?, cli::new_pane(a)?)?,
        Cmd::Edit(a) => cli::send_plain(&session(None, SessionOpt::default())?, cli::edit(a)?)?,
        Cmd::ListPanes { s, json, all } => {
            cli::list_panes(&session(None, s)?, cli::ListPanesArgs { json, all, ..Default::default() })?
        }
        Cmd::DumpScreen { s, a } => cli::dump_screen(&session(None, s)?, a)?,
        Cmd::Watch { name } => client::watch(&session(name, SessionOpt::default())?)?,
        Cmd::Subscribe { pane_id, format, scrollback, ansi } => {
            let scrollback = match scrollback.as_deref() {
                None => None,
                Some("all") => Some(None),
                Some(n) => Some(Some(n.parse().map_err(|_| anyhow::anyhow!("--scrollback takes a number of lines"))?)),
            };
            let json = matches!(format, SubscribeFormat::Json);
            client::subscribe(&session(None, SessionOpt::default())?, pane_id, scrollback, ansi, json)?
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
        // `-s` names the session before `action`'s own subcommand (after it, zellij's flags use `-s`).
        for args in [&["ac", "-s", "w", "go-to-tab", "2"][..], &["-s", "w", "action", "go-to-tab", "2"]] {
            let cli = parse(args);
            match cli.cmd {
                Some(Cmd::Action { s, .. }) => assert_eq!(s.session.or(cli.session).as_deref(), Some("w"), "{args:?}"),
                _ => panic!("{args:?} isn't action"),
            }
        }
    }

    /// docs/cli.md is the extension API's reference: every action must be in it.
    #[test]
    fn every_action_is_documented() {
        let doc = include_str!("../../../docs/cli.md");
        let cmd = Cli::command();
        let action = cmd.find_subcommand("action").expect("an action command");
        for sub in action.get_subcommands().filter(|s| s.get_name() != "help") {
            let name = sub.get_name();
            assert!(doc.contains(&format!("`{name}")), "docs/cli.md doesn't mention `{name}`");
        }
        for top in ["run", "edit", "watch", "subscribe"] {
            assert!(doc.contains(&format!("`tb {top}")), "docs/cli.md doesn't mention `tb {top}`");
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
