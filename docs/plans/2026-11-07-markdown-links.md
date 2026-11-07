# Plan: following links in the finder's Markdown preview

**Status:** built, waiting for the check by hand (step 6) · **Date:** 2026-11-07 · **Related:**
[the finder as two panes](2026-11-05-finder-divider.md)

## Decision

**A click on a link in a Markdown preview shows the link's target in the finder.** The user asked for this
(2026-11-07): clicking a link to another `.md` file should jump to that file's preview.

- **What a click does:**
  - The file list moves to the target's folder and selects the target, so the preview shows it.
  - List and preview stay in step, as they do now, and the list shows where you've landed.
  - Focus stays in the preview, so you can keep reading and clicking.
- **Backspace in the preview goes back** to the file the link was in, at the same scroll position.
  - Clicking several links in a row builds a history, like a browser's. It keeps up to 100 files.
  - Backspace is free in the preview, and in the file list it already means "go up", so it reads as "back".
  - `Ctrl+O`, vim's "jump back", isn't available: it's zellij's session mode.
  - The status bar shows `Backspace previous file` while there's somewhere to go back to.
    - It comes first in the preview's hints, because only about 20 characters of them fit after the mode
      list at 120 columns.
- **What can be clicked:**
  - `[text](dest)` links and `<https://…>` autolinks, including links that wrap onto a second line.
  - Links in tables. The generated [docs/cli/README.md](../cli/README.md) is a table of links.
  - Image placeholders (`[image: alt]`), which show the picture.
  - A badge, an image inside a link, follows the link, not the picture.
- **How a link's target is found:**
  - **A path:**
    - It's relative to the Markdown file's folder; an absolute path stays absolute.
    - `#heading` and `?query` endings are dropped, `%20` and other escapes are decoded, and `..` is folded.
      The list shows `~/docs/findings`, not `~/docs/plans/../findings`.
  - **`file://` URLs** are treated as paths.
  - **A missing file:** the status bar says `no such file: <dest>`.
  - **A folder:** it's selected in its parent, so the preview lists what's in it.
  - **A hidden file** (name starting with `.`) turns hidden files on, as `.` does. Otherwise the list
    couldn't select it.
- **What isn't followed yet:**
  - **Web and mail links** (`https:`, `mailto:` and other schemes): the status bar shows the URL.
    - Opening a browser from the server is wrong when the server runs on another machine, as in the
      [phone plan](2026-11-03-phone-interface.md).
    - Copying the URL to the clipboard would be a small follow-up.
  - **Links to a heading** (`#usage`): the status bar says so.
    - Only one such link exists in `docs/` today (`grep -c "](#"`).
    - In another file, `other.md#usage` opens `other.md` at the top.
  - **Links in HTML previews** (`<a href>`): HTML is shown through the Markdown renderer, which only
    dims raw tags.
  - **Following links from the keyboard** (for example, cycling through them with a key).
- **Size:** about 250–350 lines with tests (**estimate**).

No measurements were needed: nothing here depends on timings or tool behaviour we don't already know.

## How it works

**The renderer records where each link landed.** `view::markdown` already wraps text itself, so it knows
each word's line and column as it places it.

- **Each piece of a link** placed while inside a link becomes a `Link { line, start, end, url }`.
  - `start` and `end` are columns in the rendered line.
  - A link that wraps gives one `Link` per line.
- **Inside a table cell**, a link is recorded as a byte range of the cell's text.
  - When the table is drawn, the range becomes columns.
  - It's placed by the column widths (`table` is split into `column_widths` and the drawing), and it's cut
    where the cell is cut.
- **`View` gets `links: Vec<Link>`.** It's rebuilt with the rest of the view when the pane is resized, so
  the columns always match the wrapping on screen.

**The finder hit-tests a click in the preview.**

- **Where the click was:**
  - The line is `text_top() + (y - body.y)`.
  - The column is `x - body.x - 1`, after the one column of padding.
- **What happens:**
  - A hit calls `follow(url)`. It resolves the target, pushes `(current file, scroll)` onto the history,
    and shows the target with `cd(parent, Some(name))`.
  - Backspace pops the history, shows that file, and puts the scroll back.
  - This works the same way as `restore_state`: the scroll is kept until the view arrives.

## Steps

1. **Link areas in `view.rs`.** Unit tests cover:
   - plain links
   - a link wrapping onto two lines
   - links in a table, including a cut cell
   - an image inside a link
2. **`finder_core::link::resolve`, which finds a link's target.** It's pure path logic, so it lives in
   `finder-core`. Unit tests cover relative paths, `..`, `%20`, `#fragment`, `file://` and other schemes.
3. **The finder:** clicks in the preview, Backspace to go back, and the status-bar hint.
4. **An e2e test:**
   - Clicking a link in `a.md` previews `b.md` in another folder, and the list moves there.
   - Backspace goes back to `a.md`.
5. **One line in the README** next to the PDF note.
6. **Check by hand (the user)** in a real session. Click through `docs/cli/README.md` and the links
   between plans, then use Backspace to come back.
