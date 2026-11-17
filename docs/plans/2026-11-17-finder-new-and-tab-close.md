# Plan: New file and New folder in the finder's menu (Open in app leaves it), and a × on every tab

**Status:** built (2026-11-17); the check by hand (step 5) is yours · **Date:** 2026-11-17 · **Related:**
[the finder's menu](2026-11-07-finder-menu.md), [`o` opens in an app](2026-11-11-grid-copy-and-open-in-app.md),
[finder tabs on the left](2026-11-09-finder-tabs-left.md) · **Findings:** none needed. Nothing here depends on a
fact we'd have to measure; the rest comes from reading the current code.

## Decision

The user asked on 2026-11-17: "can the finder right click remove the option of open in app, and add create new
folder and create new file? can each tab have a X to close the tab?"

### 1. The finder's menu

- **Open in app leaves the menu.** The `o` key stays: it still opens the file in its app from the list, the preview
  and the grid, as the captions say.
- **New file (`n`) and New folder (`N`) join it,** before Delete, which stays last and red. `N` for a folder
  echoes Ctrl+Shift+N, the desktop's "new folder" key.
- **Where the new entry goes:** the folder the list shows, whichever entry was right-clicked. That's what desktop
  file managers do in a list view.
- **Right-clicking empty space** in the list, below the last entry or in an empty folder, now opens a short menu
  with only New file and New folder. Before, it opened nothing; without this, an empty folder couldn't get its
  first file. `m` in an empty folder opens the same short menu.
- **The name:** a box at the row, titled `new file` or `new folder`, holds a text field. It starts empty.
  - The field takes the same keys as Rename: typing, `←`/`→`, `Home`/`End`, `Backspace`, `Delete`, `Ctrl+U`.
  - `Enter` creates it. `Esc`, `Ctrl+C` or a click outside the box cancels, and nothing is created.
  - **Refused, with the reason on the status bar,** as Rename refuses: a name with `/`, `.`, `..`, and a taken
    name (`'notes' already exists`).
  - **Nothing is ever replaced:** a file is made with `create_new`, a folder with `create_dir`. Both fail if the name
    appears meanwhile.
  - An empty name just closes the box.
  - Afterwards the list reloads with the new entry selected, and the status bar says `created 'todo.md'` or
    `created folder 'drafts'`. A new file isn't opened for editing; Enter in its preview edits it, as for any file.

```
 notes.md                        notes.md                                     notes.md
 photo.jpg                       photo.jpg                                    photo.jpg
 todo.txt ┌───────────────┐      todo.txt                                     plan.md   ◄ selected
          │ Rename      r │  n  ┌ new file ──────────────────┐ "plan.md",    todo.txt
          │ Copy name   y │ ──► │ plan.md▏                   │ ──Enter────►
          │ Copy path   p │     └────────────────────────────┘
          │ New file    n │
          │ New folder  N │
          │ Delete      d │   (in red)
          └───────────────┘
```

**Example:** `~/notes` is empty, so the list shows `(empty)`. You right-click under it:

1. The short menu opens at the pointer: `New file n`, `New folder N`.
2. You press `N` (or click `New folder`). A box titled `new folder` opens with an empty field.
3. You type `drafts` and press `Enter`. tos calls `create_dir("~/notes/drafts")`.
4. The list reloads and shows `drafts/`, selected. The status bar says `created folder 'drafts'`.

### 2. A × on every tab

- **Every tab shows ` ×` after its name:** ` finder ×   1 work ×   2 claude ×   + `. The × takes the tab's
  colours, so each tab reads as one block: `[ finder × ] [ 1 work × ] [ + ]`.
- **A left-click on the × closes that tab** and everything in it, as `Ctrl+t x` closes the active one. It doesn't
  ask first, as `Ctrl+t x` doesn't. Closing the last tab ends the session, as `Ctrl+t x` does.
- **A click on the rest of the tab** still switches to it, and `+` still opens a new one.
- **The click area** is the × and the space after it, so two cells.
- **A tab cut off at the right edge** gets no ×: there's no room for it.

```
 your terminal                        tos client              tos server
┌─────────────────────────┐ 1. press ┌────────────┐ 2. Mouse  ┌────────────────────────────────────────┐
│ finder ×   1 work ×   + │ ───────► │ forwards   │ ────────► │ finds tab_zones (19,21) = Close(1)     │
│                   ▲     │  col 19, │ it as is   │  event    │ 3. close_tab_at(1): its panes close    │
│             click │     │  row 0   └────────────┘  over the │ 4. next frame: " finder ×   + "        │
└─────────────────────────┘                          socket   └────────────────────────────────────────┘
```

**Example:** the tab bar is ` finder ×   1 work ×   + `. ` finder × ` is columns 0–9, then a gap at 10, then
` 1 work ` at 11–18 and `× ` at 19–20.

1. You click column 19, the × of `1 work`.
2. The server finds the zone `(19, 21, Close(1))` in the tab bar zones it kept from the last frame.
3. It runs `close_tab_at(1)`, which closes the shell; the finder tab becomes active.
4. The bar now reads ` finder ×   + `.

**Size:** about 150 lines plus tests (**estimate**).

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Open in app | **Leave the menu; the `o` key stays** | ✅ What was asked; `o` is in every caption already |
| | Also drop the `o` key | ❌ Not asked |
| Where a new entry goes | **The folder the list shows** | ✅ As desktop file managers' list views |
| | Into the right-clicked folder | ❌ Different for files and folders, and surprising |
| Typing the name | **A box at the row, empty, Enter creates** | ✅ Esc leaves nothing behind |
| | Make `untitled folder` at once, then rename it (macOS Finder) | ❌ Esc leaves a stray `untitled folder` |
| | A new row inside the list, as VS Code | ❌ Every row below shifts, and clicks, scrolling and selection all have to know about the extra row |
| Letters | **`n` file, `N` folder** | ✅ `N` echoes Ctrl+Shift+N |
| | `f` and `F` | ❌ `f` reads as "folder" as easily as "file" |
| Empty space | **Opens the short menu** | ✅ Otherwise an empty folder can't get its first file |
| Asking before closing a tab | **No, as `Ctrl+t x`** | ✅ Same as the key; browsers don't ask either |
| | Ask when a program is running (iTerm2, Ghostty) | Later, if wanted: a stray click on × loses that tab's programs |
| The × | **On every tab** | ✅ What was asked |
| | Only on the active tab and the one under the pointer (VS Code) | ❌ Not asked; saves width |
| Character | **`×` (U+00D7)** | ✅ One column in every font |
| | `✕` (U+2715) | ❌ Some terminals draw it two columns wide |

## Steps

- [x] **1. The menu** (`crates/tos/src/server/finder/menu.rs`):
  - `Item::OpenApp` goes; `Item::NewFile` and `Item::NewDir` come. The full menu has 6 items, the short one 2.
  - `Popup::Menu`'s `name` becomes an `Option`: `None` for the short menu.
  - A right-click on empty space in the list opens the short menu there. `m` with no entries does too.
  - A new `Popup::Create { dir, edit, rect }`: the box with the field. It takes the field's keys, closes on a
    click, and is drawn by `render_popup`.
  - Creating: check the name as Rename does, then `create_dir` or `OpenOptions::create_new`, reload, select it.
- [x] **2. The tab bar** (`crates/tos/src/server/mod.rs`):
  - `TabZone`'s last field becomes `enum TabHit { Tab(usize), Close(usize), New }`.
  - `draw_tab_bar` writes `× ` after each label and keeps its zone; `mouse` runs `close_tab_at` on `Close`.
- [x] **3. Tests:**
  - **Unit:**
    - the full menu has New file and New folder and no Open in app; `o` does nothing in it
    - right-click on empty space and `m` in an empty folder open the short menu
    - `n` makes a file and `N` a folder, each selected afterwards
    - Esc and a click create nothing; a taken name, `/` and `..` are refused
    - the tab bar shows ` ×` on each tab, and its zones say which tab a click on × closes
  - **End to end:** right-click empty space, `N`, type a name, `Enter`, and the list shows the folder
    (`finder_menu_renames_and_deletes`). Clicking tab 2's × closes it while tab 1 is active
    (`tabs_by_key_and_click`). The tests that read the tab bar (`finder_tabs_stay_left`,
    `sessions_come_back_after_kill`) now expect the ×.
- [x] **4. Docs:** the README's menu line, the `?` help screen's mouse line, and change-log lines in
  [the finder's menu](2026-11-07-finder-menu.md).
- [ ] **5. Check by hand (you):** right-click a file and an empty part of the list, make a file and a folder, and
  close a tab with its ×.

Not done: the README's demo GIFs still show tabs without ×. Re-recording them needs your media
(`scripts/demo/record.py`, see CLAUDE.md).

## Change log

- **2026-11-17:** written and built (steps 1–4).
  - **Size:** about 120 lines of code in `finder/menu.rs`, `finder/mod.rs` and `server/mod.rs`, plus about 170 of
    tests, close to the 150 estimated.
  - **Changed while building:** a click *inside* the new name's box keeps it open; only a click outside cancels.
    Rename cancels on any click, but its field is the row itself, so there is no inside to click.
  - **Tests:** `cargo test --workspace` passes, the 44 end-to-end tests included. They need `TOS_SESSION` and
    `TOS_PANE_ID` unset when run from inside a tos pane; otherwise `tos attach` refuses with "already inside session".
