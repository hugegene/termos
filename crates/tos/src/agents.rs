//! `tos claude`, `tos codex` and the other agent launchers, and `tos hook`, which their hooks run.
//!
//! A launcher starts the agent with two additions for that session only, and writes nothing to the agent's own
//! files: a hook that runs `tos hook` after the agent writes a file, and a few lines added to its system prompt.
//! `tos hook` reads the tool call the agent hands it, and sends each Markdown file written inside the agent's
//! working folder to the docs pane of the agent's tab (`tos action add-doc`).
//! Plan: docs/plans/2026-11-17-agent-plan-pane.md; flags: docs/findings/2026-11-21-agent-launch-flags.md.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{Context, bail};
use serde_json::{Value, json};

use crate::client;
use crate::paths;
use crate::protocol::{CliAction, Command};

/// The agents `tos` can start.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Agent {
    /// Claude Code (`claude`)
    Claude,
    /// OpenAI Codex CLI (`codex`)
    Codex,
    /// Google Antigravity CLI (`agy`)
    Agy,
    /// Cursor CLI (`agent`)
    #[value(name = "agent")]
    Cursor,
    /// xAI Grok Build (`grok`)
    Grok,
    /// Gemini CLI (`gemini`)
    Gemini,
}

impl Agent {
    /// The agent's own command.
    pub fn program(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::Agy => "agy",
            Agent::Cursor => "agent",
            Agent::Grok => "grok",
            Agent::Gemini => "gemini",
        }
    }
}

/// What the agent is told at the start, unless `agent-prompt.md` says otherwise: where it runs, then how to explain
/// (the user's rules, 2026-11-23).
pub const DEFAULT_PROMPT: &str = "You are running inside tos, a terminal workspace. Markdown (.md) files that you \
create or edit inside the current working folder are shown to the user in a pane beside this session, so write \
plans, notes and reports as .md files in this folder when that helps the user follow your work.

When you explain something, in your replies and in the files you write:
- Draw relationships as plain-text (ASCII) diagrams in fenced code blocks: a box for each part (such as a \
process, module, file, service or machine), and numbered arrows labelled with what travels for each step of a \
request or data flow.
- Explain by walking through one concrete example step by step, such as a real file, command, input or \
request, rather than only stating the general rule.
- Don't use short forms to refer to things: no abbreviations or names you coined, and no bare references such \
as \"option B\", \"step 3\" or \"the fix above\" without saying what they are. Name things in full.
- Keep each reply self-contained: someone who reads only that reply should understand it without the earlier \
conversation or other files, so restate the context and the names it relies on.";

/// Folders of the agents' own state. Files there never show, even inside the working folder: Claude's memory
/// lives in `~/.claude/projects/<project>/memory/`, for example.
const AGENT_DIRS: [&str; 5] = [".claude", ".codex", ".gemini", ".cursor", ".grok"];

/// The starting instructions: `agent-prompt.md` beside `config.kdl` if it exists (empty means none), else the
/// default.
fn prompt() -> Option<String> {
    match fs::read_to_string(paths::config_dir().join("agent-prompt.md")) {
        Ok(text) => Some(text.trim().to_owned()).filter(|t| !t.is_empty()),
        Err(_) => Some(DEFAULT_PROMPT.to_owned()),
    }
}

/// The `tos` that hooks should run: the installer's `~/.local/bin/tos` if that's this program, since it
/// follows `tos update`; else this program's own path.
fn tos_path() -> PathBuf {
    let exe = env::current_exe().unwrap_or_else(|_| PathBuf::from("tos"));
    let link = env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/bin/tos"));
    match link {
        Some(link) if link.canonicalize().ok() == exe.canonicalize().ok() => link,
        _ => exe,
    }
}

/// Quotes a word for `sh -c`, which is how agents run hook commands.
fn sh_quote(word: &str) -> String {
    if !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c)) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// A TOML basic string, for Codex's `-c key=value`.
