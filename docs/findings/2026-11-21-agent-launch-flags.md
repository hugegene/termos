# Finding: what each agent CLI accepts for one session: a hook, and extra instructions (2026-11-21)

## Question

The user proposed launchers, `tos claude`, `tos codex`, `tos agent`, `tos agy`, `tos grok` and so on. Each would
start the agent with a hook and a starting prompt added for that session only, and write nothing to the agent's
own files. The user asked for a comprehensive list of the agent CLIs ("harnesses") this would have to cover.

The [docs pane plan](../plans/2026-11-17-agent-plan-pane.md) depends on, for each agent:

- **a hook for one session:** a flag, an environment variable or an extra config file that adds a hook without
  editing the user's config, and without moving the agent's config folder (which can lose its login);
- **the hook itself:** the event after a file write or edit, the tool names, and the field holding the path;
- **the starting prompt:** a way to add instructions to the system prompt for one session, and a way to give a
  first prompt while staying interactive.

## Method

**Locally** (Ubuntu 24.04, Linux 6.8; the four agents installed here, none run in a session):

```sh
claude --version   # 2.1.330
claude --help | grep -E -A3 -- '--settings |--append-system-prompt|--plugin-dir|--setting-sources'
codex --help  | sed -n '/^Arguments:/,/^Options:/p'; codex --help | grep -A3 -- '-c, --config'   # 0.160.1
strings ~/.local/bin/codex | grep -o -E '\b(developer_instructions|base_instructions|model_instructions_file)\b' | sort | uniq -c
# does Codex take hooks from -c? (valid, then invalid)
codex -c 'hooks.PostToolUse=[{matcher="apply_patch",hooks=[{type="command",command="true"}]}]' features list >/dev/null; echo $?
codex -c 'hooks.PostToolUse="not-a-list"' features list >/dev/null; echo $?
agy help | grep -i -E -- '--settings|--prompt|-i,|system|plugin'                                  # 1.5.9
gemini --help | grep -i -E -- 'prompt|system|settings|extension|hook'                              # 0.62.0
cd ~/.local/lib/node-v22.23.2-linux-x64/lib/node_modules/@google/gemini-cli/bundle
grep -o -h -E 'GEMINI_CLI_SYSTEM_SETTINGS_PATH|GEMINI_CLI_SYSTEM_DEFAULTS_PATH|GEMINI_SYSTEM_MD|GEMINI_CLI_HOME' *.js | sort | uniq -c
sed -n 14704,14800p chunk-RWN52KQV.js | grep -n -E 'AfterTool|BeforeTool:|mergeStrategy'          # the hooks schema
```

**From documentation and source:** the other agents aren't installed. On 2026-11-21, three research agents read
each one's official docs, repository source, changelog and release pages, and gave a URL for every claim.
Nothing was run. The versions come from npm, PyPI and GitHub releases on that day.

## Results

### Installed here, checked locally

**Claude Code 2.1.330:**

```
Usage: claude [options] [command] [prompt]
  --append-system-prompt <prompt>       Add text to the end of the default system prompt
  (also --append-system-prompt-file, --system-prompt[-file])
  --plugin-dir <path>                   Load a plugin from a directory or .zip for this session only
  --setting-sources <sources>           Comma-separated list of setting sources to load (user, project, local).
  --settings <file-or-json>             Settings file or JSON string to load more settings from
```

**Codex 0.160.1:**

```
Arguments:
  [PROMPT]
          Optional user prompt to start the session
  -c, --config <key=value>
          Override a configuration value that would otherwise be loaded from `~/.codex/config.toml`.
          Use a dotted path (`foo.bar.baz`) to override nested values. The `value` portion is parsed as TOML.

strings: developer_instructions 3, base_instructions 1

codex -c 'hooks.PostToolUse=[{matcher="apply_patch",hooks=[{type="command",command="true"}]}]' features list → exit 0
codex -c 'hooks.PostToolUse="not-a-list"' features list → exit 1:
    Error: failed to load bootstrap configuration
    Caused by:
        invalid type: string "not-a-list", expected a sequence
        in `hooks`
```

So Codex reads hooks from `-c` and checks their shape. Two things are **unconfirmed**: whether such a hook runs,
and how Codex's per-hook trust (see [2026-11-21-agent-hook-files](2026-11-21-agent-hook-files.md)) treats it.

**agy 1.5.9:**

```
  -i                              Short alias for --prompt-interactive
  --prompt-interactive            Run an initial prompt interactively and continue the session
  --settings                      Settings for this session only, never saved: a JSON object of settings.json keys, inline or as a file path
```

Its hooks live in `hooks.json`, not `settings.json`. Whether `--settings` accepts hooks is **unconfirmed**. No
system-prompt flag or per-session plugin flag was found.

**Gemini CLI 0.62.0:**

