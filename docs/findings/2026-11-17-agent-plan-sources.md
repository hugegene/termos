# Finding: where Claude Code, Codex and `agy` keep a plan, and which hooks they offer (2026-11-17)

## Question

The user asked for a side pane in `tos` that shows the plan of the agent session beside it (Claude Code, Codex,
Cursor CLI's `agent`, `agy`), automatically. The [plan](../plans/2026-11-17-agent-plan-pane.md) depends on:

- **whether each agent can run a program when it writes or updates a plan (a hook),** or `tos` has to find
  the plan on its own;
- **what a plan is for each agent:** a Markdown file, or a list of steps inside a tool call;
- **whether `tos` can already show Markdown in a pane, and open a pane beside a given pane without taking
  focus.**

## Method

Dev machine: Ubuntu 24.04, Linux 6.8. `tos` at `b446aec` (0.4.5). The agents, all in `~/.local/bin`:

```
claude --version   → 2.1.326 (Claude Code)
codex --version    → codex-cli 0.160.1
agy --version      → 1.5.9
command -v agent cursor-agent   → nothing (Cursor CLI isn't installed)
```

None of the agents was run for this finding: they need the network, which Claude's shell doesn't have. What
they do was read from their state folders, their transcripts and their binaries:

```sh
# Claude Code: plan files, hooks in use, and which tools the transcripts show
ls ~/.claude/plans; ls ~/.claude
python3 -c "import json; print(json.load(open('$HOME/.claude/settings.json')).get('hooks', {}))"
cat ~/.claude/projects/*/*.jsonl | grep -o '"type":"tool_use","id":"[^"]*","name":"[A-Za-z_]*"' \
  | sed 's/.*"name":"//;s/"//' | sort | uniq -c | sort -rn
cat ~/.claude/projects/*/*.jsonl | grep -o '"name":"\(Write\|Edit\)","input":{"file_path":"[^"]*"' \
  | sed 's/.*"file_path":"//;s/"$//' | grep -i plan | sed 's#/[^/]*$#/#' | sort | uniq -c

# Codex: hooks flag, hook payload fields, update_plan in the 45 newest rollouts
codex features list | grep -i -E 'hook|plan'
strings ~/.local/bin/codex | grep -o -E '"(session_id|transcript_path|hook_event_name|tool_name|tool_input|tool_response|turn_id|cwd)"' | sort | uniq -c
strings ~/.local/bin/codex | grep -o 'hooks\.json\|PostToolUse\|SessionStart\|update_plan' | sort | uniq -c
grep -l update_plan $(find ~/.codex/sessions -name '*.jsonl')
grep -h -o '"name":"[a-z_]*"' $(find ~/.codex/sessions -name '*.jsonl') | sort | uniq -c

# agy: its artifact folder, hooks file, tool names
ls ~/.gemini ~/.gemini/config ~/.gemini/antigravity-cli
find ~/.gemini/antigravity-cli/brain -maxdepth 3 -name '*.md'
strings ~/.local/bin/agy | grep -o -E 'implementation_plan\.md|task\.md|walkthrough\.md|hooks\.json|PreToolUse|PostToolUse|plan mode' | sort | uniq -c
strings ~/.local/bin/agy | grep -o -E 'exit_plan_mode|write_to_file|task_boundary' | sort | uniq -c
strings ~/.local/bin/agy | grep 'hooks.json' | sort -u

# tos: a Markdown renderer, file watching, and where new panes go
grep -n "pub fn markdown" crates/tos/src/server/finder/view.rs
grep -n "notify" crates/tos/Cargo.toml crates/tos/src/server/finder/mod.rs
sed -n 486,503p crates/tos/src/server/actions.rs     # open_pane
sed -n 961,975p crates/tos/src/server/mod.rs         # split_at
```

## Results

### Claude Code 2.1.326

```
ls ~/.claude/plans  → No such file or directory
settings.json hooks → {}
```

Tool calls in all of this machine's transcripts (top of the list, trimmed):

```
  28708 Bash
   2915 Edit
    967 Write
    925 Read
    ...
```

`ExitPlanMode`, `TodoWrite` and `Task*` don't appear at all. Plans are written with `Write` and `Edit`, as the
`CLAUDE.md` files ask. Write/Edit calls on paths containing "plan", by folder (project names left out):

```
  236 <repo 1>/docs/plans/
   47 ~/termos/docs/plans/
   13 <repo 2>/.kb/docs/plans/
    9 <repo 3>/docs/plans/
    9 <repo 4>/<its plan folder>/
   ... (a few more, under 15 each)
```

### Codex 0.160.1

```
codex features list:
  hooks                 stable        true
  plugin_hooks          removed       false

strings: hooks.json 6, PostToolUse 42, SessionStart 55, update_plan 50
hook payload fields: session_id, transcript_path, cwd, hook_event_name, turn_id, tool_name, tool_input, tool_response
```

So Codex's hooks are on by default and take the same event names and payload fields as Claude Code's.

- **The plan is a tool call:** `update_plan`, with a list of steps and their status. Codex also has a separate
  Plan Mode: the binary has a prompt section titled "## Plan Mode vs update_plan tool".
- **This machine's rollouts have none:** 45 of them, and none mentions `update_plan`. Their tool calls are
  `exec` (526), `js` (4), `kb` (3) and `request_user_input_async` (2).
- **Unconfirmed:** whether `PostToolUse` fires for `update_plan`, or only for commands, and where Codex reads
  `hooks.json` from.

### agy 1.5.9

```
~/.gemini/config/                       config.json  mcp_config.json  plugins  projects  sidecars
~/.gemini/antigravity-cli/brain/        9 conversation folders (<uuid>/scratch, .user_uploaded, …)
*.md in brain/                          1 file: brain/<id>/plan_append_only_jsonl_graph.md

strings: hooks.json 45, PreToolUse 19, PostToolUse 17, plan mode 13, planning mode 15,
         task.md 44, implementation_plan.md 3, walkthrough.md 3,
         write_to_file 46, exit_plan_mode 7, task_boundary 9
```

Its changelog, from the binary:

```
- Fixed a bug where the `/hooks` command wrote configurations to `~/.gemini/antigravity-cli/hooks.json`
  instead of the shared `~/.gemini/config/hooks.json` …
- … `PreToolUse`, `PostToolUse`, `Stop`, and startup `SessionStart` hooks are now converted, with tool names in
  matchers mapped to built-in tools (for example `Bash` to `run_command`) …
```

So `agy` reads Claude-style hooks from `~/.gemini/config/hooks.json`. It writes files with `write_to_file`, has a
plan mode that ends with `exit_plan_mode`, and keeps a conversation's artifacts (`task.md`,
`implementation_plan.md`) in `~/.gemini/antigravity-cli/brain/<conversation id>/`. Its hook payload wasn't
read: only `"session_id"`, `"conversation_id"` and `"tool_name"` turn up as strings.

