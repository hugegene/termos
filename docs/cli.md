# `tb`'s command line: the extension API

Other people extend `tb` by writing programs that call its command line, in any language. `tb` has no plugin
system. An extension is an ordinary program that:
- **acts** with `tb action …`: zellij's actions, with the same names and flags
- **reads** state as JSON (`list-tabs -j`, `list-panes -j`)
- **reacts** to what happens with `tb subscribe`
- **shows** text in the status bar with `tb action set-status`

A user starts it from a key with a `Run` binding in `~/.config/tb/config.kdl`.

zellij scripts port by changing `zellij` to `tb` and `ZELLIJ_SESSION_NAME`/`ZELLIJ_PANE_ID` to
`TB_SESSION`/`TB_PANE_ID`.

**[docs/cli/](cli/README.md) has a page for every command,** with its usage, options and examples. The pages
are generated from the same definitions as `tb … --help`. This page covers what spans commands.

## Stability

`tb --version` prints the extension API's version: `tb 0.1.1 (extension API 1)`. Within one API version,
commands, flags, JSON fields and events are only ever **added**. They are never renamed, removed or changed in
meaning. Failures exit non-zero with the message on stderr.

## Which session

| Where the extension runs | How `tb` finds the session |
|---|---|
| In a `tb` pane | `TB_SESSION` is set in every pane, so plain `tb action …` works. `TB_PANE_ID` names the pane it runs in |
| Anywhere else | `tb -s NAME action …` (also `tb action -s NAME …`). Without either, the session is "main" |

## Ids

- **Tabs and panes have ids that never change while they exist.** Tab numbers and positions change as tabs
  open, close and move; ids don't.
- **Finder tabs come first and have no number.** A tab that holds a finder always sits left of the other tabs.
  The tab bar numbers the other tabs from 1, and `go-to-tab N` (and `GoToTab N` in `config.kdl`) uses those
  numbers. `position` in JSON and events counts every tab from 0, finders included.
- **Creating prints the new id:** `new-tab` prints a tab id (`3`), and `new-pane`, `run` and `edit` print a
  pane id (`terminal_7`).
- **Pane ids are accepted as `7`, `terminal_7`, `finder_7` or `preview_8`.** zellij has neither of the last
  two kinds.
  - **A finder is two panes:** its file list (kind `finder`) and its preview (kind `preview`).
  - **Closing either closes both.**
  - **Keys you send to either one** go to the finder, as if typed there.

## Commands

### Sessions

| Command | Does |
|---|---|
| `tb attach [NAME]` (`a`), `tb -s NAME` | Attach, starting the session if needed. `-b` starts it without attaching, `--forget` drops its saved layout first, `-f` runs restored commands, `-c` is accepted |
| `tb start [NAME]` | Start without attaching |
| `tb list-sessions` (`ls`) | zellij's shape: `NAME [2 tabs, 1 attached] (current)`, `NAME (EXITED - attach to restore)`. `-s` names only, `-r` reversed, `-n` accepted |
| `tb kill-session [NAME]` (`k`), `tb kill-all-sessions` (`ka`) | End sessions; layouts are kept for next time. `-y` skips the question |
| `tb delete-session [NAME]` (`d`), `tb delete-all-sessions` (`da`) | Forget saved layouts. `-f` also ends running sessions |
| `tb watch [NAME]` (`w`) | Show a session read-only; `q` or Ctrl+C stops |
| `tb run [-d right\|down] [-n NAME] [--cwd DIR] [-s] [--] COMMAND…` (`r`) | A new pane running COMMAND; prints its id. `-s` types it at a shell's prompt without running it |
| `tb edit [-l LINE] [-d DIR] [--cwd DIR] FILE` (`e`) | FILE in the editor in a new pane; prints its id |
| `tb subscribe` | Events, below |
| `tb finder [DIR]` | A finder tab |

### `tb action …` (`tb ac …`)

Pane actions take `-p/--pane-id` (default: the focused pane), and tab actions take `-t/--tab-id` (default: the
active tab).

