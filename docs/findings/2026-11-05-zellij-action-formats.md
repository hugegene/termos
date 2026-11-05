# Finding: zellij's action arguments and output formats (2026-11-05)

## Question

Rebuild step 6 ([CLI extension plan](../plans/2026-11-05-cli-extensions.md)) copies zellij's actions into `tb`,
with the same arguments, flags, output and JSON field names. What exactly are they? The list of actions is in
[2026-11-05-zellij-actions.md](2026-11-05-zellij-actions.md).

## Method

On the dev machine, with the user's zellij 0.44.1 (snap), on 2026-11-05:

- **Help text:**
  - `zellij action help <action>` for each of the 72 actions
  - `zellij help run|edit|watch|subscribe`
  - The output was saved, and a script listed each action's arguments and flags.
- **Real output:** throwaway background sessions (`tftest-cli`, `tftest-cli4`), deleted afterwards:

  ```sh
  zellij attach --create-background tftest-cli
  zellij -s tftest-cli action new-tab -n two
  zellij -s tftest-cli action new-pane -d right -n side -- sleep 300
  zellij -s tftest-cli action query-tab-names | list-tabs | list-tabs -j | list-tabs -a -j
  zellij -s tftest-cli action current-tab-info [-j] | list-panes [-j] [-t -s -c] | list-clients
  zellij kill-session tftest-cli; zellij delete-session tftest-cli --force
  ```

  - Output had to go through a pipe (`| cat`). With a file redirect, `list-panes -j` printed nothing, twice.
  - No client was attached, so `current-tab-info` only answered "No active tab found for current client".

## Results

**Arguments and flags**, trimmed to the commands `tb` will copy (`<X>...` means one or more):

```
new-tab        [<INITIAL_COMMAND>...]  -n/--name -c/--cwd -l/--layout --close-on-exit --start-suspended
                                       --block-until-exit[-success|-failure] --layout-dir --layout-string --initial-plugin
new-pane / run [<COMMAND>...]          -d/--direction -n/--name --cwd -c/--close-on-exit -s/--start-suspended
                                       -f/--floating -i/--in-place --stacked --near-current-pane --tab-id
                                       -x -y --width --height --pinned --borderless -b/--blocking --block-until-exit[...]
                                       -p/--plugin --configuration --skip-plugin-cache --close-replaced-pane
edit           <FILE>                  -l/--line-number -d/--direction --cwd -f/--floating -i/--in-place --tab-id ...
close-pane                             -p/--pane-id
close-tab                              -t/--tab-id        close-tab-by-id <ID>
go-to-tab <INDEX>    go-to-tab-by-id <ID>    go-to-tab-name <NAME> [-c/--create]    go-to-next-tab    go-to-previous-tab
rename-tab <NAME> [-t/--tab-id]        rename-tab-by-id <ID> <NAME>     undo-rename-tab [-t/--tab-id]
move-tab <DIRECTION> [-t/--tab-id]     toggle-active-sync-tab [-t/--tab-id]
list-tabs  -a/--all -d/--dimensions -j/--json -l/--layout -p/--panes -s/--state
list-panes -a/--all -c/--command -g/--geometry -j/--json -s/--state -t/--tab
current-tab-info [-j/--json]           query-tab-names                  list-clients
focus-pane-id <PANE_ID>   focus-next-pane   focus-previous-pane   move-focus <DIRECTION>   move-focus-or-tab <DIRECTION>
move-pane <DIRECTION> [-p]   move-pane-backwards [-p]   rename-pane <NAME> [-p]   undo-rename-pane [-p]
toggle-fullscreen [-p]    toggle-pane-frames    clear [-p]    edit-scrollback [-p] [-a/--ansi]
resize <RESIZE> <DIRECTION> [-p]       (RESIZE: increase | decrease, as in zellij's keybinds)
dump-screen [-p] [-f/--full] [-a/--ansi] [--path <PATH>]
scroll-up | scroll-down | page-scroll-up | page-scroll-down | half-page-scroll-up | half-page-scroll-down
  | scroll-to-top | scroll-to-bottom                   [-p]
write <BYTES>... [-p]    write-chars <CHARS> [-p]    paste <CHARS> [-p]    send-keys <KEYS>... [-p]
set-pane-color [-p] --fg <COLOR> --bg <COLOR> --reset  (colours like "#00e000" or "rgb:00/e0/00")
switch-mode <INPUT_MODE>   switch-session <NAME> [-c/--cwd ...]   rename-session <NAME>   save-session   detach
watch <SESSION_NAME>       subscribe [-p/--pane-id] [-f/--format] [-s/--scrollback] [--ansi]
```

