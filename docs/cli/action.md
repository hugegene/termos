<!-- Generated from tb's command definitions: edit the help text in crates/tb/src, then run
     TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs -->

# `tb action`

Change the session from scripts: zellij's actions (same names and flags) and tb's own

```
tb action [OPTIONS] <COMMAND>
```

Also: `ac`.

## Options

| Option | |
|---|---|
| `-s, --session <SESSION>` | Session name (default: the session you're in, else "main"). |

## Commands

| Command | Does |
|---|---|
| [`new-tab`](action/new-tab.md) | Open a new tab running a shell, or COMMAND; prints the tab's id |
| [`close-tab`](action/close-tab.md) | Close a tab and everything in it |
| [`close-tab-by-id`](action/close-tab-by-id.md) | Close the tab with this id |
| [`go-to-tab`](action/go-to-tab.md) | Go to tab N (counting from 1) |
| [`go-to-tab-by-id`](action/go-to-tab-by-id.md) | Go to the tab with this id |
| [`go-to-tab-name`](action/go-to-tab-name.md) | Go to the tab with this name |
| [`go-to-next-tab`](action/go-to-next-tab.md) | Go to the next tab (wrapping around) |
| [`go-to-previous-tab`](action/go-to-previous-tab.md) | Go to the previous tab (wrapping around) |
| [`rename-tab`](action/rename-tab.md) | Rename a tab |
| [`rename-tab-by-id`](action/rename-tab-by-id.md) | Rename the tab with this id |
| [`undo-rename-tab`](action/undo-rename-tab.md) | Remove a tab's name, so it follows its program again |
| [`move-tab`](action/move-tab.md) | Move a tab one place left or right |
| [`toggle-active-sync-tab`](action/toggle-active-sync-tab.md) | Type into every pane of a tab at once, or stop |
| [`list-tabs`](action/list-tabs.md) | List tabs |
| [`query-tab-names`](action/query-tab-names.md) | Print each tab's name |
| [`current-tab-info`](action/current-tab-info.md) | The active tab |
| [`new-pane`](action/new-pane.md) | Open a pane running a shell, or COMMAND; prints the pane's id |
| [`split`](action/split.md) | Split the focused pane (tb's form of new-pane) |
| [`edit`](action/edit.md) | Open FILE in the editor in a new pane; prints the pane's id |
| [`close-pane`](action/close-pane.md) (`close`) | Close a pane |
| [`focus-pane-id`](action/focus-pane-id.md) | Focus a pane by id |
| [`focus-next-pane`](action/focus-next-pane.md) | Focus the next pane in the tab |
| [`focus-previous-pane`](action/focus-previous-pane.md) | Focus the previous pane in the tab |
| [`move-focus`](action/move-focus.md) | Focus the pane in a direction |
| [`move-focus-or-tab`](action/move-focus-or-tab.md) | Focus the pane in a direction, or the next tab at the edge |
| [`focus`](action/focus.md) | Focus a pane: left, right, up, down, next, prev, or an id (tb's form) |
| [`move-pane`](action/move-pane.md) | Swap a pane with its neighbour in a direction, or with the next pane |
| [`move-pane-backwards`](action/move-pane-backwards.md) | Swap a pane with the previous pane |
| [`rename-pane`](action/rename-pane.md) | Name a pane (shown in its frame) |
| [`undo-rename-pane`](action/undo-rename-pane.md) | Remove a pane's name |
| [`toggle-fullscreen`](action/toggle-fullscreen.md) (`zoom`) | Show a pane alone in its tab, or stop |
| [`toggle-pane-frames`](action/toggle-pane-frames.md) | Turn pane frames on or off |
| [`resize`](action/resize.md) | Resize a pane: `increase\|decrease [left\|right\|up\|down]` (zellij), or `DIRECTION [CELLS]` (tb) |
| [`clear`](action/clear.md) | Clear a pane's screen and scrollback |
| [`edit-scrollback`](action/edit-scrollback.md) | Open a pane's scrollback in the editor |
| [`set-pane-color`](action/set-pane-color.md) | Set a pane's default colours ("#00e000" or "rgb:00/e0/00"); default: the pane this runs in |
| [`scroll-up`](action/scroll-up.md) | Scroll a pane up one line |
| [`scroll-down`](action/scroll-down.md) | Scroll a pane down one line |
| [`page-scroll-up`](action/page-scroll-up.md) | Scroll a pane up a page |
| [`page-scroll-down`](action/page-scroll-down.md) | Scroll a pane down a page |
| [`half-page-scroll-up`](action/half-page-scroll-up.md) | Scroll a pane up half a page |
| [`half-page-scroll-down`](action/half-page-scroll-down.md) | Scroll a pane down half a page |
| [`scroll-to-top`](action/scroll-to-top.md) | Scroll a pane to the top of its scrollback |
| [`scroll-to-bottom`](action/scroll-to-bottom.md) | Scroll a pane back to the bottom |
| [`list-panes`](action/list-panes.md) | List panes |
| [`dump-screen`](action/dump-screen.md) | Print a pane's text |
| [`dump-layout`](action/dump-layout.md) | Print the session's layout (tb's saved-session JSON) |
| [`list-clients`](action/list-clients.md) | List attached clients |
| [`write`](action/write.md) | Send bytes, given as numbers (e.g. 27 91 65) |
| [`write-chars`](action/write-chars.md) | Type text into a pane |
| [`paste`](action/paste.md) | Paste text (bracketed, if the program asked for it) |
| [`send-keys`](action/send-keys.md) | Send keys, e.g. "Ctrl a" "F1" "Alt Shift b" Enter |
| [`switch-mode`](action/switch-mode.md) | Switch every client to a mode: normal, locked, pane, tab, resize, move, scroll, search, session, tmux |
| [`rename-session`](action/rename-session.md) | Rename the session |
| [`save-session`](action/save-session.md) | Save the session's layout now |
| [`detach`](action/detach.md) | Detach every attached client |
| [`switch-session`](action/switch-session.md) | Move your client to another session, starting it if needed |
| [`set-status`](action/set-status.md) | Show TEXT in the status bar (tb). Each --id is its own segment; clicks reach `tb subscribe` |
| [`clear-status`](action/clear-status.md) | Remove a status segment, or all of them (tb) |
| [`reload-config`](action/reload-config.md) | Read ~/.config/tb/config.kdl again and print what it skipped (tb) |

See also: [all commands](README.md) · [the overview](../cli.md)
