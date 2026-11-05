# Plan: rebuild step 6, the CLI as the extension API

**Status:** planned · **Date:** 2026-11-05 · **Part of:** [rebuild plan](2026-11-03-rebuild-plan.md) §11, step 6 ·
**Follows:** [zellij's session commands](2026-11-04-zellij-cli.md) (built) · **Findings:**
[zellij's actions](../findings/2026-11-05-zellij-actions.md), [zellij's session commands](../findings/2026-11-04-zellij-cli.md)

## Decision

**Other people extend `tb` by writing programs that call its CLI**, the way `prbot` (`~/claudehook`) drives
zellij today. There is no plugin system. `tb`'s CLI becomes a stable, documented API with six parts:

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

**Safety stays as it is.**
- Only the session's own user can connect: the socket is `0600`, and the server checks the peer's uid.
- Extensions run with the user's rights, in their own process, so a buggy extension can't take sessions down.
- Nothing listens on the network; that's the daemon's job.

**Acceptance test:** a copy of `prbot`, with only `zellij` → `tb` and the variable names changed, opens,
renames, finds and closes its tabs in a throwaway `tb` session.

**Size:** about 2–4k lines, **estimate**. The event stream is the largest new piece.

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

## Steps

- [ ] Measure each zellij action's arguments, output and JSON fields, in a throwaway `tftest-*` session, and
  record a finding.
- [ ] Tab and pane ids in every output; accept `terminal_N`; creating commands print the new id.
- [ ] **Tabs:**
  - `list-tabs`, `query-tab-names`, `current-tab-info`
  - `close-tab`, `close-tab-by-id`
  - `go-to-tab-by-id`, `go-to-tab-name`, `go-to-next-tab`, `go-to-previous-tab`
  - `rename-tab-by-id`, `undo-rename-tab`
  - `move-tab`, `toggle-active-sync-tab`
- [ ] **Panes:**
  - `new-pane` (zellij's flags) and `run`; `edit`
  - `move-pane`, `move-pane-backwards`, `move-focus-or-tab`
  - `rename-pane`, `undo-rename-pane`
  - `toggle-pane-frames`, `clear`, `edit-scrollback`, `set-pane-color`
  - zellij's forms of `new-tab`, `resize`, `dump-screen` and `list-panes`
- [ ] **Scrolling:** `scroll-up`/`scroll-down`, `page-scroll-*`, `half-page-scroll-*`, `scroll-to-top`,
  `scroll-to-bottom`.
- [ ] **Input:** `write` (bytes), `paste` (bracketed), `send-keys`.
- [ ] **Session and clients:** `list-clients`, `switch-mode`, `rename-session`, `save-session`,
  `switch-session`, `watch`.
- [ ] **Events:** `subscribe`.
- [ ] **API:** the version, the stability rules, `docs/cli.md` and an example extension.
- [ ] **Tests:** end-to-end tests for each group of commands, plus the `prbot` acceptance run.

## Open questions

1. **Keys that start an extension**, for example "Ctrl+t p runs my extension". This needs key config,
   which is part of launch (step 9). Should a minimal `bind` come forward into this step?
2. **Extension UI beyond a pane**, for example a status-bar segment set with `tb action set-status`. Is that
   in scope?

## Change log

- **2026-11-05:** written. The user set the aim: other people extend `tb` through its CLI.
