# Selecting and copying text in the finder's preview

## Decision

The finder's preview gets its own text selection, so you can copy from a file without opening it:

- **Drag** over the preview to select text, from one character to another as in a terminal pane. Letting go
  copies it (OSC 52), as a drag in a terminal pane does. The selection stays highlighted. Dragging above or
  below the preview scrolls it.
- **`v` or `V`** selects whole lines from the keyboard, starting at the first line on screen. `j`/`k` (and the
  other scroll keys) extend it. `y`, `Ctrl+c` or `Enter` copies it, and `Esc` cancels.
- **`Ctrl+c`** copies whatever is selected in the preview. It also copies in the CSV grid, as `y` does there.
  `Ctrl+Shift+c` does the same, but only if the outer terminal passes it on; Ghostty, kitty and GNOME Terminal
  keep it for their own copy on Linux (**estimate**, not checked).
- **What's copied** is the text as shown, without a code file's line numbers and with trailing spaces trimmed.
  Lines in the middle of a selection are copied whole, even past the pane's right edge. Markdown copies as
  shown (wrapped, without `#` or `**`); `E` opens the source in Neovim for that.
- **Links:** a click on a link in a Markdown preview still follows it, but on release, so a drag can start on a
  link.

Pictures, video frames and a PDF's page view have no text, so nothing is selected there.

## Why: what happens today

Found by reading the code at `b446aec` (tos 0.4.5), not by running it.

```
 outer terminal ──1. SGR mouse / keys──▶ tos client ──2. InputEvent──▶ tos server ──3──▶ Finder (preview)
 (Ghostty)                               (mouse capture on)                             handle_mouse / handle_key
```

1. tos turns on mouse capture (`client.rs:329`), so a plain drag goes to tos, not to the outer terminal.
2. The server hands mouse events on a finder preview to `Finder::handle_mouse` (`server/mod.rs:1716`).
3. The preview only uses the wheel and clicks on links (`finder/mod.rs:1204-1212`); a drag does nothing.
   `Ctrl+c` falls through the preview's keys (`finder/mod.rs:1105-1124`). Copy mode (`Ctrl+s v`) only works on
   terminal panes (`copy.rs:33`).

tos never had this. The zellij prototype (now `~/tfinder`) did, without any code for it: its preview was a
terminal pane running `preview.py`, which never turned on mouse capture, so zellij selected and copied there
itself. It was lost when tos started drawing the preview itself (`37b8fb8`, 2026-11-04).

The only way to copy is Shift+drag, which the outer terminal handles itself. That selects screen cells, so
lines include the file list and the frame's `│`, and only what's on screen.

## Example: copying lines 3 to 5 of `main.rs`

The preview shows code with a line-number gutter (`  3 │ `), drawn one column in from the pane's body.

```
 press (line 3, col 4) ─▶ drag ─▶ drag ─▶ release (line 5, col 10)
       │                   │                │
       ▼                   ▼                ▼
 Finder: anchor =     head moves,       text of lines 3..=5 from the anchor to the head, gutter dropped
 (3, gutter+0)        highlight drawn   ─▶ Action::CopyCells { text, "3 lines" } ─▶ server: OSC 52 + "copied 3 lines"
```

1. Press on line 3: the finder notes where (line 3, column 0 of the text, because columns in the gutter count
   as the text's first column). Nothing is highlighted yet.
2. The server sees the finder holding the button and keeps sending it the drag's events until release, even if
   the pointer moves over the file list (new `Drag::Finder`).
3. Each drag event moves the selection's end and the preview draws it reversed.
4. Release on line 5: the finder returns the text: from the press point to the end of line 3, all of line 4,
   and line 5 up to the release column. The server copies it with OSC 52 and the status bar says
   "copied 3 lines".
5. `Ctrl+c` now copies the same text again. `Esc`, a click, or another file clears the selection.

## Options considered

| Option | For | Against |
|---|---|---|
| Keep Shift+drag in the outer terminal | No work | Picks up the list and frames; only what's on screen |
| Draw the preview into a real terminal (alacritty `Term`) and reuse copy mode | One selection model | A large change to how every preview is drawn |
| **The finder keeps its own selection over its rendered lines** (chosen) | Small; knows lines, gutter and off-screen text | A second, simpler selection model next to copy mode |

Mouse selection is by character, as in terminal panes, so a word or a path can be copied. Keyboard selection
is by whole line, because the preview has no cursor column (`h` and `←` go back to the list).

## Steps

1. **`finder/select.rs`** (new): the selection (anchor, end, whole lines or not, still being made or not), the
   text it covers, and its highlight. Columns are display columns of the rendered line, so wide characters
   line up.
2. **`view.rs`:** `View` gets `gutter`, the columns of each line that are line numbers (code previews), so
   they're neither copied nor highlighted.
3. **`finder/mod.rs`:**
   - Mouse: press, drag (scrolling at the top and bottom edges), release copies; a link follows on release
     when the mouse didn't move.
   - Keys in the preview: `v`/`V` start a line selection; while selecting, the scroll keys move its end;
     `y`/`Ctrl+c`/`Enter` copy; `Esc` cancels. With a finished mouse selection, `y` or `Ctrl+c` copies it again
     and `Esc` clears it.
   - The selection clears when the preview shows another file or reloads, and moves with a PDF's text when
     pages load above it.
   - Captions: text previews end with `v select`, after `←→ focus`, so it's the first hint dropped when the
     status bar is short. While selecting with keys: `j/k extend · y copy · Esc cancel`. After a drag:
     `Ctrl+c copy · Esc clear`.
4. **`grid.rs`:** `Ctrl+c` copies as `y` does.
5. **`server/mod.rs`:** `Drag::Finder`, so a drag that starts in a finder's preview (text or grid) gets its
   events until release. This also fixes the grid missing a release outside its pane.
6. **Docs:** README's finder section and the help screen's mouse line.
7. **Tests:** unit tests for the copied text (gutter, wide characters, middle lines whole) and the keys; an
   e2e test that drags in a preview and checks the clipboard, and that `Ctrl+c` copies again. Update the
   caption tests.

Not done: double- and triple-click to select a word or line in the preview.

## Changes

- **2026-11-21, while building it:**
  - `v select` moved to the end of the captions. Before `←→ focus` it pushed `←→ focus` off a 160-column
    status bar (`finder_follows_markdown_links`).
  - The hint after a drag is `Ctrl+c copy · Esc clear`, not `y or Ctrl+c copy · Esc clear`: at 120 columns only
    about 24 columns are left after the mode list.
  - The end-to-end tests have to run with `TOS_SESSION` unset (`env -u TOS_SESSION -u TOS_PANE_ID cargo test`)
    when the shell is inside a tos session, or `tos attach` refuses to nest.
