//! Renders `docs/cli/`: a Markdown page for each `tb` command, from the same definitions as `tb … --help`.
//! The test `cli_docs_are_current` (in main.rs) keeps the pages in step with the code; see
//! docs/plans/2026-11-05-cli-docs.md.

use std::fmt::Write as _;

use clap::{Arg, Command};

const HEADER: &str = "<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run\n     \
                      TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->\n";

/// Every page: (path under docs/cli, Markdown).
pub fn render(mut root: Command) -> Vec<(String, String)> {
    root.build();
    let mut pages = Vec::new();
    let mut index = format!(
        "{HEADER}\n# `tb` commands\n\nOne page per command, generated from `tb … --help`. For what spans commands \
         (ids, JSON fields, events, key bindings, stability), see [the overview](../cli.md).\n\n"
    );
    let usage = root.render_usage().to_string();
    let usage = usage.trim().strip_prefix("Usage: ").unwrap_or(usage.trim()).to_owned();
    let _ = writeln!(index, "```\n{usage}\n```\n\nPlain `tb` attaches to (or starts) the session.\n");
    index.push_str(&options_table(&root));
    index.push_str("## Commands\n\n| Command | Does |\n|---|---|\n");
    let mut actions = String::from("| Action | Does |\n|---|---|\n");
    for cmd in visible(&root) {
        let name = cmd.get_name();
        index.push_str(&format!("| [`tb {name}`]({name}.md){} | {} |\n", aliases(cmd), about(cmd)));
        pages.push((format!("{name}.md"), page(cmd, "")));
        for sub in visible(cmd) {
            let sub_name = sub.get_name();
            actions.push_str(&format!("| [`{sub_name}`]({name}/{sub_name}.md){} | {} |\n", aliases(sub), about(sub)));
            pages.push((format!("{name}/{sub_name}.md"), page(sub, "../")));
        }
    }
    index.push_str("\n## `tb action …`\n\nzellij's actions, with zellij's names and flags, and `tb`'s own.\n\n");
    index.push_str(&actions);
    pages.push(("README.md".into(), index));
    pages
}

fn visible(cmd: &Command) -> impl Iterator<Item = &Command> {
    cmd.get_subcommands().filter(|c| !c.is_hide_set() && c.get_name() != "help")
}

fn about(cmd: &Command) -> String {
    cmd.get_about().map(|a| a.to_string()).unwrap_or_default().replace('|', "\\|")
}

fn aliases(cmd: &Command) -> String {
    let a: Vec<String> = cmd.get_visible_aliases().map(|a| format!("`{a}`")).collect();
    if a.is_empty() { String::new() } else { format!(" ({})", a.join(", ")) }
}

/// How an argument is written: `-d, --direction <DIRECTION>`, `<NAME>`, `[COMMAND]...`.
fn arg_form(arg: &Arg) -> String {
    let values: Vec<String> = arg
        .get_value_names()
        .map(|v| v.iter().map(|n| n.to_string()).collect())
        .unwrap_or_else(|| vec![arg.get_id().to_string().to_uppercase()]);
    let takes_value = arg.get_num_args().is_some_and(|n| n.takes_values());
    if arg.is_positional() {
        let v = values.join(" ");
        let many = arg.get_num_args().is_some_and(|n| n.max_values() > 1);
        let dots = if many { "..." } else { "" };
        return if arg.is_required_set() { format!("<{v}>{dots}") } else { format!("[{v}]{dots}") };
    }
    let mut s = String::new();
    if let Some(c) = arg.get_short() {
        let _ = write!(s, "-{c}, ");
    }
    if let Some(l) = arg.get_long() {
        let _ = write!(s, "--{l}");
    }
    if takes_value {
        let optional = arg.get_num_args().is_some_and(|n| n.min_values() == 0);
        let v = values.join(" ");
        let _ = if optional { write!(s, " [<{v}>]") } else { write!(s, " <{v}>") };
    }
    s
}

