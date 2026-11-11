# Plan: copy a range in the grid, `o` to open files in their app, Enter only for editing in tos, and shorter captions

**Status:** planned, not built · **Date:** 2026-11-11 · **Related:**
[editing xlsx, docx and pptx](2026-11-11-office-editing.md) (moved out of this plan; it makes Enter edit docx and
pptx), [video and Office previews](2026-11-07-video-and-office-previews.md) (Enter on a video changes here),
[rebuild plan](2026-11-03-rebuild-plan.md) (finder and previews) · **Findings:**
[the finder's captions on an HTML file](../findings/2026-11-11-finder-captions.md),
[`[ ]` past a spreadsheet's first or last sheet](../findings/2026-11-11-sheet-ends-caption.md); the rest comes from
reading the current code, cited by file and line.

## Decision

**Your rule for the finder's keys (2026-11-11):**

- **Enter in the file list goes in,** as `→` does: it opens a folder, or moves focus to a file's preview.
- **Enter in the preview edits in tos,** and only there:
  - Neovim, for text, Markdown and HTML
  - the grid, for CSV, TSV and spreadsheets
- **`o` opens any file in its default app.**
- **A caption shows Enter only where Enter does something.**
- **Captions drop what isn't needed,** to save room:
  - no label (`preview ·` or the file's kind)
  - one `←→ focus` for what `←` and `→` already do: up a folder, into a folder, to the preview and back. Those keys
    don't change.
- **`[ ]` pages everywhere,** as it already does in PDFs, and every caption that pages says `[ ] page`.

Four changes follow. They don't depend on each other, except that **E** points to **O**'s key. Nothing needs a new
crate.

| | What | Size (**estimate**) |
|---|---|---|
| **R** | In the grid (`.csv`, `.tsv`, spreadsheets): select a range of cells with the mouse or keys, and copy it | 150–200 lines + tests |
| **O** | `o` opens any file in its default app (LibreOffice, Word, a PDF viewer, a video player…) | 80–120 lines + tests |
| **E** | Enter in the list opens a folder or focuses the preview; Enter in the preview edits in tos. On docx, pptx, images and videos, it edits nothing, and their captions don't show it. A video no longer plays on Enter; `o` opens it. | 40–60 lines + tests |
| **K** | Shorter captions: no label, `←→ focus` instead of `l preview`, `h up` and `h back`, `[ ] page` (and `[ ]` pages in every preview) instead of `Space/b page`, no hints for moving and scrolling, and Enter and `o` first | 30–40 lines + tests |

Editing xlsx moved to [editing xlsx, docx and pptx](2026-11-11-office-editing.md), together with editing docx and
pptx. When that plan is built, Enter on a docx or pptx edits it in tos, and its caption shows Enter again.

### R. Select a range and copy it

- **Mouse:** press on a cell, drag, and release. The range is highlighted while you drag and copied when you
  release. A click without dragging just moves to that cell, as it does now.
- **Keys:**
  - `v` starts a range at the current cell, and moving stretches it. `V` does the same but selects whole rows.
  - `y` copies the range, or just the current cell if there's no range. `Esc` drops the range.
- **What's copied:** the cells as the grid shows them, so a formula's last value and a date as `2026-11-07`.
  - **Format:** a tab between cells and a newline between rows, which Excel, LibreOffice and Google Sheets paste as
    separate cells.
  - **Special characters:** a cell containing a tab, a newline or `"` goes in quotes with `"` doubled
    (`"say ""hi"""`), as Excel does.
  - **No newline at the end,** so pasting into a shell doesn't run anything.
- **How it reaches your clipboard:** through the server's OSC 52 sender (`copy_to_clipboard`,
  `server/mod.rs:626`), the same path as drag-select in a terminal pane and the menu's Copy path. The status bar says
  `copied 3 rows × 2 columns`.
- **Limit:** a copy over 1 MB says `too big to copy (2.3 MB)` and copies nothing. Terminals limit OSC 52 in
  different ways; the 1 MB figure is an **estimate**, checked in step H.
- **Dragging past the edge:** the grid only receives mouse events while the pointer is inside the preview pane
  (`server/mod.rs:1673`, `finder/mod.rs:1150`).
  - Dragging onto the top or bottom data row scrolls one row per move.
  - If you release outside the pane, the range stays selected and `y` copies it.

**Example:** in `sales.xlsx` you press on B2, drag to C4 and release.

```
 1. press B2, drag, release on C4
┌──────────────────┐ 2. mouse events ┌────────────┐ 3. over the socket ┌───────────────────────────────┐
│ your terminal    │ ──────────────► │ tos client │ ─────────────────► │ tos server                    │
│ (iTerm2, kitty…) │                 └────────────┘                    │  finder ─► grid               │
│                  │                                                   │   4. range B2:C4 → text       │
│                  │ 6. OSC 52 escape sequence, base64 of the text     │      "3\t0.5\n10\t0.25\n7\t1" │
│  clipboard ◄─────┼───────────────────────────────────────────────────┤   5. Action::Copy             │
└──────────────────┘    (passes through the client unchanged)          └───────────────────────────────┘
```

### O. `o` opens any file in its default app

- **Where:** the file list, the preview, and the grid; also a right-click menu item, `Open in app`, with the key `o`.
  `o` isn't used in any of them now.
- **Which files:** any, as you chose.
  - **Here:** a spreadsheet opens in LibreOffice Calc (`xdg-mime` says `libreoffice-calc.desktop` for `.xlsx`).
  - **Elsewhere:** a PDF opens in your PDF viewer, a `.docx` in Writer or Word, and a video in your video player.
- **How:** `xdg-open FILE` on Linux, `open FILE` on macOS. It starts the way the video player does now
  (`server/player.rs`), and that code becomes the shared launcher:
  - on the screen of the client that pressed the key, with that client's display variables
  - detached, in its own session, with no input or output; `tos` doesn't track it
- **Messages:**
  - `opening budget.xlsx in its default app`
  - `o needs xdg-open (package xdg-utils)` if `xdg-open` is missing
  - `no display to open it on: you're attached over SSH`, as for videos now
- **In a grid with unsaved changes** (a CSV you're editing): it says `unsaved changes: Alt+s saves them first` and
  opens nothing. The app would otherwise open the saved file without your changes.
- **When the app saves:** the grid and preview don't notice by themselves; `q` and Enter, or `r` in the list, read
  the file again.
- **No app set for the file type:** `xdg-open` may fail or fall back to a browser after `tos` has let it go, so `tos`
  can't report it.

```
 1. o
┌───────────────┐ 2. key ┌────────────┐ 3. socket ┌─────────────────────────────┐
│ your terminal │ ─────► │ tos client │ ────────► │ tos server                  │
└───────────────┘        └────────────┘           │  finder ─► Action::OpenApp  │
                                                  │  4. xdg-open budget.xlsx    │
                                                  │     DISPLAY etc. from the   │
                                                  │     client that pressed o   │
                                                  └──────────────┬──────────────┘
                                                                 │ 5. starts, detached
                                                                 ▼
                                                  ┌─────────────────────────────┐
                                                  │ LibreOffice Calc (a window) │
                                                  └─────────────────────────────┘
```

### E. Enter only edits in tos, and captions show it only where it works

**What's wrong now.** Every preview that isn't a PDF, video or spreadsheet shows the same caption,
`preview · j/k scroll · Space/b page · Enter edit · h back` (`finder/mod.rs:1215`).
- In the preview, Enter calls `edit_current` (`finder/mod.rs:1081`). That only edits Markdown, HTML, CSV and text
  (`Kind::is_editable`, `finder-core/src/lib.rs:79`).
- On a docx, pptx or image it returns nothing, so Enter silently does nothing under a caption that promises
  editing.
- Enter on a video plays it in mpv (`finder/mod.rs:838`, `1078`). That's an outside program, which your rule gives
  to `o`.

**Example, now:** you select `office.docx` in the list.

```
1. Enter in the list    → activate(): not editable, not a sheet, not a video → focus moves to the preview
2. caption              → "preview · j/k scroll · Space/b page · Enter edit · h back"
3. Enter in the preview → edit_current(): editable() is None → Action::None → nothing happens
```

**The fix:**

- **What Enter does:**

  | Selected | Enter in the list | Enter in the preview |
  |---|---|---|
  | folder | opens it (as now) | — |
  | Markdown, HTML, text | focuses the preview (was: Neovim) | Neovim |
  | CSV, TSV | focuses the preview (was: the grid) | the grid |
  | spreadsheet | focuses the preview (was: the grid) | the grid (read-only until the [Office editing plan](2026-11-11-office-editing.md)) |
  | docx, pptx, doc, ppt | focuses the preview (as now) | nothing (edits once the Office editing plan is built) |
  | video | focuses the preview (was: play in mpv) | nothing (was: play in mpv) |
  | image, PDF, anything else | focuses the preview (as now) | nothing |

  - **In the list, Enter does what `→` does,** your choice: the list is for moving around, and the preview is where
    a file is edited. So editing a file from the list is Enter twice: once to focus its preview, once to edit.
  - **A double-click in the list does what Enter does,** as now (both call `activate`).
  - **`e` in the list** still edits directly, as now, unadvertised.
  - **"Nothing" in the preview** flashes `tos can't edit .docx; o opens it in its default app`, so a stray Enter
    isn't silent.
- **Each kind of preview gets a caption that only shows keys that work,** trimmed as in **K**:

  | Preview of | Caption | Characters |
  |---|---|---|
  | Markdown, HTML, CSV, text | `Enter edit · o app · [ ] page · ←→ focus` | 40 |
  | spreadsheet | `Enter grid · o app · [ ] sheet · ←→ focus` | 41 |
  | PDF, text view | `o app · [ ] page · t pages · ←→ focus` | 37 |
  | PDF, pages view | `o app · [ ] page · +/- zoom · t text · ←→ focus`; on a page zoomed wider than the pane, `←→ pan` instead of `←→ focus`, as now | 47 |
  | docx, pptx, a folder's listing | `o app · [ ] page · ←→ focus` | 27 |
  | images, video, doc, ppt, binary files (nothing to scroll) | `o app · ←→ focus` | 16 |

  - **After following a link,** `Backspace back` comes before `←→ focus` (was `Backspace previous file`).
  - **A spreadsheet has no page hint:** its `[ ]` switch sheets, as now, and `PgDn`/`PgUp` page.

  The file list's caption follows the same kinds:

  | Selected | Caption | Characters |
  |---|---|---|
  | anything | `o app · ←→ focus · / filter · . hidden · m menu` | 47 |

  - **No `Enter` in the list's caption:** it does what `→` does there, which `←→ focus` already names.
  - **`o` on a folder** opens it in your file manager.
- **Videos:**
  - **Before:** Enter plays in mpv, falling back to ffplay, with `Enter plays videos with mpv` when neither is
    installed.
  - **After:** `o` opens the video in your default video player through `xdg-open`, like any other file.
  - **What goes:** player.rs's list of players.
  - **What stays:** its display handling, as the shared launcher.

**Example, after:** `office.docx` selected in the list.

```
1. caption              → "o app · ←→ focus · / filter · . hidden · m menu"
2. Enter (or →)          → focus moves to the preview (its frame turns green)
3. caption              → "o app · [ ] page · ←→ focus"
4. Enter                → status bar: "tos can't edit .docx; o opens it in its default app"
5. o                    → status bar: "opening office.docx in its default app"; LibreOffice Writer opens
```

### K. Shorter captions

**What's there now:**

- **The arrow keys already do what the letters do:**
  - In the preview, `↓`/`↑` scroll (`finder/mod.rs:1068–1069`), PDF pages too (`finder/mod.rs:711–712`), and `←`
    goes back to the list (`finder/mod.rs:1082`). In a zoomed PDF page, `←`/`→` pan first (`finder/mod.rs:726–734`).
  - In the list, `↓`/`↑` move, `→` does what `l` does, and `←` goes up a folder (`finder/mod.rs:1099`, `1107`).
  - In the right-click menu, `↓`/`↑` move.

  But every finder caption says `j/k`, `l` and `h`. The rest of tos already writes arrows (`server/keys.rs:674`),
  so the finder is the odd one out. No test presses an arrow in the finder (`tests/e2e.rs` has none), so nothing
  would catch it if they stopped working.
- **`l preview` reads as "show the preview", which is already on screen.** You asked about it on an HTML file. The
  [finding](../findings/2026-11-11-finder-captions.md) shows `l` does work.
  - What it does is move keyboard focus to the preview pane: its frame turns green, the caption changes, and `↓↑`
    then scroll the preview instead of moving through files.
  - The preview's content doesn't change, so it looks like nothing happened.
  - The preview's caption also starts with the word `preview`, which reads like a key.
- **Captions are cut from the right.** tos's own keys fill the left of the status bar. At 130 columns the list's
  caption shows only `j/k move · Enter open/edit`, and `l preview` and the rest are cut (finding).

**The fix: drop what isn't needed, and keep the status bar's spacing as it is.** The keys don't change.

- **`←→ focus`** replaces `l preview`, `h up` and `h back`. In your sense of "focus", it covers everything `←` and `→`
  already do:
  - **In the list:** `←` goes up a folder; `→` goes into a folder, or on a file moves focus to the preview.
  - **In the preview:** `←` goes back to the list.
- **No label:** captions don't start with `preview ·` or the file's kind. The pane's title already names the file.
- **No hints for moving and scrolling** (`j/k move`, `j/k scroll`): the arrow keys move and scroll, as you'd
  expect, and `j`/`k` still work.
- **`[ ] page` for paging, everywhere:**
  - **Captions:** `[ ] page` replaces `Space/b page`.
  - **Keys:** `[` and `]` now page up and down in every preview that scrolls: Markdown, HTML, CSV, text, docx, pptx
    and a folder's listing. Today they do nothing there.
  - **PDFs already work this way:** `[ ]` turns pages in the pages view (`finder/mod.rs:717–718`) and jumps between
    pages in the text view (`finder/mod.rs:1064–1065`).
  - **What still pages:** `Space`, `b`, `PgDn` and `PgUp`, unadvertised.
  - **Spreadsheets keep `[ ]` for sheets** (`finder/mod.rs:1066–1067`).
- **Shorter where it's long:** `Backspace previous file` becomes `Backspace back`.
- **Order:** Enter and `o` first, then `←→ focus`, then the rest.
  - **Why:** the status bar keeps dropping whole hints from the end when there isn't room, as now
    (`server/keys.rs:609–614`), so the least useful go first.
  - **Where:** the tables in **E** are written in that order.
- **What stays:**
  - keys that differ by file: `[ ] sheet`, `[ ] page`, `t pages`, `+/- zoom`
  - the list's `/ filter`, `. hidden` and `m menu`
- **`[ ]` past the first or last sheet says nothing,** as `[ ]` past a PDF's first or last page already doesn't.
  - **Today:** the preview flashes `first sheet` or `last sheet` (`finder/mod.rs:858–860`), which takes the whole
    status bar, caption included, for 3 seconds. The grid writes the same in its last row until the next key
    (`finder/grid.rs:400`).
  - **"Lost forever":** you saw the caption lost for good. I couldn't reproduce that: at 200 and 100 columns it came
    back after 3 seconds ([finding](../findings/2026-11-11-sheet-ends-caption.md)). But narrower than about 130
    columns, little of the caption shows even before the message (at 100 columns, only `spread`). That may be what
    you saw.
  - **After:** no message, so nothing ever takes the caption's place. The frame title already says
    `sheet 2/2 Notes`.
- **The menu's caption:** `menu · j/k move · Enter or a letter runs · Esc closes` becomes
  `Enter or a letter runs · Esc closes`.
- **A test:** no test presses an arrow in the finder now, so an end-to-end test presses `↓`, `↑`, `→` and `←` in the
  list and the preview.

**Example at 130 columns, with an HTML file selected** (the status bar's spacing as now; the full key list takes 93
columns, so 36 are left for the caption):

```
list focused, now:     … <q> QUIT           j/k move · Enter open/edit
list focused, after:   … <q> QUIT          o app · ←→ focus · / filter
preview focused, now:  … <q> QUIT  preview · j/k scroll · Space/b page
preview focused, after:… <q> QUIT        Enter edit · o app · [ ] page
```

The preview's whole 40-character caption shows from 134 columns (93 + 1 + 40).

From 141 columns, the list's whole 47-character caption shows (93 + 1 + 47).

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Copy format | **Tab-separated, Excel-style quoting** | ✅ What spreadsheet apps put on and read from the clipboard; pastes as cells |
| | Comma-separated | ❌ Pastes into one column in most spreadsheet apps |
| Clipboard | **OSC 52 from the server** | ✅ Already used; works over SSH; no extra program |
| | `xclip` / `wl-copy` / `pbcopy` | ❌ Only on the server's machine, and one more thing to install |
| Where you select | **The grid** | ✅ It has a cursor and columns already |
| | The preview table too | Later, if wanted; Enter opens the grid |
| Opening in an app | **`xdg-open` / `open`, any file** | ✅ Your choice: your own default app for every kind of file |
| | `soffice` by name, spreadsheets only | ❌ Ignores your choice of app, and doesn't help PDFs or Word files |
| The key | **`o`** | ✅ Your choice; free in the list, the preview, the grid and the menu |
| What Enter does | **Only edit in tos (your rule)** | ✅ One key, one meaning: Enter never starts an outside program |
| | Enter also opens the app where tos can't edit (docx, video) | ❌ The same key would sometimes edit in tos and sometimes open a window |
| Videos | **`o`, through `xdg-open`, like every file** | ✅ Follows the rule; your default video player |
| | Keep Enter → mpv as an exception | ❌ Breaks the rule; ask if you want mpv kept |
| Enter where it does nothing | **Not in the caption; in the preview, a message pointing to `o`** | ✅ The caption doesn't lie, and a stray Enter isn't silent |
| | Not in the caption, and silent | ❌ That silence is what made the docx case confusing |
| `j`, `k`, `h`, `l` | **Keep working, not in captions** | ✅ Nothing you're used to breaks; the arrows are what captions name |
| | Remove them | ❌ Nothing gained; they don't conflict with anything |
| `←` and `→` in captions | **One hint, `←→ focus`, for what they already do** | ✅ Your choice: up a folder, into a folder, to the preview and back are all "focus" |
| | Separate hints (`→ focus preview`, `← up`, `← files`) | ❌ Three hints' worth of room for one idea |
| Caption label | **None** | ✅ Your choice; the pane's title already names the file, and `preview ·` read like a key |
| | The file's kind (`html ·`) | ❌ Repeats the title and costs room |
| Hints for moving and scrolling | **Left out** | ✅ The arrows do what every list and viewer does |
| | `↓↑ move`, `↓↑ scroll` | ❌ Room spent on what you'd try first anyway |
| Paging | **`[ ]` in every preview, shown as `[ ] page`** | ✅ Your choice; one pair of keys for "page" in every finder preview, as PDFs already have |
| | `Space/b page` | ❌ A longer hint, and a different pair of keys from the PDF's `[ ]` |
| Caption order | **Enter and `o`, then `←→ focus`, then the rest** | ✅ When the status bar drops hints from the end, the least useful go |
| Room for the caption | **The status bar's spacing as now; shorter captions instead** | ✅ Your choice |
| | Shorten tos's key list when the caption needs room | ❌ You'd rather keep the key list as it is |

## Steps

### Range copy

- [ ] **R1. Selection** (`server/finder/grid.rs`): an anchor cell (`Option<(row, col)>`) and a whole-rows flag; `v`,
  `V`, `y` and `Esc`; moving keeps the anchor. Draw the range with a background colour, and keep the cursor
  reversed. The hint line gains `v select · y copy`.
- [ ] **R2. Mouse:** `Down(Left)` sets the anchor; `Drag(Left)` moves the cursor to the cell under the pointer, and
  scrolls at the top and bottom rows; `Up(Left)` after a drag copies.
- [ ] **R3. Copy:** the range's text (tabs, newlines, Excel quoting, no newline at the end), and the 1 MB limit.
  `grid::Outcome::Copy { text, what }` becomes `Action::Copy`, which gains a label for the status bar, so it can
  say `copied 3 rows × 2 columns` instead of quoting the whole text.
- [ ] **R4. Tests:**
  - **Unit:** the text of a range (quoting, ragged CSV rows, `V`); dragging from cell to cell.
  - **End to end:** select a range with `v` and keys, press `y`, and find the OSC 52 sequence in the output, as
    `select_mode_search_and_osc52` does.

### Open in app

- [ ] **O1. Launcher:** `server/player.rs` becomes a launcher module: keep `no_screen` and `start`, drop the player
  list, and add `open_in_app(path, display)`: `xdg-open` on Linux, `open` on macOS. On Linux, use `xdg-open` by
  name, because `open` is not the same program on every Linux system.
- [ ] **O2. Keys:** `o` in the list, preview and grid becomes `Action::OpenApp(path)` (replacing `Action::Play`); add
  the menu item. Refuse in a grid with unsaved changes.
- [ ] **O3. Tests:**
  - **Unit:** the messages for no display and for a missing `xdg-open`.
  - **End to end:** a fake `xdg-open` on `PATH` (as `fake_player` fakes mpv now) records the file it was given.
  - **Video tests:** `video_preview_and_play` and `video_without_ffmpeg_mpv_or_a_display` press `o` and use the fake
    `xdg-open` instead of a fake mpv.

### Enter and captions

- [ ] **E1. Enter:**
  - **In the list,** `activate` opens a folder and focuses the preview for any file. This also covers a
    double-click.
  - **In the preview,** Enter edits in tos, as in the table above. Video Enter goes.
  Enter in the preview on anything else flashes `tos can't edit .EXT; o opens it in its default app`. A
  double-click in the list still focuses the preview.
- [ ] **E2. Captions** (`finder/mod.rs:1196–1225`): the preview and list captions per kind, as in the tables above,
  trimmed as in **K**. Also the menu's caption (`finder/menu.rs:257`).
- [ ] **K1. `[ ]` pages:** in the preview's keys (`finder/mod.rs:1064–1073`), after the PDF and spreadsheet cases,
  `]` and `[` scroll by a page, as `Space` and `b` do.
- [ ] **K2. Sheet ends:** `switch_sheet` (`finder/mod.rs:855`) and the grid's `[ ]` (`finder/grid.rs:397–402`) do
  nothing past the first or last sheet, with no message.
- [ ] **E3. Tests:**
  - **End to end:**
    - Enter on `office.docx` in the preview shows the message and starts nothing.
    - **Captions:**
      - HTML's preview: `Enter edit · o app · [ ] page · ←→ focus`
      - docx's preview: `o app · [ ] page · ←→ focus`
      - an image's preview: `o app · ←→ focus`
      - the list, on any entry: `o app · ←→ focus · / filter · . hidden · m menu`
    - **At 130 columns:** with an HTML file selected, the list's status bar shows `o app · ←→ focus · / filter`.
    - **Enter:**
      - In the list, Enter on a folder opens it.
      - On an HTML file, the first Enter focuses the preview and doesn't start Neovim; a second Enter, in the preview,
        does.
    - **Sheet ends:** `]` on a spreadsheet's last sheet, in the preview and in the grid, leaves the status bar's
      caption in place, with no `last sheet` message.
    - **`[ ]`:** in a long file's preview, `]` scrolls down a page and `[` back up. In a PDF they still turn pages,
      and in a spreadsheet they still switch sheets.
    - **Arrows, unchanged but now tested:**
      - In the list, `↓` and `↑` move the selection, `→` goes into a folder, and `←` comes back up.
      - On a file, `→` focuses the preview, and there `↓` and `↑` scroll a long file and `←` goes back to the list.

### Last

- [ ] **D. Docs:**
  - **README:** the grid's `v` `V` `y`; `o`; what Enter does; videos open with `o`; `[ ]` pages every preview.
  - **[Video and Office plan](2026-11-07-video-and-office-previews.md):** a change-log line saying Enter no longer
    plays videos.
- [ ] **H. Check by hand (you):**
  - copy a range from a real spreadsheet and paste it into LibreOffice or Google Sheets
  - copy a big range, to see where your terminal's OSC 52 limit is
  - `o` on an xlsx, a PDF, a docx and a video

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tos -p finder-core`, `cargo deny check licenses`. No CLI command changes, so `docs/cli/` doesn't change.

## Change log

- **2026-11-11:** written as `2026-11-11-grid-copy-and-xlsx-editing.md`. After the
  [video and Office plan](2026-11-07-video-and-office-previews.md) made spreadsheets read-only, you asked for range
  copy in the grid, option A (change only the edited xlsx cells) and option C (open in the default app).
- **2026-11-11 (later):** your answers and changes:
  - **The open-in-app key works on every file,** as you chose.
  - **Option A (editing xlsx) moved out,** to a new plan,
    [editing xlsx, docx and pptx](2026-11-11-office-editing.md), together with editing docx and pptx. Its open
    question is answered there: formulas' saved values are removed on save, as recommended.
  - **Added E:** Enter on docx and pptx did nothing under an `Enter edit` caption.
  - **Added K:** arrow keys in the captions.
  - **Renamed** to `2026-11-11-grid-copy-and-open-in-app.md`, since xlsx editing is no longer here.
- **2026-11-11 (later still):** your rule for Enter and the app key.
  - **Enter only edits in tos;** `o` (lowercase, not `O`) opens any file in its default app; and a caption shows
    Enter only where it works.
  - **What changed in E:** Enter on docx and pptx no longer opens the app.
  - **Videos:** Enter no longer plays a video in mpv; `o` opens it in the default video player.
- **2026-11-11 (later again):** you asked whether a caption mentioning "preview" did nothing on an HTML file.
  - **What the check found:** `l preview` works, but only moves keyboard focus, so it looks like nothing happened.
    Captions are also cut from the right ([finding](../findings/2026-11-11-finder-captions.md)).
  - **K grew:** `→ focus preview`, `←` in captions, the file's kind first instead of `preview ·`, and Enter and `o`
    before movement.
- **2026-11-11 (evening):** your changes to K.
  - **`←→ focus`** is the caption for what `←` and `→` already do; the keys don't change.
  - **No label** at the start of captions: neither `preview ·` nor the file's kind.
  - **The status bar's spacing stays as it is.** An S that would have shortened tos's key list was written and
    dropped the same evening. Instead, captions lose hints that aren't needed: `j/k move`, `j/k scroll` and
    `Space/b page`, and `Backspace previous file` becomes `Backspace back`.
- **2026-11-11 (evening, later):** paging stays in captions, as `[ ] page`, and `[ ]` now pages every preview that
  scrolls (they did nothing there before). `Space`, `b`, `PgDn` and `PgUp` keep working.
- **2026-11-11 (night):** two more of your changes.
  - **Enter in the list goes in, like `→`:** it opens a folder, or focuses a file's preview. Editing is Enter in the
    preview. The list's caption no longer names Enter.
  - **`[ ]` past the first or last sheet says nothing,** so its message can't take the caption's place. You reported
    the caption lost for good; I could only reproduce it gone for 3 seconds
    ([finding](../findings/2026-11-11-sheet-ends-caption.md)).
