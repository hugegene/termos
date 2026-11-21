# Finding: which file each agent reads its hooks from, and in what shape (2026-11-21)

## Question

The user asked where option A of the [docs pane plan](../plans/2026-11-17-agent-plan-pane.md) would write the
hook for each agent. What `tos` writes, and how it removes it again, depends on the file and its format for each
agent. It also depends on whether the agent asks you anything before a new hook runs.

## Method

Dev machine: Ubuntu 24.04, Linux 6.8. Claude Code 2.1.327, `codex-cli 0.160.1`, `agy` 1.5.9. No agent was run:
this comes from their files, help and binaries. Cursor CLI isn't installed.

```sh
python3 -c "import json; d=json.load(open('$HOME/.claude/settings.json')); print(list(d), d.get('hooks'))"
strings -n 6 ~/.local/bin/codex | LC_ALL=C grep -a -o -E \
  '.{0,100}(\.codex/hooks|/hooks\.json|trusted_hash|hook_trust|hooks\.state|HookEventsToml).{0,100}'
codex --help | grep -B3 -A1 'persisted hook trust'
# agy carries its own docs; this prints the "Lifecycle Hooks (hooks.json)" page
n=$(strings -n 4 ~/.local/bin/agy | LC_ALL=C grep -a -n 'Lifecycle Hooks (`hooks.json`)' | head -1 | cut -d: -f1)
strings -n 4 ~/.local/bin/agy | sed -n "${n},$((n+260))p"
strings -n 4 ~/.local/bin/agy | LC_ALL=C grep -a -E 'customization root|~/.gemini/config/hooks.json|\.agents/hooks.json'
```

## Results

### Claude Code 2.1.327

`~/.claude/settings.json` has the keys `model`, `modelSettings`, `tui`, `theme`, `remoteControlAtStartup`,
`agentPushNotifEnabled` and `autoMode`, and no `hooks`. Hooks go under a `hooks` key in this file. The shape
(event → matcher groups → handlers) is from Claude Code's hooks documentation, not measured here.

### Codex 0.160.1

From the binary, trimmed:

```
.codex/config.toml  .codex/agents  .codex/hooks  …  hooks.json  …  hooks/hooks.json   (plugins)
internally tagged enum HookHandlerConfig … matcher hooks HookStateToml trusted_hash HookEventsToml
  PreToolUse PermissionRequest PostToolUse PreCompact PostCompact SessionStart SessionEnd SubagentStart …
hooks.state."
 failed to serialize hooks.json:
`bypass_hook_trust` override must be a boolean
```

```
--dangerously-bypass-hook-trust
    Run enabled hooks without requiring persisted hook trust for this invocation. DANGEROUS.
    Intended only for automation that already vets hook sources
```

The event names, `matcher` and `hooks` match Claude's. The payload fields found earlier are `session_id`,
`cwd`, `tool_name`, `tool_input` and so on ([2026-11-17](2026-11-17-agent-plan-sources.md)).

- **The file is probably `~/.codex/hooks.json`** (`$CODEX_HOME`, default `~/.codex`). The `…Toml` names suggest
  that a `[hooks]` table in `config.toml` may work too. **Unconfirmed.**
- **Trust is kept per hook** as `hooks.state."<id>".trusted_hash` in `config.toml`. A hook with no trusted hash
  doesn't run until you trust it in Codex, unless the session starts with the bypass flag.

### agy 1.5.9

From its own docs page (`# Lifecycle Hooks (hooks.json)`), trimmed:

```
Hooks are configured in a single `hooks.json` file placed in your customization root directory
(e.g., `.agents/hooks.json`).

The `hooks.json` file is a JSON object where each top-level key is a **hook name**, mapping to its
event configuration.
  "lint-checker": {
    "PostToolUse": [
      { "matcher": "run_command",
        "hooks": [ { "type": "command", "command": "./scripts/lint.sh", "timeout": 10 } ] } ] },
  "safety-gate": { "enabled": false, "PreToolUse": [ … ] }

*   **Merging**: Multiple named hooks … for the same event type are merged and executed sequentially.
*   **`command`** (string, required): … run via `sh -c` on Unix … `~` is expanded to the home directory.
    The working directory is set to the directory containing `hooks.json`.
*   **`timeout`** (int, optional): Execution timeout in seconds. Defaults to `30`.

Hook commands receive context as a JSON object on **stdin** and must output their result as a JSON object
on **stdout**.
> All JSON keys in the hook payloads use **camelCase** (protojson encoding), e.g., `conversationId` and `stepIdx`.
  common: conversationId, workspacePaths, transcriptPath, artifactDirectoryPath, modelName, lastUserInput,
          hookEventName, cwd
### 2. `PostToolUse` Contract
    { "toolCall": { "id": "toolu_01", "name": "run_command", "args": { "CommandLine": "npm test" } },
      "stepIdx": 5, "error": "exit status 1", ... (common fields) }
*   **Output (stdout)**: Expects an empty JSON object `{}`.
```

Its changelog names the user-wide file:

```
- Fixed a bug where the `/hooks` command wrote configurations to `~/.gemini/antigravity-cli/hooks.json`
  instead of the shared `~/.gemini/config/hooks.json` …
- Fixed workspace-local hooks defined in `<workspace>/.agents/hooks.json` not loading after trusting a folder …
```

### Cursor CLI

It isn't installed, so nothing was checked. Its docs describe `~/.cursor/hooks.json` with an `afterFileEdit`
event. That's from memory, **unverified**.

## Conclusions

| Agent | User-wide file | Shape | Before a new hook runs |
|---|---|---|---|
| Claude Code | `~/.claude/settings.json`, key `hooks` | event → `[{matcher, hooks: [{type, command}]}]` | nothing known |
| Codex | probably `~/.codex/hooks.json` (unconfirmed) | the same as Claude's | you trust it once in Codex. That's kept as `hooks.state.<id>.trusted_hash` in `config.toml` |
| `agy` | `~/.gemini/config/hooks.json` | **named** hooks: `"tos-docs": {PostToolUse: [{matcher, hooks: [...]}]}` | nothing known |
| Cursor CLI | `~/.cursor/hooks.json` (unverified) | `afterFileEdit` (unverified) | unknown |

- **`agy` differs in three ways:**
  - its hooks are named, so `tos` can own one key, `tos-docs`, and remove it whole;
  - its payloads are camelCase, with the tool under `toolCall.name` and `toolCall.args`;
  - a `PostToolUse` hook must print `{}`.
- **Codex's trust step can't be done from `tos`.** `tos` shouldn't write a trusted hash itself, since that would
  skip Codex's own check on new hooks.
- **Still unknown (step 0):**
  - which file Codex reads, and which tool name a Codex file edit arrives as (`apply_patch`, or `exec`, which
    this machine's rollouts show);
  - the field in `write_to_file`'s `args` that holds the path.
