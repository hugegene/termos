# Plan: zellij's session commands in `tb`

**Status:** built (2026-11-04) · **Date:** 2026-11-04 · **Finding:** [zellij's session commands](../findings/2026-11-04-zellij-cli.md)
· **Related:** [zellij keys and frames](2026-11-04-zellij-keys-and-frames.md), [build steps 4–5](2026-11-04-build-steps-4-5.md)

## Decision

`tb` accepts zellij's session commands, with the same names, aliases, arguments and flags, so the user's zellij
habits work unchanged. Where zellij would refuse (attaching to a missing session), `tb` keeps its friendlier
behaviour, so nothing that works in zellij fails in `tb`.

| zellij | tb |
|---|---|
| `zellij -s work` | `tb -s work`: attach to "work", creating it if needed (unchanged) |
| `zellij attach work`, `zellij a work` | same; the name can also be given with `-s` |
| `attach -c` / `--create` | accepted; `tb` always creates a missing session |
| `attach -b` / `--create-background` | starts the session without attaching (as `tb start`) |
| `attach --forget` | deletes the saved layout first, so the session starts fresh; ignored, with a note, if the session is running |
| `attach -f` / `--force-run-commands` | when restoring, runs the commands that were running instead of waiting for Enter |
| `list-sessions`, `ls` with `-s` / `--short`, `-r` / `--reverse`, `-n` / `--no-formatting` | names only; reverse order; `-n` accepted (`tb` prints no colours) |
| `kill-session [NAME]`, `k` | `tb kill` with the name as an argument |
| `delete-session [NAME] [-f]`, `d` | `tb delete`; `-f` added beside `--force` |
| `kill-all-sessions [-y]`, `ka` | new: ends every running session (layouts kept); asks first unless `-y` |
| `delete-all-sessions [-y] [-f]`, `da` | new: forgets every exited session; `-f` also ends and forgets running ones; asks first unless `-y` |
| `action`, `ac` | `ac` alias added |

- `tb ls` marks the session you're in with `(current)`, as zellij does.
- `tb`'s own names stay (`tb start`, `tb kill`, `tb delete`), and zellij's names are aliases shown in the help.

**Kept different:**
- Plain `tb` attaches to (or creates) "main". zellij makes a new session with a random name each time; `tb`
  keeps one predictable default, so `tb` always gets you back to the same place.
- `tb attach` with no name uses "main" (or the session you're in), not "the only running session".

**Not copied:**
- `attach --index`.
- zellij's remote options (`--token`, `--ca-cert`, `--insecure`, `--remember`) and the non-session commands
  (`run`, `edit`, `watch`, `web`, `pipe`, `plugin`, `setup`, `options`, `convert-*`, `list-aliases`,
  `subscribe`).
- `zellij action`'s own action names. `tb action` already has some zellij names as aliases (`move-focus`,
  `close-pane`, `toggle-fullscreen`, `write-chars`); the rest is later work.

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Missing session on `attach` | Fail without `-c`, as zellij does | ❌ Breaks `tb attach`, which already creates; a refusal helps nobody here |
| | **Always create; accept `-c`** | ✅ |
| Plain `tb` | A new session with a random name, as zellij | ❌ The user already relies on `tb` returning to "main" |
| | **Keep "main"** | ✅ |
| `-s` | One global `-s` for every command (today) | ❌ Clashes with `ls -s` (`--short`) |
| | **`-s` on each command that takes a session** | ✅ `tb attach -s work` keeps working, and `tb ls -s` means `--short` |

## Steps

- [x] CLI: positional session names on `attach`, `kill` and `delete`; `-s` per subcommand; aliases `a`, `k`,
  `d`, `ka`, `da`, `ac`.
- [x] `attach -c`, `-b`, `--forget`, `-f` (the server takes a hidden `--run-commands` flag).
- [x] `ls -s`, `-r`, `-n` and the `(current)` mark.
- [x] `kill-all-sessions` and `delete-all-sessions`, with a y/N question unless `-y`.
- [x] Tests: parsing unit tests; an end-to-end test for the new commands.
- [x] README: the commands.

## Change log

- **2026-11-04:** written.
- **2026-11-04:** built.
  - **Code:** the command line in `main.rs`; `list_sessions`, `kill_all_sessions`, `delete_all_sessions`,
    `forget_saved` and `ensure_outside` in `client.rs`; the server's hidden `--run-commands` flag, which makes
    `restore` press Enter after each restored command.
  - **Ending all sessions from inside one:** the session you're in is ended last, so `tb` isn't stopped before it
    has ended the others.
  - **Tests:** 3 unit tests (`Cli::command().debug_assert()`, parsing, the order the session is picked in) and
    the end-to-end test `zellij_session_commands`. The suite passed 3 runs in a row. Other tests now also run
    `kill-all-sessions -y` when they finish, in their own socket folder, so no test leaves a session running.
