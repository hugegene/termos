# Plan: rebuild step 6, the CLI as the extension API

**Status:** built (2026-11-05) · **Date:** 2026-11-05 · **Part of:** [rebuild plan](2026-11-03-rebuild-plan.md) §11, step 6 ·
**Follows:** [zellij's session commands](2026-11-04-zellij-cli.md) (built) · **Findings:**
[zellij's actions](../findings/2026-11-05-zellij-actions.md), [zellij's session commands](../findings/2026-11-04-zellij-cli.md)

## Decision

**Other people extend `tb` by writing programs that call its CLI**, the way `prbot` (`~/claudehook`) drives
zellij today. There is no plugin system. `tb`'s CLI becomes a stable, documented API with eight parts:

1. **zellij's commands, names and flags.** Every zellij command that maps onto a `tb` feature gets zellij's
   name, aliases and flags, and `tb`'s own names stay as aliases. A zellij script then ports by changing
   `zellij` to `tb` and the `ZELLIJ_*` variables to `TB_*`. Of zellij's 72 actions:

   | Group | Count | When |
   |---|---|---|
   | Already in `tb` (some need zellij's argument forms) | 14 | this step fixes the forms |
   | `tb` has the feature; only the CLI is missing | 41 | **this step** |
   | Need floating, stacked or pinned panes, swap layouts or borderless panes | 12 | step 8 (floating panes) |
   | Plugins (`launch-plugin`, `launch-or-focus-plugin`, `start-or-reload-plugin`, `pipe`), `override-layout` | 5 | not copied |

   - **Other top-level commands:** `run`, `edit`, `watch` (read-only attach) and `subscribe` come in this
     step.
   - **Not copied:**
     - `plugin` and `pipe`: no plugins
     - `web`: the [phone plan](2026-11-03-phone-interface.md) covers phones
     - `setup`, `options`, `convert-*` and `list-aliases`: config comes at launch
     - layout files (`new-tab --layout`): launch
2. **Stable ids.**
   - Every tab and pane has an id that never changes while it exists. `tb` already keeps them internally.
   - Commands that create a tab or pane print its id. zellij's form is `terminal_7`; `tb` prints the same and
     accepts both `7` and `terminal_7`.
3. **Machine-readable output.**
   - Every list or query command takes `-j`/`--json`.
   - Field names follow zellij's wherever zellij has the field (`tab_id`, `name`, `active`, `id`,
     `is_plugin`, …). `tb`'s extra fields are added beside them.
4. **Events.** `tb subscribe` prints one JSON object per line as things happen, so extensions react
   instead of polling (`prbot` polls zellij's tab list today). The events:
   - tab opened, closed, renamed or focused
   - pane opened, exited (with its exit status), renamed, focused or retitled
   - pane text: the viewport, like zellij's `subscribe`
   - client attached or detached
   - mode changed
5. **Context for extensions in a pane.**
   - `TB_SESSION` and `TB_PANE_ID`, already set in every pane, tell a program where it runs.
   - `current-tab-info` and `list-panes` give the rest.
   - `ZELLIJ_*` variables are never set.
6. **A stability promise and docs.**
   - The API has a version: `tb --version`, plus an `"api"` field in JSON output.
   - Within a version, commands, flags and JSON fields are only ever added, never renamed or removed.
   - Failures exit non-zero, with the message on stderr.
   - `docs/cli.md` is the reference, kept in step with `tb help`, with one example extension.

7. **Keys that run commands** (decided 2026-11-05). A user can bind a key to start an extension, as zellij's
   `Run` action does.
   - **Where:** `~/.config/tb/config.kdl`, in **zellij's keybind syntax**, so a zellij `bind` block copies
     across unchanged:
     ```kdl
     keybinds {
         tab {
             bind "p" { Run "prbot" "ls"; SwitchToMode "Normal"; }
         }
     }
     ```
   - **Actions in this step:**
     - `Run`, with zellij's `cwd`, `direction`, `name` and `close_on_exit` options
     - `NewTab`, with a command
     - `SwitchToMode`
     - any other `tb action` command, by its zellij name
   - **Clashes:** a binding replaces the built-in key in that mode.
   - **When it's read:** when the session starts, and on `tb action reload-config`.
   - **The rest of config** (themes, options, layouts) stays at launch (step 9), which extends the same
     file, so there's only one config format.
8. **Status-bar text from extensions** (decided 2026-11-05). zellij can only do this with plugins (its bars
   are plugins), so these are `tb`'s own commands:
   - `tb action set-status --id prbot "PR #487 needs review"` shows the text in the bottom bar, and
     `tb action clear-status --id prbot` removes it.
   - **Segments:** each `--id` is one segment, so several extensions share the bar. `--ttl SECS` makes the
     text expire. Plain text only for now.
   - **Layout:** segments sit at the right end of the bottom bar. When space is short, the key hints shrink
     first.
   - **Clicks:** clicking a segment sends a `status_clicked` event with its id through `tb subscribe`, so the
     extension can react, for example by opening the PR's tab.
   - **Not saved with the session:** after a restore, extensions set their text again.

**Safety stays as it is.**
- Only the session's own user can connect: the socket is `0600`, and the server checks the peer's uid.
- Extensions run with the user's rights, in their own process, so a buggy extension can't take sessions down.
- Nothing listens on the network; that's the daemon's job.

**Acceptance test:** a copy of `prbot`, with only `zellij` → `tb` and the variable names changed, opens,
renames, finds and closes its tabs in a throwaway `tb` session.

**Size:** about 3–5k lines, **estimate**. The event stream is the largest new piece; key bindings and status
text add about 1k lines.

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| How people extend `tb` | WebAssembly plugins, like zellij | ❌ A large runtime, sandboxing and an API to maintain; the rebuild plan already ruled it out |
| | An embedded scripting language (Lua) | ❌ One language only, and it runs inside the server, where a crash or hang takes sessions down |
| | **The CLI, JSON output and an event stream** | ✅ Any language; runs in its own process; the CLI exists already; zellij scripts port easily |
| Command names | Only `tb`'s own names | ❌ Every zellij script would need rewriting |
| | **zellij's names and flags, `tb`'s names as aliases** | ✅ |
| Environment in panes | Also set `ZELLIJ_*` | ❌ Programs would think they're in zellij (our own image detection checks `ZELLIJ`), and `zellij` commands would try to reach zellij |
| | **`TB_*` only** | ✅ Scripts rename two variables |
| Reacting to changes | Polling the lists | ❌ Slow and wasteful |
| | **`tb subscribe`** | ✅ |
| Key-binding format | A small format of our own (TOML) | ❌ A second format, beside the full config at launch |
| | Bindings registered through the CLI only (`tb bind`) | ❌ Lost when the session ends; users expect a config file |
| | **zellij's KDL keybind syntax** | ✅ zellij snippets copy across; launch grows the same file. Needs a KDL parser crate (to measure) |
| Status text | One shared line that any extension overwrites | ❌ Extensions would erase each other |
| | **A segment per extension id, with click events** | ✅ |

## Steps

- [x] Measure each zellij action's arguments, output and JSON fields, in a throwaway `tftest-*` session, and
  record a finding.
- [x] Tab and pane ids in every output; accept `terminal_N`; creating commands print the new id.
- [x] **Tabs:**
  - `list-tabs`, `query-tab-names`, `current-tab-info`
  - `close-tab`, `close-tab-by-id`
  - `go-to-tab-by-id`, `go-to-tab-name`, `go-to-next-tab`, `go-to-previous-tab`
  - `rename-tab-by-id`, `undo-rename-tab`
  - `move-tab`, `toggle-active-sync-tab`
- [x] **Panes:**
  - `new-pane` (zellij's flags) and `run`; `edit`
  - `move-pane`, `move-pane-backwards`, `move-focus-or-tab`
  - `rename-pane`, `undo-rename-pane`
  - `toggle-pane-frames`, `clear`, `edit-scrollback`, `set-pane-color`
  - zellij's forms of `new-tab`, `resize`, `dump-screen` and `list-panes`
- [x] **Scrolling:** `scroll-up`/`scroll-down`, `page-scroll-*`, `half-page-scroll-*`, `scroll-to-top`,
  `scroll-to-bottom`.
- [x] **Input:** `write` (bytes), `paste` (bracketed), `send-keys`.
- [x] **Session and clients:** `list-clients`, `switch-mode`, `rename-session`, `save-session`,
  `switch-session`, `watch`.
- [x] **Events:** `subscribe`.
- [x] **Keys that run commands:**
  - Measure the KDL parser crate (version, license, dependencies) and record a finding. (None was available
    offline; `tb` has its own small parser, [finding](../findings/2026-11-05-kdl-parser.md).)
  - Parse zellij's `keybinds` syntax for `Run`, `NewTab`, `SwitchToMode` and the CLI actions.
  - Add `reload-config`.
- [x] **Status text:** `set-status`, `clear-status`, `--ttl`, the bar layout, `status_clicked` events.
- [x] **API:** the version, the stability rules, `docs/cli.md` and an example extension.
- [x] **Tests:** end-to-end tests for each group of commands, plus the `prbot` acceptance run.

## Open questions

None at the moment. Both earlier questions (keys that start an extension, and status-bar text) were answered
"yes" on 2026-11-05; see items 7 and 8.

## Change log

- **2026-11-05:** written. The user set the aim: other people extend `tb` through its CLI.
- **2026-11-05:** the user said yes to both open questions.
  - **Keys that run commands come into this step:** zellij's keybind syntax in `~/.config/tb/config.kdl`.
  - **Status-bar text** from extensions: `set-status` and `clear-status`, with click events.

  The size estimate rose to 3–5k lines.
- **2026-11-05:** built.
  - **Code:**
    - `cli.rs`: `tb action` with zellij's names, flags and text columns
    - `server/actions.rs`: the actions and the JSON
    - `server/events.rs`: `subscribe`
    - `server/config.rs` and `kdl.rs`: key bindings
    - `keyspec.rs`: zellij's key names
    - status segments in `keys.rs`
    - `switch-session` and `watch` in `client.rs`
  - **Reference:** [docs/cli.md](../cli.md), with the example extension
    [docs/examples/exit-watcher.py](../examples/exit-watcher.py).
  - **Tests:**
    - 13 unit tests: ids, `resize` forms, text columns, colours, modes, key names, the KDL parser, bindings,
      events, and every action being in `docs/cli.md`
    - 9 end-to-end tests: tab actions, pane actions, input and session actions, `subscribe`, status
      segments, `switch-session` and `watch`, key bindings, the example extension
    - The suite passed 3 runs in a row, and the real Neovim test passed too.
  - **Acceptance:** `prbot`'s own tab code runs on `tb` with 7 lines changed
    ([finding](../findings/2026-11-05-prbot-on-tb.md)).
  - **Changes from the plan:**
    - **Events** are differences between summaries of the session, sent at most 10 times a second, so every
      code path is covered without hooks. Something that appears and disappears within 0.1 s may go
      unreported.
    - **The API version is in `tb --version`** (`extension API 1`), not in an `"api"` JSON field, because
      list output is a JSON array.
    - **`tb ls` prints zellij's shape** (`NAME [2 tabs, 1 attached]`, `NAME (EXITED - attach to restore)`).
      `prbot` parses it; the acceptance run found this.
    - **`write` now takes bytes, as zellij's does;** typing text is `write-chars` (`--enter` stays).
    - **`-s SESSION` goes before `action`'s subcommand** (`tb action -s work new-tab`), because zellij's
      actions use `-s` for their own flags (`list-panes -s`, `new-pane -s`).
    - **The protocol version is now 3.** A session started by an older `tb` refuses new clients; `tb kill`
      ends it, and attaching restores it.
    - **Also bindable,** because the user's zellij config uses them: `BreakPane`, `BreakPaneLeft`,
      `BreakPaneRight`, `ToggleTab`, `Search` and `SearchToggleOption`. Loading the user's whole zellij
      `config.kdl` gives 342 bindings and 24 warnings. The warnings are the ignored `clear-defaults`, and
      bindings that use plugins, floating, stacked or pinned panes, swap layouts, pane groups, or the rename
      and search-input modes.
  - **Known gaps:**
    - `rename-session` leaves the old `TB_SESSION` in panes that are already running.
    - `switch-session` moves the client that typed last.
    - `dump-screen --ansi` writes basic SGR (styles, 16/256/RGB colours), not hyperlinks.
    - Floating-pane actions come with step 8.