fn toml_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// How to start an agent: the files to write first, then the program, its arguments and environment.
#[derive(Debug, PartialEq)]
pub struct Launch {
    pub files: Vec<(PathBuf, String)>,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default() + "\n"
}

/// Claude-style hooks: `{"hooks": {EVENT: [{"matcher": M, "hooks": [{"type": "command", "command": C}]}]}}`.
fn claude_style(event: &str, matcher: &str, hook: &str) -> Value {
    json!({ "hooks": { event: [ { "matcher": matcher, "hooks": [ { "type": "command", "command": hook, "timeout": 10 } ] } ] } })
}

/// How to start `agent` with `args`, its hook running `hook`, its files kept in `dir`.
pub fn plan(agent: Agent, args: Vec<String>, prompt: Option<&str>, hook: &str, dir: &Path) -> Launch {
    let mut files = Vec::new();
    let mut env = Vec::new();
    let mut first: Vec<String> = Vec::new();
    let path = |p: &Path| p.to_string_lossy().into_owned();
    match agent {
        Agent::Claude => {
            let settings = dir.join("claude/settings.json");
            files.push((settings.clone(), pretty(&claude_style("PostToolUse", "Write|Edit|MultiEdit", hook))));
            first.extend(["--settings".into(), path(&settings)]);
            if let Some(p) = prompt {
                first.extend(["--append-system-prompt".into(), p.to_owned()]);
            }
        }
        Agent::Codex => {
            // No matcher: every tool. Codex edits files with `apply_patch`, which may come as a shell command, and
            // `tos hook` only acts on .md files anyway.
            let hooks = format!(
                "hooks.PostToolUse=[{{hooks=[{{type=\"command\",command={},timeout=10}}]}}]",
                toml_string(hook)
            );
            first.extend(["-c".into(), hooks]);
            if let Some(p) = prompt {
                first.extend(["-c".into(), format!("developer_instructions={}", toml_string(p))]);
            }
        }
        Agent::Agy => {
            // agy's hooks are named; "*" matches every tool, since its edit tools' names aren't all known.
            let settings = dir.join("agy/settings.json");
            let hooks = json!({ "hooks": { "tos-docs": { "PostToolUse": [
                { "matcher": "*", "hooks": [ { "type": "command", "command": hook, "timeout": 10 } ] } ] } } });
            files.push((settings.clone(), pretty(&hooks)));
            first.extend(["--settings".into(), path(&settings)]);
        }
        Agent::Cursor => {
            let plugin = dir.join("cursor-plugin");
            let manifest = json!({ "name": "tos-docs", "version": "1.0.0",
                "description": "Shows the Markdown files the agent writes in tos's docs pane" });
            files.push((plugin.join(".cursor-plugin/plugin.json"), pretty(&manifest)));
            let hooks = json!({ "version": 1, "hooks": { "afterFileEdit": [ { "command": hook } ] } });
            files.push((plugin.join("hooks/hooks.json"), pretty(&hooks)));
            first.extend(["--plugin-dir".into(), path(&plugin)]);
        }
        Agent::Grok => {
            let plugin = dir.join("grok-plugin");
            let manifest = json!({ "name": "tos-docs", "version": "1.0.0",
                "description": "Shows the Markdown files the agent writes in tos's docs pane" });
            files.push((plugin.join("plugin.json"), pretty(&manifest)));
            files.push((plugin.join("hooks/hooks.json"), pretty(&claude_style("PostToolUse", "Edit|Write", hook))));
            first.extend(["--plugin-dir".into(), path(&plugin)]);
            if let Some(p) = prompt {
                first.extend(["--rules".into(), p.to_owned()]);
            }
        }
        Agent::Gemini => {
            // A system settings file that holds only hooks: Gemini adds hook lists from every settings file to
            // each other, so the user's own hooks still run.
            let settings = dir.join("gemini/settings.json");
            files.push((settings.clone(), pretty(&claude_style("AfterTool", "write_file|replace", hook))));
            env.push(("GEMINI_CLI_SYSTEM_SETTINGS_PATH".into(), path(&settings)));
        }
    }
    first.extend(args);
    Launch { files, program: agent.program().to_owned(), args: first, env }
}

