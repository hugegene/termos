# Plan: a right-click menu in `tb`'s finder (rename, copy name, copy path, delete)

**Status:** built; the check by hand (step 6) is still to do · **Date:** 2026-11-07 · **Related:** [the finder as two panes](2026-11-05-finder-divider.md),
[hidden files](2026-11-07-finder-hidden-files.md) · **Findings:** [`gio trash`](../findings/2026-11-07-gio-trash.md)

## Decision

**Right-clicking a file or folder in the finder's file list selects it and opens a small menu at the pointer:
Rename, Copy name, Copy path, Delete.** `m` opens the same menu from the keyboard, for terminals that keep
right-clicks for themselves and for phones. The user asked for this on 2026-11-07.

```
 notes.md
 photo.jpg ┌─────────────┐
 todo.txt  │ Rename    r │
           │ Copy name y │
           │ Copy path p │
           │ Delete    d │   (Delete in red)
           └─────────────┘
```

- **Opening it:**
  - **Right-click an entry:** selects it and opens the menu just below the pointer, or above it if there's no
    room below. Right-clicking another entry while the menu is open moves the menu there.
  - **`m`** in the file list: opens the menu at the selected entry.
  - **Right-clicking an empty part of the list** opens nothing; nothing is selected there.
- **Using it:**
  - **Mouse:** the item under the pointer is highlighted. A left-click runs it. A click outside the menu only
    closes it, as in desktop menus, so that click doesn't also select a file.
  - **Keys:** `j`/`k` or `↓`/`↑` move, and `Enter` runs. Each item's letter runs it directly. `Esc`, `q` or `m`
    closes the menu.
  - These letters work only while the menu is open, so they don't take any keys from the list. `r` stays
    "reload" in the list.
- **Rename:** the entry's row becomes a text field holding the name, with the cursor at the end.
  - **Editing:** typing, `←`/`→`, `Home`/`End`, `Backspace`, `Delete`, and `Ctrl+U` to clear. `Enter` renames
    and `Esc` cancels. A click anywhere also cancels.
  - **Refused, with the reason on the status bar:** a name containing `/`, `.`, `..`, and a name that's already
    taken. Renames use `renameat2(RENAME_NOREPLACE)` (rustix, already a dependency), so an existing file is never
    replaced, even if it appears meanwhile. Plain `std::fs::rename` would replace it silently.
  - **An empty name or an unchanged name** just ends the edit.
  - Afterwards the list reloads with the renamed entry selected.
- **Copy name / Copy path:** copies the name, or the absolute path, to the clipboard with OSC 52, as copy mode
  does. The status bar says `copied 'notes.md'`, or the copied path.
- **Delete: moves the file or folder to the trash with `gio trash`.** It doesn't ask first, as GNOME Files
  doesn't, since the trash can restore it. The status bar says `moved 'x' to the trash`.
  - **Where there's no trash** (`gio` says "Trashing on system internal mounts is not supported" for `/` and
    `/tmp` on this machine) or `gio` is missing, a red box at the entry gives the reason and asks
    `Delete it for good? y / n`. For a folder it says `Delete the folder and everything in it for good?`.
  - **Only `y` deletes;** any other key or click cancels. tb never deletes for good without that `y`.
  - **A symlink** is removed itself, never what it points to (both by `gio` and by our fallback).
  - The selection stays at the same row, so it lands on the next file.
  - It runs in the server thread. `gio trash` took 10 ms for a file and for a folder.
  - **`TB_TRASH`** overrides the trash command, as `TB_EDITOR` overrides the editor: a command, with the path
    added at the end. The end-to-end tests set it so they never touch the user's trash.
- **Drawing:** the menu and the question box are drawn by the server on top of the panes.
  - They're moved left and up to fit inside the file list.
  - If the list is narrower than the menu, the menu spills over the preview. While it does, the preview's
    picture shows as blocks, as it does under the help screen. Real images can't be covered by text.
- **Hints:**
  - The list's status-bar hint gains `m menu`.
  - The `?` help screen's mouse line gains "right-click a finder's file for rename, copy and delete".
  - The README's finder line gains the same.
- **Not changed:**
  - The preview pane: no menu there.
  - The editor and the CSV grid: the list doesn't take clicks while they're open, as now.
  - The prototype (yazi has its own keys).