/// The help for an argument, with its allowed values, default and aliases.
fn arg_help(arg: &Arg) -> String {
    let mut help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
    if !help.is_empty() && !help.ends_with(['.', '?', '!']) {
        help.push('.');
    }
    let mut parts = vec![help];
    let values: Vec<String> =
        arg.get_possible_values().iter().filter(|v| !v.is_hide_set()).map(|v| format!("`{}`", v.get_name())).collect();
    if !values.is_empty() {
        parts.push(format!("Values: {}.", values.join(", ")));
    }
    // On/off flags default to off; saying so is noise.
    let takes_value = arg.get_num_args().is_some_and(|n| n.takes_values());
    let defaults: Vec<String> = if takes_value {
        arg.get_default_values().iter().map(|d| format!("`{}`", d.to_string_lossy())).collect()
    } else {
        Vec::new()
    };
    if !defaults.is_empty() {
        parts.push(format!("Default: {}.", defaults.join(", ")));
    }
    if let Some(a) = arg.get_visible_aliases().filter(|a| !a.is_empty()) {
        parts.push(format!("Also `--{}`.", a.join("`, `--")));
    }
    parts.retain(|p| !p.is_empty());
    parts.join(" ").replace('|', "\\|").replace('\n', " ")
}

fn page(cmd: &Command, up: &str) -> String {
    let full = cmd.get_bin_name().unwrap_or(cmd.get_name()).to_owned();
    let mut out = format!("{HEADER}\n# `{full}`\n\n");
    if let Some(about) = cmd.get_long_about().or(cmd.get_about()) {
        let _ = writeln!(out, "{about}\n");
    }
    let usage = cmd.clone().render_usage().to_string();
    let usage = usage.trim().strip_prefix("Usage: ").unwrap_or(usage.trim());
    let _ = writeln!(out, "```\n{usage}\n```\n");
    let names: Vec<&str> = cmd.get_visible_aliases().collect();
    if !names.is_empty() {
        let _ = writeln!(out, "Also: {}.\n", names.iter().map(|a| format!("`{a}`")).collect::<Vec<_>>().join(", "));
    }
    let args: Vec<&Arg> = cmd
        .get_arguments()
        .filter(|a| !a.is_hide_set() && !matches!(a.get_id().as_str(), "help" | "version"))
        .collect();
    let positionals: Vec<&&Arg> = args.iter().filter(|a| a.is_positional()).collect();
    let options: Vec<&&Arg> = args.iter().filter(|a| !a.is_positional()).collect();
    if !positionals.is_empty() {
        out.push_str("## Arguments\n\n| Argument | |\n|---|---|\n");
        for a in positionals {
            let _ = writeln!(out, "| `{}` | {} |", arg_form(a), arg_help(a));
        }
        out.push('\n');
    }
    if !options.is_empty() {
        out.push_str(&options_table(cmd));
    }
    let subs: Vec<&Command> = visible(cmd).collect();
    if !subs.is_empty() {
        out.push_str("## Commands\n\n| Command | Does |\n|---|---|\n");
        for s in subs {
            let _ = writeln!(
                out,
                "| [`{}`]({}/{}.md){} | {} |",
                s.get_name(),
                cmd.get_name(),
                s.get_name(),
                aliases(s),
                about(s)
            );
        }
        out.push('\n');
    }
    if let Some(more) = cmd.get_after_long_help() {
        let more = more.to_string();
        let examples = more.trim().strip_prefix("Examples:").unwrap_or(more.trim());
        let lines: Vec<&str> = examples.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let _ = writeln!(out, "## Examples\n\n```sh\n{}\n```\n", lines.join("\n"));
    }
    let _ = writeln!(out, "See also: [all commands]({up}README.md) · [the overview]({up}../cli.md)");
    out
}

/// The options of `cmd`, as a Markdown table under "## Options".
fn options_table(cmd: &Command) -> String {
    let mut out = String::from("## Options\n\n| Option | |\n|---|---|\n");
    for a in cmd
        .get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set() && !matches!(a.get_id().as_str(), "help" | "version"))
    {
        let _ = writeln!(out, "| `{}` | {} |", arg_form(a), arg_help(a));
    }
    out.push('\n');
    out
}