**Defaults, from the help text:**
- `-p/--pane-id` accepts `terminal_1`, `plugin_2`, or a bare `3` (meaning `terminal_3`).
- `set-pane-color` defaults to `$ZELLIJ_PANE_ID`; the other pane actions act on the focused pane.
- `send-keys` takes "space-separated strings", for example `"Ctrl a"`, `"F1"`, `"Alt Shift b"`.

**Output:**

```
$ zellij action new-tab -n two
1
$ zellij action new-pane -d right -n side -- sleep 300
terminal_2
$ zellij action query-tab-names
Tab #1
two
$ zellij action list-tabs
TAB_ID  POSITION  NAME
0  0  Tab #1
1  1  two
$ zellij action list-panes
PANE_ID  TYPE  TITLE
plugin_0  plugin  (.) - zellij:link
terminal_0  terminal  hugegene@hugegene-GM5AR7O: /tmp
$ zellij action list-panes -t -s -c
TAB_ID  TAB_POS  TAB_NAME  PANE_ID  TYPE  TITLE  COMMAND  CWD  FOCUSED  FLOATING  EXITED
0  0  Tab #1  terminal_0  terminal  hugegene@hugegene-GM5AR7O: /tmp  /bin/bash  /tmp  true  false  false
$ zellij action list-clients
CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND
```

**`list-tabs -j`**: an array. Each tab has these fields, the same with or without `-a`:

```
position name active panes_to_hide is_fullscreen_active is_sync_panes_active are_floating_panes_visible
other_focused_clients active_swap_layout_name is_swap_layout_dirty viewport_rows viewport_columns
display_area_rows display_area_columns selectable_tiled_panes_count selectable_floating_panes_count tab_id
has_bell_notification is_flashing_bell
```

**`list-panes -j`**: an array, plugin panes included. One terminal pane:

```json
{"id": 0, "is_plugin": false, "is_focused": true, "is_fullscreen": false, "is_floating": false,
 "is_suppressed": false, "title": "hugegene@hugegene-GM5AR7O: /tmp", "exited": false, "exit_status": null,
 "is_held": false, "pane_x": 0, "pane_content_x": 1, "pane_y": 1, "pane_content_y": 2, "pane_rows": 48,
 "pane_content_rows": 46, "pane_columns": 50, "pane_content_columns": 48, "cursor_coordinates_in_pane": null,
 "terminal_command": null, "plugin_url": null, "is_selectable": true, "index_in_pane_group": {},
 "default_fg": null, "default_bg": null, "tab_id": 0, "tab_position": 0, "tab_name": "Tab #1",
 "pane_command": "/bin/bash", "pane_cwd": "/tmp"}
```

**Ids:** tab ids and pane ids both start at 0. A tab's id stayed the same as its position here, but they're
separate fields.

## Conclusions

- **Ids:** `tb` prints new ids the way zellij does: a bare tab id from `new-tab`, and `terminal_<id>` from
  `new-pane`, `run` and `edit`.
- **Text output:** `list-tabs` and `list-panes` print zellij's columns, so scripts that read the text work
  too.
- **JSON:** `tb` emits zellij's field names with `tb`'s values. Fields for features `tb` lacks get fixed values
  (`is_floating: false`, `is_plugin: false`, `panes_to_hide: 0`, `active_swap_layout_name: null`). The finder
  pane, which zellij has no type for, keeps `is_plugin: false` and adds `"kind": "finder"`.
- **One clash: `write`.** zellij's `write` takes bytes, while `tb`'s `write` types characters (zellij's
  `write-chars`). `tb`'s command becomes `write-chars`, and `write` takes bytes like zellij's.
- **One shape to reconcile: `resize`.** zellij's is `resize increase left`, `tb`'s is `resize left 2`. Both
  forms are accepted.
- **Unknown:**
  - `current-tab-info`'s format with a client attached. `tb` will print the same line as `list-tabs`.
  - Which zellij keys `send-keys` names exactly beyond the examples. `tb` will take the key names used in
    zellij's config files.
