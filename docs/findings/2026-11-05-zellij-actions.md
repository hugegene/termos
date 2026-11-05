# Finding: zellij's `action` commands and other CLI commands (2026-11-05)

## Question

The user wants `tb`'s CLI to replicate zellij's, so scripts such as `prbot` (`~/claudehook`) can drive `tb`
the way they drive zellij. Which commands does zellij's CLI have, and which do `prbot`'s scripts use? The
scope of rebuild step 6 ([zellij's CLI plan](../plans/2026-11-04-zellij-cli.md)) depends on it. The session
commands were recorded on 2026-11-04 in [2026-11-04-zellij-cli.md](2026-11-04-zellij-cli.md).

## Method

On the dev machine (Ubuntu, Linux 6.8), on 2026-11-05, with the user's zellij:

```sh
/snap/bin/zellij help action                     # the list of action subcommands
for c in run edit watch subscribe pipe plugin web; do /snap/bin/zellij help $c | sed -n '2,3p'; done
# which commands prbot uses:
grep -ohE '(action|zj|zellij_action)\(\s*\[?"[a-z-]+"|"action", "[a-z-]+"|action [a-z-]+' \
  ~/claudehook/prbot.py ~/claudehook/bin/*
```

Only help text and `prbot`'s source were read; the actions' arguments and output formats weren't checked.

## Results

`zellij 0.44.1`: `zellij help action` lists **72 actions** (73 entries including `help`):

```
are-floating-panes-visible  change-floating-pane-coordinates  clear  close-pane  close-tab  close-tab-by-id
current-tab-info  detach  dump-layout  dump-screen  edit  edit-scrollback  focus-next-pane  focus-pane-id
focus-previous-pane  go-to-next-tab  go-to-previous-tab  go-to-tab  go-to-tab-by-id  go-to-tab-name
half-page-scroll-down  half-page-scroll-up  hide-floating-panes  launch-or-focus-plugin  launch-plugin
list-clients  list-panes  list-tabs  move-focus  move-focus-or-tab  move-pane  move-pane-backwards  move-tab
new-pane  new-tab  next-swap-layout  override-layout  page-scroll-down  page-scroll-up  paste  pipe
previous-swap-layout  query-tab-names  rename-pane  rename-session  rename-tab  rename-tab-by-id  resize
save-session  scroll-down  scroll-to-bottom  scroll-to-top  scroll-up  send-keys  set-pane-borderless
set-pane-color  show-floating-panes  stack-panes  start-or-reload-plugin  switch-mode  switch-session
toggle-active-sync-tab  toggle-floating-panes  toggle-fullscreen  toggle-pane-borderless
toggle-pane-embed-or-floating  toggle-pane-frames  toggle-pane-pinned  undo-rename-pane  undo-rename-tab
write  write-chars
```

Selected descriptions, as printed:

```
new-pane         Open a new pane in the specified direction [right|down] If no direction is specified, will try
                 to use the biggest available space. Returns: Created pane ID (format: terminal_<id> or plugin_<id>)
new-tab          Create a new tab, optionally with a specified tab layout and name
go-to-tab-by-id  Go to tab with stable ID
list-tabs        List all tabs with their information
send-keys        Send one or more keys to the terminal (e.g., "Ctrl a", "F1", "Alt Shift b")
switch-mode      Switch input mode of all connected clients [locked|pane|tab|resize|move|search|session]
```

Other top-level commands (beyond the session commands recorded on 2026-11-04):

```
run        Run a command in a new pane Returns: Created pane ID (format: terminal_<id>)
edit       Edit file with default $EDITOR / $VISUAL Returns: Created pane ID (format: terminal_<id>)
watch      Watch a session (read-only)
subscribe  Subscribe to pane render updates (viewport and scrollback)
pipe       Send data to one or more plugins, launch them if they are not running
plugin     Load a plugin Returns: Created pane ID (format: plugin_<id>)
web        Run a web server to serve terminal sessions
```

**What `prbot` uses**, with counts:

```
3 rename-tab-by-id   2 query-tab-names   2 close-tab-by-id   1 new-tab   1 list-tabs   1 list-panes   1 go-to-tab-by-id
```

- `prbot.py`'s `spawn()` runs `new-tab -n <name> -c <dir> -- bin/run-claude …`, then `go-to-tab-by-id` back to
  the tab that had focus.
- It finds its own tab from `ZELLIJ_PANE_ID` and `ZELLIJ_SESSION_NAME`, through `list-panes -j -t`, which it
  reads for `tab_id`, `id` and `is_plugin`. It reads `list-tabs` for `tab_id`, `name` and `active`.

## Conclusions

- **Already in `tb`:** 14 of the 72 actions have a `tb` equivalent today (some with different argument
  forms):
  - `new-tab`, `close-pane`, `go-to-tab`, `rename-tab`, `resize`, `toggle-fullscreen`, `write-chars`,
    `detach`, `list-panes`, `dump-screen`
  - `move-focus`, `focus-next-pane`, `focus-previous-pane`, `focus-pane-id`, through `focus`
- **Possible now:** 41 more map onto features `tb` already has (tabs, panes, scrolling, modes, sync, frames,
  renaming, persistence) and need only the CLI. These include the tab-by-id commands `prbot` needs.
- **Wait for missing features:** 12 need features `tb` doesn't have yet (floating, stacked and pinned panes,
  swap layouts, borderless panes).
- **Not copied:** 5 depend on plugins or layout files.
- `prbot` also depends on zellij's JSON field names (`tab_id`, `name`, `active`, `id`, `is_plugin`) and on
  `ZELLIJ_*` environment variables.
- **Unknown, to measure when the step starts:** each action's exact arguments, output format and JSON
  fields.