- **Size:** about 300 lines plus tests (**estimate**). It came to about 540 (see the change log).

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Opening | Right-click only | ❌ Some terminals keep right-click for their own menu, and phones have no right button |
| | **Right-click, plus `m`** | ✅ |
| | Also a direct list key per action (yazi's `r` `d` `y`) | Later, if wanted. `r` is "reload" in tb's list today |
| Delete | **`gio trash`** | ✅ 10 ms; writes standard `.trashinfo`, so the desktop's Trash restores it; no new crate ([finding](../findings/2026-11-07-gio-trash.md)) |
| | The freedesktop trash spec in our own code | ❌ More code, and `gio` already handles other mounts' `.Trash-$uid` |
| | The `trash` crate | ❌ A new dependency (`cargo fetch`) for what `gio` does |
| | Delete for good, after a question | ❌ As the default; ✅ only where there's no trash |
| Asking before trashing | **No; it can be restored** | ✅ As GNOME Files |
| | Always | ❌ A question for every delete, when the trash can restore it |
| Rename | **Inline, in the entry's row** | ✅ Where you're looking, as in desktop file managers |
| | On the status bar, like "rename tab" | ❌ Far from the file |
| Existing name | **Refuse (`RENAME_NOREPLACE`)** | ✅ Never loses a file |
| | Ask to replace | Later, if wanted |
| Where the menu is drawn | Inside the list pane only, cut off if it doesn't fit | ❌ Items could go missing |
| | **On top of everything, moved to fit the list** | ✅ |

## Steps

- [x] **1. The menu** (`crates/tb/src/server/finder/menu.rs`): its items, its position, keys, mouse and drawing.
  - `Finder` gains a `popup` state: the menu, the rename field, or the delete question.
  - `handle_mouse` opens it on `Down(Right)` and passes events to it while it's open, `Moved` included.
  - `handle_key` sends keys to it first.
- [x] **2. The actions:**
  - **Rename:** `renameat_with(…, NOREPLACE)`, then reload and select the new name.
  - **Copy:** a new `Action::Copy(String)`, which the server sends with `copy_to_clipboard`.
  - **Delete:** the trash command (`TB_TRASH` or `gio trash --`). On failure, the question; on `y`,
    `remove_file` or `remove_dir_all`, using `symlink_metadata` so links are removed themselves.
- [x] **3. Drawing:** the server draws the focused finder's popup after the panes. Real images are off while it
  overlaps the preview.
- [x] **4. Tests:**
  - **Unit:**
    - a right-click opens the menu and selects the entry
    - `Esc` and a click outside close it
    - a rename works and refuses `a/b`, `..` and a taken name
    - copy gives the name and the absolute path
    - delete runs the trash command
    - a failing trash command asks, `n` keeps the file, `y` deletes it
    - a folder is deleted with its contents, and a symlink without its target
  - **End to end:** right-click a file and see the menu; `r`, type a new name, and the list shows it.
    `TB_TRASH=false` makes Delete ask; `y` removes the file.
- [x] **5. Docs:** the README's finder line, the `?` help, and a change-log line in the
  [finder plan](2026-11-05-finder-divider.md).
- [ ] **6. Check by hand (you):** right-click in your terminal. Does it reach tb, or does the terminal open its
  own menu? Rename a file, copy a path and paste it, delete a file in your home folder and find it in the trash.

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tb -p finder-core`.

## Change log

- **2026-11-07:** written. The user asked for a right-click menu with rename, copy filename and delete.
- **2026-11-07:** built (steps 1–5); step 6, the check by hand, is still to do.
  - **Size:** about 480 lines in `finder/menu.rs` (including comments) and 60 elsewhere, plus about 250 lines
    of unit tests. That's well over the 300 estimated. The question box's word wrap, the rename field and the
    mouse handling each took more than expected.
  - **Code:**
    - `finder/menu.rs` holds `Popup` (the menu, the rename field, the question), its keys, mouse, drawing and
      actions.
    - `Finder` gains `popup` and `trash`, which holds the trash command so unit tests can swap it.
    - `Action::Copy` goes to `Server::copy_to_clipboard`.
    - The server draws the focused list's popup after the panes. It turns real images off while the popup
      covers the preview, and closes the popup in `focus_changed`.
  - **Tests:**
    - **Unit** (`finder::menu::tests`):
      - placing the boxes, word wrap, the field
      - a right-click, Esc, a click outside, an empty row
      - hover and a click on an item; keys
      - rename, plus refused names and Esc
      - trash, the question (`n`, then `y`), and a folder and a symlink deleted for good
    - **End to end:** `finder_menu_renames_and_deletes` right-clicks a file and renames it. Then, with
      `TB_TRASH=false`, it deletes the file with `m d y`.
    - None of the tests uses the real trash.
  - **Changed while building:**
    - **The question box** is at least 30 columns wide rather than 40. At 40 it spilled 5 columns over the
      preview on a 100-column screen, where the list is 35 wide.
    - **Wording:** the box says "Can't move 'x' to the trash: <reason>. Delete it for good? y / n", and names
      the file once.