```
  gemini [query..]             Launch Gemini CLI  [default]
  -i, --prompt-interactive     Execute the provided prompt and continue in interactive mode
  gemini hooks <command>       Manage Gemini CLI hooks.
  -e, --extensions             A list of extensions to use. If not provided, all extensions are used.

bundle: GEMINI_CLI_SYSTEM_SETTINGS_PATH 12, GEMINI_CLI_SYSTEM_DEFAULTS_PATH 6, GEMINI_SYSTEM_MD 6, GEMINI_CLI_HOME 4
hooks schema: BeforeTool … mergeStrategy: "concat", AfterTool … mergeStrategy: "concat" (every hook event)
```

So a hooks-only file named by `GEMINI_CLI_SYSTEM_SETTINGS_PATH` adds its hooks to the user's own. The file
replaces any `/etc/gemini-cli/settings.json` for that session. `GEMINI_SYSTEM_MD` replaces the whole system
prompt; nothing appends to it.

### From documentation: an agent with a per-session flag or environment variable

| Agent, command, version | Hook for one session | After-edit event, tools, path field | Starting prompt, still interactive | Extra instructions |
|---|---|---|---|---|
| Cursor CLI `agent` 2026.11.17 | `--plugin-dir DIR`, with the hooks in the plugin's `hooks/hooks.json`. Plugin hooks run since 2026.08.11 | `afterFileEdit` (`file_path`), or `postToolUse` with matcher `Write`. That `afterFileEdit` fires in the CLI is **unconfirmed** | `agent "…"` | no flag. Plugin rules: **unconfirmed** |
| GitHub Copilot CLI `copilot` 1.0.114 | `--plugin-dir DIR` (plugin `hooks.json`). Trust for plugin hooks: **unconfirmed** | `postToolUse`, tools `create`, `edit`; path key in `toolArgs` **unconfirmed** | `-i "…"` | env `COPILOT_CUSTOM_INSTRUCTIONS_DIRS` (reads `AGENTS.md` and `*.instructions.md` there) |
| xAI Grok Build `grok` (1.0.45 in the repo) | `--plugin-dir DIR`; the source says these are "always trusted: hooks and MCP servers activate without a prompt" | `PostToolUse`, matcher `Edit\|Write` (tool `search_replace`, `file_path`) | `grok "…"` | `--rules` (alias `--append-system-prompt`) |
| Qwen Code `qwen` 0.28.3 | env `QWEN_CODE_SYSTEM_SETTINGS_PATH`; system hooks load "regardless of folder trust" | `PostToolUse`, tools `write_file`, `edit` (`tool_input.file_path`) | `-i "…"` | `--append-system-prompt` |
| Factory Droid `droid` 1.16.0 | `--settings FILE`, "merged on top … for this process only" | `PostToolUse`, matcher `Create\|Edit\|ApplyPatch` (`file_path`; `ApplyPatch` needs the patch read) | `droid "…"` | `--append-system-prompt[-file]` |
| Auggie `auggie` 1.2.2 | `--plugin-dir DIR` | `PostToolUse`, matcher `Write\|Edit` (`tool_input.path`) | `auggie "…"` | `--append-system-prompt`, `--rule` |
| JetBrains Junie `junie` (EA nightly 4191.1) | `--config-location FILE` or `JUNIE_CONFIG_LOCATION`, with a `hooks` object | `PostToolUse`, tools `Write`, `Edit`, `apply_patch` (`file_path`, absolute) | `--prompt "…"` | `--system-prompt "…"` |
| OpenCode `opencode` 2.3.2 | env `OPENCODE_CONFIG_CONTENT='{"plugins":["/abs/dir"]}'`, merged above the project config. The hook is a TS/JS plugin, not a shell command | `execute.after`; tools `write`, `edit` (`input.path`), `patch` | `--prompt "…"` | `instructions` in the same variable |

### From documentation: only by moving the agent's config folder, or a workaround

| Agent | How | What it costs |
|---|---|---|
| Amp `amp` | `XDG_CONFIG_HOME=<tmp>` moves the plugin folder; `--settings-file` keeps the user's settings. The hook is a TS plugin (`tool.result`) | whether login and `AGENTS.md` move too is **unconfirmed** |
| Crush `crush` 0.102.0 | `CRUSH_GLOBAL_CONFIG=<tmp>`, whose `crushrc` sources the user's and adds `hook add PostToolUse …` | a legacy `crush.json` isn't loaded. The login survives |
| Cline `cline` 3.0.74 | `--hooks-dir` is documented, but nothing in the source reads it. `--config <tmp>` plus `CLINE_DATA_DIR=~/.cline/data` works in the source | hides the user's own hooks, rules and skills for that run |
| Aider `aider` 0.86.2 | no hooks. `--lint-cmd "markdown: /abs/hook"` runs on each edited `.md` file | the hook must print nothing |

### From documentation: no per-session way found