**Tabs**

| Action | Does |
|---|---|
| `new-tab [-n NAME] [-c DIR] [--start-suspended] [--] [COMMAND…]` | A new tab; prints its id |
| `close-tab [-t ID]`, `close-tab-by-id ID` | Close a tab and everything in it |
| `go-to-tab N`, `go-to-tab-by-id ID`, `go-to-tab-name NAME [-c]` | Go to a tab; `-c` creates it if there's none |
| `go-to-next-tab`, `go-to-previous-tab` | Wrapping around |
| `rename-tab NAME [-t ID]`, `rename-tab-by-id ID NAME`, `undo-rename-tab [-t ID]` | Name a tab, or let it follow its folder again |
| `move-tab left\|right [-t ID]` | Move a tab one place |
| `toggle-active-sync-tab [-t ID]` | Type into every pane of a tab at once |
| `list-tabs [-j] [-s] [-p] [-d] [-l] [-a]` | zellij's columns, or JSON |
| `query-tab-names` | One name per line |
| `current-tab-info [-j]` | The active tab |

**Panes**

| Action | Does |
|---|---|
| `new-pane [-d right\|down] [-n NAME] [--cwd DIR] [-s] [--] [COMMAND…]` | A new pane (default: where there's most room); prints its id |
| `split right\|down [--cwd DIR] [-- COMMAND…]` | `tb`'s older form of `new-pane` |
| `edit FILE [-l LINE] [-d DIR] [--cwd DIR]` | As `tb edit` |
| `close-pane [-p ID]` (`close`) | Close a pane |
| `focus-pane-id ID`, `focus-next-pane`, `focus-previous-pane`, `move-focus DIR`, `move-focus-or-tab DIR` | Move focus |
| `focus TARGET` | `tb`'s form: left, right, up, down, next, prev or an id |
| `move-pane [DIR] [-p ID]`, `move-pane-backwards [-p ID]` | Swap a pane with a neighbour, or the next or previous pane |
| `rename-pane NAME [-p ID]`, `undo-rename-pane [-p ID]` | The name in the pane's frame |
| `toggle-fullscreen [-p ID]` (`zoom`) | Show a pane alone in its tab |
| `toggle-pane-frames` | Frames on or off |
| `resize increase\|decrease [DIR] [-p ID]`, `resize DIR [CELLS]` | zellij's form (5% steps), or `tb`'s (move the border by CELLS) |
| `clear [-p ID]` | Clear the screen and scrollback |
| `edit-scrollback [-p ID]` | Open the scrollback in the editor, in the pane's place |
| `set-pane-color [-p ID] [--fg COLOR] [--bg COLOR] [--reset]` | Default colours, `#00e000` or `rgb:00/e0/00`. Default pane: the one it runs in |
| `scroll-up`, `scroll-down`, `page-scroll-up`, `page-scroll-down`, `half-page-scroll-up`, `half-page-scroll-down`, `scroll-to-top`, `scroll-to-bottom` | `[-p ID]` |
| `list-panes [-j] [-t] [-c] [-s] [-g] [-a]` | zellij's columns, or JSON. `tb list-panes [-j] [-a]` too |
| `dump-screen [-p ID] [-f] [-a] [--path FILE]` | The pane's text; `-f` with scrollback, `-a` with colours. `tb dump-screen` too |
| `dump-layout` | The session's layout, as `tb` saves it (JSON) |
| `list-clients` | Attached clients |

**Input**

| Action | Does |
|---|---|
| `write-chars TEXT [-p ID] [--enter]` | Type text; `--enter` presses Enter after it (`tb`) |
| `write BYTE… [-p ID]` | Bytes as numbers: `write 27 91 65` |
| `paste TEXT [-p ID]` | Paste, bracketed if the program asked for it |
| `send-keys KEY… [-p ID]` | Keys as zellij names them: `"Ctrl a"`, `F1`, `"Alt Shift b"`, `Enter`, `PageDown` |

**Session**

| Action | Does |
|---|---|
| `switch-mode MODE` | normal, locked, pane, tab, resize, move, scroll, search, session, tmux |
| `switch-session NAME` | Move your client (the one that typed last) to another session, starting it if needed |
| `rename-session NAME` | Its socket, saved layout and log follow. Panes keep the old `TB_SESSION` until restarted |
| `save-session` | Save the layout now (it's also saved every 2 seconds) |
| `detach` | Detach every attached client |

**`tb`'s own**

| Action | Does |
|---|---|
| `set-status [--id ID] [--ttl SECS] TEXT` | Show TEXT at the right of the status bar. Each `--id` is one segment, so extensions share the bar. Default id: `default` |
| `clear-status [--id ID]` | Remove a segment, or all |
| `reload-config` | Read `config.kdl` again; prints what it skipped |

## JSON

**`list-tabs -j`**: an array. Each tab has zellij's fields:
- `tab_id`, `position`, `name`, `active`
- `is_fullscreen_active`, `is_sync_panes_active`
- `viewport_rows`, `viewport_columns`, `display_area_rows`, `display_area_columns`
- `selectable_tiled_panes_count`

Fields for features `tb` lacks have fixed values: `panes_to_hide` (0), `are_floating_panes_visible` (false),
`other_focused_clients` ([]), `active_swap_layout_name` (null), `is_swap_layout_dirty` (false),
`selectable_floating_panes_count` (0), `has_bell_notification` and `is_flashing_bell` (false).

`tb` adds `auto_name`: true while the name is automatic. A finder tab is then `finder`, or `finder: <folder>`
when there are several; another tab follows its folder (the last part of the focused pane's working folder,
or `~` for home).

**`list-panes -j`**: an array. Each pane has zellij's fields:
- `id`, `is_plugin` (false), `is_focused`, `is_fullscreen`, `title`
- `exited` (false) and `exit_status` (null); exits come as events instead
- `pane_x`, `pane_y`, `pane_rows`, `pane_columns`, and the same with `content_` (inside the frame)
- `cursor_coordinates_in_pane`: `[column, row]`
- `terminal_command`: the command it was started with, or null for a shell
- `pane_command`: what runs now
- `pane_cwd`, `default_fg`, `default_bg`
- `tab_id`, `tab_position`, `tab_name`

`tb` adds:
- `kind`: `terminal`, `finder` (a finder's file list) or `preview` (a finder's preview)
- `tab`: position from 1
- `name`: given with `rename-pane`
- `focused`, `visible`
- `x`, `y`, `cols`, `rows`
- `cwd`
- `editor_pane`: a finder's Neovim pane, on both of its panes; Neovim shows in the preview

**`list-clients`**: `client_id`, `pane_id`, `running_command`, `active`, `cols`, `rows`, `images`.

## Events: `tb subscribe`

```
tb subscribe [-f raw|json] [-p PANE…] [-s [N]] [--ansi]
```

`tb subscribe` prints one JSON object per line as things change, until the session ends.
- **Format:** with `-f json`, everything is JSON. With `raw` (the default), pane text prints as plain lines
  and other events as JSON.
- **Pane text:** `-p` also sends those panes' text: once at the start (with `-s`, plus all of the scrollback,
  or its last N lines), then whenever it changes. A subscription to panes ends when they've all closed.

| Event | Fields |
|---|---|
| `tab_opened` | `tab_id`, `name`, `position` |
| `tab_closed` | `tab_id` |
| `tab_renamed` | `tab_id`, `name` |
| `tab_moved` | `tab_id`, `position` |
| `tab_focused` | `tab_id` |
| `pane_opened` | `pane_id`, `tab_id`, `kind`, `title` |
| `pane_closed` | `pane_id`, `tab_id`, `exit_status` (the program's status, 128 + signal if killed, or null) |
| `pane_retitled` | `pane_id`, `title` |
| `pane_renamed` | `pane_id`, `name` |
| `pane_moved` | `pane_id`, `tab_id` (to another tab) |
| `pane_focused` | `pane_id` |
| `pane_text` | `pane_id`, `lines` |
| `mode_changed` | `mode` |
| `client_attached`, `client_detached` | `client_id` |
| `status_clicked` | `id` (the segment), `button` (`left`, `right`, `middle`) |

**How events are made:**
- They are differences from the session as it was when you subscribed: nothing is replayed. Read the current
  state with `list-tabs -j` and `list-panes -j`.
- Changes are collected for up to 0.1 seconds. Something that appears and disappears within that time may
  not be reported.

## Key bindings: `~/.config/tb/config.kdl`

The file is `$TB_CONFIG` if set, else `$XDG_CONFIG_HOME/tb/config.kdl`, else `~/.config/tb/config.kdl`. It's
read when a session starts, and by `tb action reload-config`. It uses zellij's keybind syntax, so zellij
`bind` blocks copy across:

```kdl
keybinds {
    tab {
        bind "p" { Run "prbot" "ls" { cwd "/home/me/claudehook"; direction "Down"; name "prbot"; }; SwitchToMode "Normal"; }
    }
    shared_except "locked" {
        bind "Alt y" { WriteChars "make test"; Write 13; }
    }
}
```

- **Blocks:**
  - modes: `normal`, `locked`, `pane`, `tab`, `resize`, `move`, `scroll`, `search`, `session`, `tmux`
  - `shared`, `shared_except "mode"…` and `shared_among "mode"…`
- **A binding replaces `tb`'s own key in that mode.** `clear-defaults=true` is ignored: `tb` keeps its keys.
- **Actions:**
  - `Run "cmd" "arg"…`, with `cwd`, `direction`, `name`, `start_suspended` and `close_on_exit`
  - `NewTab`, with `name` and `cwd`
  - `SwitchToMode`, `NewPane`, `CloseFocus`, `CloseTab`
  - `MoveFocus`, `MoveFocusOrTab`, `FocusNextPane`, `FocusPreviousPane`, `SwitchFocus`
  - `GoToTab`, `GoToNextTab`, `GoToPreviousTab`, `ToggleTab`
  - `ToggleFocusFullscreen`, `TogglePaneFrames`, `ToggleActiveSyncTab`
  - `Resize`, `MovePane`, `MovePaneBackwards`, `MoveTab`
  - `BreakPane`, `BreakPaneLeft`, `BreakPaneRight`
  - `Clear`, `EditScrollback`
  - the eight scroll actions, `Search`, `SearchToggleOption`
  - `UndoRenamePane`, `UndoRenameTab`
  - `Write`, `WriteChars`, `Detach`, `Quit` (asks first)
- **Skipped, with a warning naming the line:** a binding that uses anything else (plugins, floating panes,
  swap layouts, pane groups). The warnings show briefly in the status bar when you attach, and in full from
  `tb action reload-config`.

## Not copied from zellij

- `attach --index`, and zellij's remote-session options.
- **Plugins:** `plugin`, `pipe`, `launch-plugin`, `launch-or-focus-plugin`, `start-or-reload-plugin`.
- **Layout files:** `override-layout`, `new-tab --layout`.
- **`web`:** the phone plan covers phones.
- **Config commands:** `setup`, `options`, `convert-*`, `list-aliases`.
- **Arriving with floating panes (rebuild step 8):** floating, stacked and pinned panes, swap layouts and
  borderless panes, and their 12 actions.

## Example extension

[`examples/exit-watcher.py`](examples/exit-watcher.py) shows a status-bar warning when a pane's command fails,
and goes back to its tab when you click the warning:

```sh
python3 docs/examples/exit-watcher.py          # inside a tb session
```

It subscribes with `tb subscribe --format json`, remembers pane titles from `pane_opened` and
`pane_retitled`, and on a `pane_closed` with a non-zero `exit_status` runs
`tb action set-status --id exit-watcher --ttl 30 "✗ make exited 2"`. On a `status_clicked` with that id, it
clears the warning and runs `tb action go-to-tab-by-id`.