/// `tos claude` and the others: writes the hook's files, then becomes the agent.
pub fn launch(agent: Agent, args: Vec<String>) -> anyhow::Result<()> {
    let hook = format!("{} hook --agent {}", sh_quote(&tos_path().to_string_lossy()), agent.program());
    let dir = paths::state_dir().join("agents");
    let prompt = prompt();
    let launch = plan(agent, args, prompt.as_deref(), &hook, &dir);
    for (path, text) in &launch.files {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("making {}", parent.display()))?;
        }
        // Unchanged text isn't written again, so two launches at once never meet half a file.
        if fs::read_to_string(path).ok().as_deref() != Some(text.as_str()) {
            let tmp = path.with_extension(format!("tmp{}", std::process::id()));
            fs::write(&tmp, text)
                .and_then(|()| fs::rename(&tmp, path))
                .with_context(|| format!("writing {}", path.display()))?;
        }
    }
    let err = std::process::Command::new(&launch.program).args(&launch.args).envs(launch.env).exec();
    if err.kind() == std::io::ErrorKind::NotFound {
        bail!("{} isn't on your PATH", launch.program);
    }
    Err(err).with_context(|| format!("starting {}", launch.program))
}

// ----- tos hook -----

/// The parts of a hook's JSON that hold the tool's input, for each agent's shape: Claude, Codex, Gemini, Grok
/// (`tool_input`), agy (`toolCall.args`), Copilot (`toolArgs`) and Cursor's `afterFileEdit` (`file_path` at the
/// top).
fn tool_inputs(payload: &Value) -> Vec<&Value> {
    let mut out: Vec<&Value> =
        [&payload["tool_input"], &payload["toolInput"], &payload["toolCall"]["args"], &payload["toolArgs"]]
            .into_iter()
            .filter(|v| !v.is_null())
            .collect();
    for key in ["file_path", "filePath", "path"] {
        if payload[key].is_string() {
            out.push(&payload[key]);
        }
    }
    out
}