| Agent | Why not |
|---|---|
| Kimi Code CLI `kimi` 2.7.2 | hooks only in `~/.kimi-code/config.toml` or installed plugins. `KIMI_CODE_HOME` moves the login |
| Goose `goose` 1.60.2 | plugins only in `~/.agents/plugins` or the project. `GOOSE_PATH_ROOT` moves its config |
| Kiro CLI `kiro-cli` 3.5.0 | only a new agent file in `~/.kiro/agents/` plus `--agent NAME`. `KIRO_HOME` holds sign-ins |
| Continue `cn` 1.5.47 | none. `CONTINUE_GLOBAL_DIR` moves `auth.json`. It also runs the user's Claude Code hooks |
| Mistral Vibe `vibe` 3.1.4 | none. `VIBE_HOME` moves the API key |
| OpenHands `openhands` 1.16.0 | the hooks path is hard-coded to `~/.openhands/hooks.json` |

### Found but not researched in depth

- **With hooks:** Kilo CLI `kilo` (TS plugins), Devin CLI `devin`, Qoder CLI `qodercli`, Letta Code `letta`, Pi
  `pi` (TS extensions), Atlassian Rovo Dev `acli rovodev` and Snowflake Cortex Code `cortex`.
- **No hooks found:** Warp `warp`, Trae Agent `trae-cli`, Plandex `plandex` (no release since July 2025), and Roo
  Code CLI (hooks only in a draft PR).
- **Gone:** iFlow CLI shut down on 2026-04-17.
- **Two programs called `grok`:** xAI's Grok Build and the community `superagent-ai/grok-cli` both install the
  command `grok`.

### Sources

- **Cursor:** https://cursor.com/docs/hooks · https://cursor.com/docs/cli/reference/parameters ·
  https://cursor.com/docs/cli/changelog · https://cursor.com/docs/plugins
- **Copilot:** https://docs.github.com/en/copilot/reference/hooks-reference ·
  https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference ·
  https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-plugin-reference
- **Gemini:** https://github.com/google-gemini/gemini-cli/blob/main/docs/hooks/reference.md ·
  `packages/cli/src/config/settingsSchema.ts`
- **Qwen:** https://qwenlm.github.io/qwen-code-docs/en/users/features/hooks/
- **Grok:** https://docs.x.ai/build/features/hooks · https://docs.x.ai/build/cli/reference ·
  https://github.com/xai-org/grok-build (`crates/codegen/xai-grok-pager/src/app/cli.rs`) ·
  https://github.com/superagent-ai/grok-cli
- **Kimi:** https://github.com/MoonshotAI/kimi-code/blob/HEAD/docs/en/customization/hooks.md
- **Amp:** https://ampcode.com/docs/customize/plugins · https://ampcode.com/docs/cli/settings
- **OpenCode:** https://opencode.ai/docs/plugins/ · https://opencode.ai/docs/config/
- **Crush:** https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md
- **Goose:** `documentation/docs/guides/context-engineering/hooks.md` in `aaif-goose/goose`
- **Aider:** `aider/linter.py` and `aider/website/docs/config/options.md` in `Aider-AI/aider`
- **Droid:** https://docs.factory.ai/reference/hooks-reference · https://docs.factory.ai/reference/cli-reference
- **Cline:** https://docs.cline.bot/cli/cli-reference.md, and in `cline/cline`
  `sdk/packages/core/src/hooks/hook-file-config.ts` and `apps/cli/src/main.ts`
- **Kiro:** https://kiro.dev/docs/hooks/ · https://kiro.dev/docs/reference/cli-commands/
- **Auggie:** https://docs.augmentcode.com/cli/hooks.md · https://docs.augmentcode.com/cli/reference.md
- **Continue:** `extensions/cli/src/hooks/hookConfig.ts` in `continuedev/continue`
- **Vibe:** https://docs.mistral.ai/vibe/code/cli/hooks
- **OpenHands:** https://docs.openhands.dev/openhands/usage/customization/hooks.md
- **Junie:** https://junie.jetbrains.com/docs/junie-cli-hooks.html · https://junie.jetbrains.com/docs/parameters.html

## Conclusions

- **The launcher idea works without writing to any agent's files** for Claude, Codex, Gemini, Cursor, Copilot,
  Grok Build, Qwen, Droid, Auggie, Junie and OpenCode. Each has a flag or environment variable that adds a hook for
  one session. For Codex it's `-c hooks.…`, which parses; whether such a hook runs is unconfirmed.
- **`agy`, the fourth installed agent, is uncertain.** It has `--settings`, but its hooks live in a different
  file. If `--settings` can't carry them, `tos agy` needs the folder-watch fallback.
- **Amp, Crush, Cline and Aider need workarounds** that move a config folder or misuse a linter. Kimi, Goose,
  Kiro, Continue, Vibe and OpenHands have no per-session way at all. For those, `tos` would have to watch the
  folder instead.
- **Extra instructions for one session** are easy for Claude, Grok, Qwen, Droid, Auggie, Junie and OpenCode, and
  probably Codex (`developer_instructions`). They're missing or replace-only for `agy`, Gemini, Cursor and
  Cline.
- **Unknowns for plan step 0,** with the installed agents:
  - Claude: do hooks from `--settings` run alongside the user's own?
  - Codex: does a `-c` hook run, and what does its trust prompt look like?
  - `agy`: does `--settings` take hooks?
  - Gemini: does a hook from the system settings file run?
