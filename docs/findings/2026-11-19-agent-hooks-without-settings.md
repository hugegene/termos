# Finding: can `tos` give an agent a hook without writing to the agent's settings file? (2026-11-19)

## Question

The user asked why the hook in the [docs pane plan](../plans/2026-11-17-agent-plan-pane.md) still lives in the
agent's settings file, and whether it can live in the `tos` finder. That decides between:

- **hooks in the agents' settings files,** which `tos` writes;
- **hooks passed per session,** on the agent's command line, with the file kept in `tos`'s own folder;
- **no hooks:** `tos` watches the files itself.

The second needs a per-session flag in each agent, and a way for `tos` to add it when you type `claude` in a
pane.

## Method

Dev machine: Ubuntu 24.04, Linux 6.8. `tos` at `b446aec` (0.4.5). Claude Code 2.1.327, `codex-cli 0.160.1`,
`agy` 1.5.9. Nothing was run in a session: help text and source only.

```sh
claude --help | grep -i -E -- '--settings|--plugin-dir|setting-sources'
codex --help  | grep -i -E -- '-c, --config|--enable|hook trust'
codex --help  | grep -B3 -A1 'persisted hook trust'
agy help      | grep -i -E 'settings|hook|config|plugin'
grep -n PATH ~/.bashrc ~/.profile ~/.bash_profile
sed -n 95,130p crates/tos/src/server/pane.rs     # how tos starts a pane's program
cat /proc/sys/fs/inotify/max_user_watches
```

## Results

**Claude Code 2.1.327:**

```
  --settings <file-or-json>             Settings file or JSON string to load
                                        more settings from
  --plugin-dir <path>                   Load a plugin from a directory or .zip
  --setting-sources <sources>           Comma-separated list of setting sources
```

**Codex 0.160.1:**

```
  -c, --config <key=value>
      --enable <FEATURE>
          Enable a feature (repeatable). Equivalent to `-c features.<name>=true`
      --dangerously-bypass-hook-trust
          Run enabled hooks without requiring persisted hook trust for this invocation. DANGEROUS.
          Intended only for automation that already vets hook sources
```

**`agy` 1.5.9:**

```
  --settings                      Settings for this session only, never saved: a JSON object of settings.json keys, inline or as a file path
```

**This machine's shell startup** puts `~/.local/bin` first in `PATH` again, after anything set beforehand:

```
~/.bashrc:118: export PATH="$HOME/.local/bin:$PATH"
~/.bashrc:129: export PATH="/home/hugegene/.local/bin:$PATH"
~/.bashrc:135: export PATH="$HOME/termos/target/release:$PATH"
```

**How `tos` starts a pane:** it runs the default shell, or the given command, with `TERM`, `COLORTERM`,
`TOS_SESSION` and `TOS_PANE_ID` added to the environment. It doesn't change `PATH`, and it adds no startup file
of its own (`pane.rs` 108–117).

**inotify:** `max_user_watches` is `524288`.

## Conclusions

- **A hook only exists inside the agent's process.** The agent learns of it from its settings files, or from
  its command line when it starts. Nothing `tos` does from outside can add a hook to an agent that's already
  running.
- **Per-session flags:**
  - **Claude:** `--settings` and `--plugin-dir` add to the session's settings without saving anything. Whether
    hooks given that way merge with the user's own hooks is **unconfirmed**.
  - **`agy`:** `--settings` is "for this session only, never saved". Whether it takes hooks, which `agy`
    otherwise keeps in `hooks.json` rather than `settings.json`, is **unconfirmed**.
  - **Codex:** only has `-c key=value`. Whether hooks can be set that way is unconfirmed.
- **Codex needs its own approval for any hook:** "persisted hook trust", given inside Codex, unless the session
  starts with a flag Codex calls DANGEROUS. So hooks in Codex always involve one step outside `tos`.
- **Adding a flag when you type `claude` would need `tos` to step into the shell's startup.** A `claude` wrapper
  placed first in the pane's `PATH` loses to this machine's `~/.bashrc`, which puts `~/.local/bin` first again.
  The way around it is to inject a startup file for bash, zsh and fish, as VS Code's shell integration does.
- **Watching files instead is affordable:** the inotify limit (524288) leaves room for watching a repo's
  folders one by one, skipping `.git`, `node_modules` and `target`.