/// Every file a tool call names as written: string values under keys that mention a path or file, and the
/// files in an `apply_patch` patch (`*** Add File: x`, `*** Update File: x`, `*** Move to: x`).
pub fn named_files(payload: &Value) -> Vec<String> {
    fn walk(v: &Value, key_is_path: bool, out: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                if key_is_path && !s.contains('\n') {
                    out.push(s.clone());
                }
                for line in s.lines() {
                    let line = line.trim_start();
                    for head in ["*** Add File: ", "*** Update File: ", "*** Move to: "] {
                        if let Some(file) = line.strip_prefix(head) {
                            out.push(file.trim().to_owned());
                        }
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|i| walk(i, key_is_path, out)),
            Value::Object(map) => {
                for (k, v) in map {
                    let k = k.to_ascii_lowercase();
                    walk(v, k.contains("path") || k.contains("file"), out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for input in tool_inputs(payload) {
        walk(input, input.is_string(), &mut out);
    }
    out.dedup();
    out
}

/// The file to show for `raw`, a path a tool call named: only an existing Markdown file inside `cwd`, and not
/// inside an agent's own folder under one of `homes`.
pub fn keep(raw: &str, cwd: &Path, homes: &[PathBuf]) -> Option<PathBuf> {
    let p = Path::new(raw);
    let ext = p.extension()?.to_str()?.to_ascii_lowercase();
    if ext != "md" && ext != "markdown" {
        return None;
    }
    let file = cwd.join(p).canonicalize().ok().filter(|f| f.is_file())?;
    let cwd = cwd.canonicalize().ok()?;
    if !file.starts_with(&cwd) {
        return None;
    }
    (!in_agent_dir(&file, homes)).then_some(file)
}

/// Whether `file` is inside an agent's own folder under one of `homes`, such as `~/.claude`.
pub fn in_agent_dir(file: &Path, homes: &[PathBuf]) -> bool {
    homes.iter().any(|h| AGENT_DIRS.iter().any(|d| file.starts_with(h.join(d))))
}

/// `tos hook --agent NAME`: run by an agent after a tool call, with the call as JSON on stdin. Never fails and
/// prints nothing, so it can't get in the agent's way; agy alone expects `{}`.
pub fn hook(agent: Agent) {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let result = hook_inner(&input);
    if env::var_os("TOS_HOOK_LOG").is_some() {
        log_hook(agent, &input, &result);
    }
    if agent == Agent::Agy {
        let _ = std::io::stdout().write_all(b"{}\n");
    }
}

fn hook_inner(input: &str) -> anyhow::Result<Vec<PathBuf>> {
    let (Ok(session), Some(pane)) =
        (env::var("TOS_SESSION"), env::var("TOS_PANE_ID").ok().and_then(|p| p.parse().ok()))
    else {
        return Ok(Vec::new());
    };
    let payload: Value = serde_json::from_str(input).context("reading the hook's JSON")?;
    let cwd = match payload["cwd"].as_str() {
        Some(dir) => PathBuf::from(dir),
        None => env::current_dir()?,
    };
    let mut shown = Vec::new();
    for raw in named_files(&payload) {
        let Some(file) = keep(&raw, &cwd, paths::home_dirs()) else { continue };
        if shown.contains(&file) {
            continue;
        }
        let action = CliAction::AddDoc { pane: Some(pane), path: file.clone(), cwd: Some(cwd.clone()) };
        client::command(&session, Command::Action(action))?;
        shown.push(file);
    }
    Ok(shown)
}

/// With `TOS_HOOK_LOG` set, each call and what came of it goes to `agents/hook.log` in the state folder.
fn log_hook(agent: Agent, input: &str, result: &anyhow::Result<Vec<PathBuf>>) {
    let path = paths::state_dir().join("agents/hook.log");
    let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new("/")));
    let outcome = match result {
        Ok(files) => format!("shown: {files:?}"),
        Err(e) => format!("error: {e:#}"),
    };
    let pane: Option<OsString> = env::var_os("TOS_PANE_ID");
    let line = format!("--- {} pane {:?}\n{}\n{outcome}\n", agent.program(), pane, input.trim());
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(payload: Value) -> Vec<String> {
        named_files(&payload)
    }

    #[test]
    fn each_agents_payload_names_its_file() {
        // Claude Code, Codex, Gemini, Grok: tool_input.file_path.
        let claude =
            json!({"cwd": "/w", "tool_name": "Write", "tool_input": {"file_path": "/w/docs/x.md", "content": "# X\n"}});
        assert_eq!(files(claude), ["/w/docs/x.md"]);
        // agy: camelCase, toolCall.args, with the path's key unknown.
        let agy = json!({"cwd": "/w", "toolCall": {"name": "write_to_file", "args": {"TargetFile": "/w/plan.md", "CodeContent": "x"}}});
        assert_eq!(files(agy), ["/w/plan.md"]);
        // Cursor's afterFileEdit: file_path at the top.
        let cursor = json!({"file_path": "/w/a.md", "edits": [{"old_string": "a", "new_string": "b"}]});
        assert_eq!(files(cursor), ["/w/a.md"]);
        // Kiro, Goose and others: tool_input.path.
        assert_eq!(files(json!({"tool_input": {"path": "n.md"}})), ["n.md"]);
    }

    #[test]
    fn patches_name_their_files() {
        let patch = "*** Begin Patch\n*** Add File: docs/new.md\n+# New\n*** Update File: src/main.rs\n@@\n-a\n+b\n*** End Patch";
        // As apply_patch's own input, and as a shell command's argument list.
        assert_eq!(files(json!({"tool_input": {"input": patch}})), ["docs/new.md", "src/main.rs"]);
        assert_eq!(files(json!({"tool_input": {"command": ["apply_patch", patch]}})), ["docs/new.md", "src/main.rs"]);
        // A plain command names nothing.
        assert!(files(json!({"tool_input": {"command": "cat README.md"}})).is_empty());
    }

    #[test]
    fn only_markdown_inside_the_working_folder_shows() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let work = root.join("work");
        for f in ["work/docs/x.md", "work/notes.MARKDOWN", "work/main.rs", "elsewhere.md", "work/.claude/m.md"] {
            let p = root.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, "x").unwrap();
        }
        let homes = [root.clone()];
        assert_eq!(keep("docs/x.md", &work, &homes), Some(work.join("docs/x.md")));
        assert_eq!(
            keep(&work.join("notes.MARKDOWN").to_string_lossy(), &work, &homes),
            Some(work.join("notes.MARKDOWN"))
        );
        assert_eq!(keep("main.rs", &work, &homes), None);
        assert_eq!(keep("../elsewhere.md", &work, &homes), None);
        assert_eq!(keep("missing.md", &work, &homes), None);
        // An agent's own folder never shows, even when the agent runs in the folder that holds it.
        assert_eq!(keep(".claude/m.md", &work, std::slice::from_ref(&work)), None);
        assert_eq!(keep("work/.claude/m.md", &root, &[]), Some(work.join(".claude/m.md")));
    }

    #[test]
    fn each_agent_gets_its_hook_and_prompt() {
        let dir = Path::new("/s/agents");
        let hook = "/bin/tos hook --agent x";
        let args = vec!["--resume".to_owned()];

        let c = plan(Agent::Claude, args.clone(), Some("Hi"), hook, dir);
        assert_eq!(c.program, "claude");
        assert_eq!(
            c.args,
            ["--settings", "/s/agents/claude/settings.json", "--append-system-prompt", "Hi", "--resume"]
        );
        let settings: Value = serde_json::from_str(&c.files[0].1).unwrap();
        assert_eq!(settings["hooks"]["PostToolUse"][0]["matcher"], "Write|Edit|MultiEdit");
        assert_eq!(settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"], hook);

        let x = plan(Agent::Codex, args.clone(), Some("say \"hi\"\nok"), hook, dir);
        assert_eq!(x.args[0], "-c");
        assert_eq!(
            x.args[1],
            r#"hooks.PostToolUse=[{hooks=[{type="command",command="/bin/tos hook --agent x",timeout=10}]}]"#
        );
        assert_eq!(x.args[3], r#"developer_instructions="say \"hi\"\nok""#);
        assert_eq!(x.args[4], "--resume");
        assert!(x.files.is_empty());

        let g = plan(Agent::Gemini, args.clone(), Some("Hi"), hook, dir);
        assert_eq!(g.args, ["--resume"], "Gemini has no way to add to its prompt");
        assert_eq!(
            g.env,
            [("GEMINI_CLI_SYSTEM_SETTINGS_PATH".to_owned(), "/s/agents/gemini/settings.json".to_owned())]
        );

        let k = plan(Agent::Cursor, args.clone(), None, hook, dir);
        assert_eq!(
            (k.program.as_str(), &k.args[..2]),
            ("agent", &["--plugin-dir".to_owned(), "/s/agents/cursor-plugin".to_owned()][..])
        );
        assert_eq!(k.files.len(), 2);

        let r = plan(Agent::Grok, args.clone(), None, hook, dir);
        assert_eq!(r.args, ["--plugin-dir", "/s/agents/grok-plugin", "--resume"]);

        let a = plan(Agent::Agy, args, Some("Hi"), hook, dir);
        assert_eq!(a.args, ["--settings", "/s/agents/agy/settings.json", "--resume"]);
        let settings: Value = serde_json::from_str(&a.files[0].1).unwrap();
        assert_eq!(settings["hooks"]["tos-docs"]["PostToolUse"][0]["hooks"][0]["command"], hook);
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/home/me/.local/bin/tos"), "/home/me/.local/bin/tos");
        assert_eq!(sh_quote("/My Apps/tos"), "'/My Apps/tos'");
        assert_eq!(sh_quote("it's"), r"'it'\''s'");
        assert_eq!(toml_string("a\\b\t"), r#""a\\b\t""#);
    }
}