### Cursor CLI

It isn't installed, so nothing was checked.

### tos 0.4.5

- **Showing Markdown:** `server/finder/view.rs` has `pub fn markdown(src, width) -> (Vec<Line>, Vec<Link>)`,
  built on pulldown-cmark. The finder's preview uses it. `notify` is already a dependency; the finder watches its
  folder with it.
- **Where a new pane goes:** `open_pane` splits the focused pane of the active tab. Under it, `split_at(target,
  dir, opts)` can split any pane, and then sets `tab.focus` to the new pane:

```
let old = std::mem::replace(&mut tab.focus, id);
self.focus_changed(Some(old), Some(id));
```

  So `tos run -d right …`, started from a hook, would open in whichever tab is active and take the focus away
  from the agent.
- **Every pane's environment has `TOS_SESSION` and `TOS_PANE_ID`** (`pane.rs:113`). An agent's hooks are its
  child processes, so they get both.
- **Speed:** an earlier finding measured a `tos action` at a median of 2.7 to 3.1 ms
  ([2026-11-07-tb-action-latency](2026-11-07-tb-action-latency.md)).

## Conclusions

- **All three installed agents have Claude-style hooks** (`PreToolUse`/`PostToolUse` with `tool_name` and
  `tool_input` on stdin). A hook runs inside the agent's pane, so `TOS_PANE_ID` says which pane the plan belongs
  to. No guessing is needed.
- **For this user, a plan is mostly a Markdown file** that Claude writes to `docs/plans/`. Plan mode and todo
  lists aren't used. The plan pane should therefore follow a file first, and render step lists second.
- **`tos` already has the parts for the viewer** (the Markdown renderer and `notify`). It lacks a way to open a
  pane beside a given pane without taking focus.
- **Still unknown, to check with a logging hook before building `tos plan hook` (plan step 0):**
  - Claude: the payloads of `Write`/`Edit` and of `ExitPlanMode`.
  - Codex: whether `PostToolUse` fires for `update_plan`, and where its `hooks.json` lives.
  - `agy`: its payload fields, the path inside `write_to_file`'s input, and when it writes `task.md` and
    `implementation_plan.md`.
  - Cursor CLI: everything.
