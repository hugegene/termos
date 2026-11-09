# Tab names follow the working folder

## Decision

A tab nobody has named shows the **last part of its working folder** instead of the program in the foreground.
The pane frame still shows the program, so you can see both.

- The folder belongs to the tab's focused pane: the shell's folder for a terminal, the shown folder for a finder.
- Your home folder shows as `~` and the root folder as `/`.
- Renamed tabs keep their name. `undo-rename-tab` (or an empty name in rename mode) goes back to following the
  folder.
- Finder tabs are still named `finder`, as before.

### Example

```
$ cd ~/terminal-browser/crates      tab bar:  1 crates           frame: ┌ bash
$ nvim Cargo.toml                   tab bar:  1 crates           frame: ┌ nvim
:q, then cd                         tab bar:  1 ~                frame: ┌ bash
Ctrl+T r  work  Enter               tab bar:  1 work             (named; cd no longer changes it)
```

Before this change, the first two lines showed `1 bash` and `1 nvim`.

### Where the name comes from

```
 tb server                                    kernel
┌──────────────────────────────┐           ┌──────────────────────────┐
│ draw_tab_bar (each redraw,   │ 1 readlink│ /proc/<shell pid>/cwd    │
│ at least every 500 ms)       │──────────▶│  -> /home/u/tb/crates    │
│   tab_label(tab)             │◀──────────│                          │
│     auto_name? -> pane_cwd   │ 2 path    └──────────────────────────┘
│     -> folder_label: "crates"│
└──────────────────────────────┘
```

The shell's folder changes only when the shell runs `cd`, and the shell then prints a prompt, which triggers a
redraw. The existing 500 ms redraw covers anything else.

## Options considered

- **Folder only (chosen):** this is what was asked for, and the frame already shows the program.
- **Folder, plus the program while something other than the shell runs** (`crates · nvim`): more information,
  but tabs get wider. Easy to add later if wanted.
- **The full path, starting at home** (`~/terminal-browser/crates`): too wide for a tab bar.

## Steps

- [x] `pane_cwd(id)` in `server/mod.rs` gives the folder of any pane; `focused_cwd` uses it.
- [x] `tab_label` uses `folder_label(pane_cwd(tab.focus))` for tabs that follow, and falls back to the stored
  name ("shell") if the folder can't be read.
- [x] Wording: `undo-rename-tab` help, `docs/cli.md` (`auto_name`, the tab table), and regenerated `docs/cli/`.
- [x] Tests: a unit test for `folder_label`; the e2e test `tabs_are_named_after_the_running_program` becomes
  `tabs_are_named_after_the_working_folder` (`cd` changes the tab; a running program changes only the frame;
  home shows `~`).
