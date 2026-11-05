# Plan: PDF preview in `tb`'s finder

**Status:** built, waiting for the check by hand (step 11) · **Date:** 2026-11-05 · **Related:** findings [PDF preview with poppler's tools](../findings/2026-11-05-pdf-preview-poppler.md),
[rendering zoomed pages](../findings/2026-11-05-pdf-zoom-render.md) and [a scroll step](../findings/2026-11-05-pdf-scroll-step.md), [rebuild plan](2026-11-03-rebuild-plan.md)
(finder and previews)

## Decision

The finder shows a PDF's **text** by default, in every terminal. **`t` switches to the page view**, which shows
the page as a picture, **fitted to the pane's width**, with **zoom**. It runs poppler's command-line tools as
separate programs; it doesn't link a PDF library.

- **Scope:** `tb`'s finder only. The prototype (`preview.py`) is unchanged.
- **Tools** (package `poppler-utils`, which the user installs; we don't bundle it):
  - `pdfinfo` gives the page count, the title and whether the file is password-protected. It runs once per file.
  - `pdftotext -layout -f A -l B` gives the text of pages A to B. Pages are split at the form feeds `pdftotext`
    puts between them.
  - `pdftoppm -f N -l N -scale-to-x W -scale-to-y -1` renders page N, W pixels wide. We read raw PPM from its
    stdout. **Never `-png`**: PNG encoding added 100 ms to 2.1 s in the finding.

### Text view (the default)

- **Loading:** text comes in **100 pages at a time**. Scrolling to within one screen of the end loads the
  next 100.
  - Speed: from the finding's pace (0.8–1 ms a page; 93 pages took 89 ms and 1004 pages 0.8 s), a chunk
    should take about 100 ms. That's an **estimate**; chunks weren't measured.
- **Layout:** pages are separated by a dim `── page N ──` line, and the text scrolls like any text file.
- **Header:** `pdf · page 3/93 · 1.2 MB · <title>`, where the page is the one at the top of the screen.
- **No text:** if the first chunk has none, as with scanned pages (`scan20` in the finding had none), the finder
  shows the page view for that file instead, with "no text · showing pages" in the header. That doesn't
  change your `t` choice. We don't do OCR.

### Page view (`t`)

- **Where it opens:** at the page at the top of the text view, and `t` goes back to the text at the start of the
  page you're on.
- **Fit-to-width is the default.** The page is as wide as the preview pane, and you scroll down it. Past the
  bottom of a page, scrolling carries on to the top of the next.
- **Zoom levels:** fit page (the whole page in view) ← **fit width** → 150% → 200% → 300% → 400%.
  - Wider than the pane, you pan sideways.
  - The zoom level and the `t` choice stick while you browse and are saved with the finder (`FinderState`),
    like hidden files.
  - Selecting a PDF starts it at page 1.
- **How it's drawn:**
  - **One render per page:** the worker renders the whole page once at the current zoom: the pane's width in
    pixels × zoom, or fitted inside the pane for "fit page".
  - **Scrolling and panning:** cut the visible part out of that picture, with no new render. The cut-out goes
    through the existing image path, so kitty, iTerm2, sixel and the half-block fallback all work.
  - **Why the whole page:** rendering only the visible slice on every scroll step would cost 13–26 ms a step on
    text pages, but 292 ms on a scan, because poppler still decodes the whole scan (zoom finding).
  - **The pane's pixel width** comes from the active client's cell size, which the server already knows. If
    the active client changes cell size, the page renders again.
- **Speed** (zoom finding, 1000-pixel pane):

  | | ×1 | ×2 | ×3 | ×4 |
  |---|---|---|---|---|
  | **text pages** | 16–27 ms | 34–53 ms | 59–84 ms | 91–120 ms |
  | **scanned page** | 58 ms | 108 ms | 246 ms | 397 ms |

  These are the costs of a page turn or a zoom change. Scrolling costs only cutting out, encoding and sending,
  which isn't measured yet (step 7).
- **Memory cap:** the page picture is held while you look at it.
  - **A4 page:** 16 MB at ×2 and 62–68 MB at ×4.
  - **The cap:** **24 megapixels (72 MB)**. Above it, the page renders at the cap and looks a little soft. The
    cap is an **estimate**; it lets ×4 on normal cells through.
  - **HiDPI cells:** with 20-pixel-wide cells, ×4 would be 270 MB, so the cap applies.
- **Header:** `pdf · page 3/93 · fit width · 1.2 MB · <title>`, with `fit page` or `200%` for other levels.
- **While a page renders:** `rendering page 3…`.

### Slow pages

- **Retry without anti-aliasing:** if a page takes over **2 s**, the finder stops it and renders it again with
  `pdftoppm -aaVector no`. The header shows `rendering page 3 without smoothing…` while it does.
  - The slowest normal page measured took 67 ms (0.4 s at ×4 on a scan). The 200,000-line stress page took
    16 s, and 4.5 s without anti-aliasing.
- **Remembered per file:** once a file needed this, its other pages start without anti-aliasing.
- **Giving up:** after **10 s** more, the preview says the page was too slow, with `t` for text.
- **Estimates:** both limits are estimates. They go in `Limits`, so tests can set them.

### Cancelling

Every preview request gets a number. If a newer request arrives while a poppler program is running, the
program is killed within about 10 ms and the worker moves on. Without this, a slow page would hold up every
preview behind it, because the worker does one preview at a time.

### Keys

| Where | Key | Does |
|---|---|---|
| list or preview | `t` | text view ⇄ page view |
| preview, text view | the usual scroll keys | scroll the text; more pages load near the end |
| preview, page view | `j` `k` `↓` `↑`, wheel | scroll 3 rows; past the end of a page, on to the next or previous page |
| preview, page view | `Space` `PageDown` / `b` `PageUp` | scroll a screen, carrying on to the next or previous page the same way |
| preview, page view | `]` / `[` | next / previous page |
| preview, page view | `g g` `Home` / `G` `End` | first / last page |
| preview, page view | `+` `=` / `-` | zoom in / out |
| preview, page view | `0` | back to fit width |
| preview, page view | `h` `l` `←` `→` | pan sideways, when the page is wider than the pane |

- **Going back:** when the page isn't wider than the pane, `h` and `←` go back to the list as they do now.
  `Esc`, `q` and `Tab` always go back.
- **Fit page:** there's nothing to scroll, so the scroll keys turn pages.

### Problems are shown in the preview

- **poppler isn't installed:** "PDF preview needs `pdftotext` (package poppler-utils)".
- **Broken file:** poppler's first error line, for example "Couldn't find trailer dictionary".
- **Password-protected file:** "password-protected PDF".

### Size

About 700–900 lines of Rust plus tests (**estimate**).

## Options considered

| Option | Verdict |
|---|---|
| **poppler's tools as separate programs** | ✅ 14–67 ms a page; killing the program cancels it; a crash can't take down the server; nothing to ship; poppler's GPL doesn't apply to `tb` |
| `pdfium-render` with a shipped pdfium library | ❌ Saves only the 6–8 ms start-up per page. We'd ship another library, a crash would kill the server and all its sessions, and cancelling needs its own mechanism. Its speed wasn't measured (no network) |
| `mupdf` crate | ❌ AGPL; fails `cargo deny` |
| `lopdf` / `pdf-extract` | ❌ Weaker text than `pdftotext`, and no pictures |
| `pdftocairo` instead of `pdftoppm` | ❌ Slower on normal pages (28 vs 22 ms), and TIFF can't go through a pipe. It was faster only on the stress page with anti-aliasing off |
| PNG output from `pdftoppm` | ❌ Adds 100 ms to 2.1 s of encoding |
| Page view as the default | ❌ The user chose text first, in every terminal (2026-11-05) |
| Text of the whole document at once, cut at 2 MB | ❌ 0.8 s before anything shows on a 1004-page file; chunks of 100 pages show sooner |
| Slow pages: give up only, no retry | ❌ The user chose the retry without anti-aliasing (2026-11-05) |
| Fit page as the page view's default | ❌ The user chose fit-to-width (2026-11-05); fit page stays one zoom step out |
| A fixed 1600-pixel render, scaled to fit | ❌ Too soft at fit-to-width and zoom; we render at the pane's width × zoom instead |
| Render only the visible slice, on every scroll step | ❌ 13–26 ms a step on text pages but 292 ms on scans (zoom finding) |
| **Render the whole page at the current zoom, cut it out as you scroll** | ✅ One render per page turn or zoom change (≤ 0.4 s, a scan at ×4); scrolling needs no poppler call; memory capped |
| Cache rendered pages, or render the next page ahead | Later, if page turns feel slow |

## Steps

- [x] **1. File type** (`finder-core`):
  - Add `Kind::Pdf`, detected by the `.pdf` extension or a `%PDF-` start. `detect` already reads the first
    8 KB.
  - Its label is "pdf", and it isn't editable.
  - Turn on the `image` crate's `pnm` feature in the workspace `Cargo.toml`. It's code inside `image`
    0.25.10 (`pnm = []`), so no download is needed.
- [x] **2. Running a tool** (`finder-core`): one helper that:
  - starts the program and reads its stdout on a thread, up to a byte limit
  - checks every 10 ms for cancel or timeout, and kills the program if either happens
  - returns the first line of stderr as the error
  - turns "not found" into the "needs poppler-utils" message

  Unit test: cancelling `sleep 5` returns within 100 ms. This needs no poppler.
- [x] **3. PDF functions** (`finder-core`):
  - `pdf_info`, `pdf_text(path, pages)` returning one string per page, and
    `pdf_page(path, page, size, smooth)`. The size is a width, or a box for "fit page", capped at 24 MP.
  - `load_preview` takes the view (text pages A–B, or page N at a size) and the cancel check.
  - The slow-page limits and the cap go in `Limits`.
  - Tests use small fixtures under `crates/finder-core/tests/data/`:
    - a 2-page PDF made with `ps2pdf`, with its PostScript source alongside
    - an image-only PDF made with Pillow
    - a truncated copy of the 2-page PDF
    - a password-protected copy (`ps2pdf -sUserPassword=…`)
  - A test sets the slow limit to 0 ms, to exercise the retry without anti-aliasing.
  - Tests that need poppler say so and skip when it's missing.
- [x] **4. Worker** (`tb`, `server/finder`):
  - `Request` gains the view, the pane's pixel size and a request number. A shared counter lets the running
    request see that it's stale.
  - The worker keeps each file's `pdfinfo` result and its "needs no smoothing" mark, keyed by path, size and
    modification time.
  - Text chunks are added to the end of the view, not swapped in, so scrolling doesn't jump.
  - Each chunk records the line where each page starts.
- [x] **5. Page view** (`tb`):
  - The finder holds the current page's picture and a viewport: page, zoom, top row and left column.
  - Scrolling and panning cut a new picture from it. Scrolling past an edge requests the next or previous
    page.
  - The server passes the active client's cell size to the finder.
  - Unit tests: the viewport rules (moving on at page edges, clamping the pan, zoom steps, the cap).
- [x] **6. Keys, header and hints** (`tb`):
  - Add the keys above, the headers and the status-bar hints:
    - text view: `pdf · j/k scroll · t pages · h back`
    - page view: `pdf · j/k scroll · [ ] page · +/- zoom · t text · h back`
  - Add `pdf_pages` and `pdf_zoom` to `FinderState` with `#[serde(default)]` (text, fit width), so sessions
    saved before this change still restore.
- [x] **7. Measure a scroll step** before polishing scrolling:
  - Time one step for each protocol (cut out, then encode for kitty, iTerm2 and sixel) on a 1000×900 viewport,
    in a new finding.
  - Check that holding `j` doesn't queue up stale encodes; only the newest view should be encoded.
  - If a step takes over about 50 ms (**estimate** of where scrolling stops feeling smooth):
    - for kitty, send the whole page once and move the visible part with the protocol's source rectangle
    - for the others, drop the steps that arrive while one is encoding
- [x] **8. End-to-end test** (`crates/tb/tests/e2e.rs`): a finder on a folder holding the fixtures should:
  - show the page-one text, with `page 1/2` in the header
  - show `fit width` after `t`
  - show `150%` after `+`, and `fit width` again after `0`
  - show `page 2/2` after `]`
  - show the page-two text after `t`
  - fall back to pages, saying "no text · showing pages", on the image-only fixture
- [x] **9. CI:** install `poppler-utils` in the `check` job, so the poppler tests run there.
- [x] **10. Docs:**
  - In the rebuild plan, add poppler-utils under "External program": optional, not bundled, and the finder works
    without it. Add a PDF row under "Finder and previews", and a change-log line.
- [ ] **11. Check by hand (you):**
  - a real PDF in Ghostty (kitty graphics) and in a block-only terminal
  - both views, scrolling, page turns and every zoom level

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy, `cargo fmt -p tb -p finder-core`,
`cargo deny check licenses`.

## Change log

- **2026-11-05:** written, from the [poppler finding](../findings/2026-11-05-pdf-preview-poppler.md).
- **2026-11-05:** changed after review:
  - **Text first:** text view is the default in every terminal, and `t` switches to the page view. This
    replaced page view first.
  - **Text in chunks:** text loads 100 pages at a time.
  - **Scanned PDFs** fall back to the page view.
  - **Slow pages** retry without anti-aliasing after 2 s, before giving up after 10 s more.
  - **Questions:** the block-only terminal question is gone, and fit-to-width and zoom are explained.
- **2026-11-05:** changed after review:
  - **Page view:** fit-to-width is now its default, with zoom from fit page to 400% and panning. Pages are
    rendered at the pane's width × zoom instead of a fixed 1600 pixels.
  - **Drawing:** the whole page renders once, scrolling cuts the visible part out of it, and pages are capped
    at 24 MP. This is from the new [zoom finding](../findings/2026-11-05-pdf-zoom-render.md).
  - **New steps:** step 5 (page view) and step 7 (measure a scroll step).
  - **Questions:** the open questions section is gone; none are left.
- **2026-11-05:** built (steps 1–10); step 11, the check by hand, is still to do.
  - **Code:**
    - `finder-core`: `tool.rs` runs and stops a program, and `pdf.rs` wraps poppler. `Kind::Pdf` and
      `Preview::Pdf` are new; the latter is the first 100 pages of text, for other users of `load_preview`.
    - `tb`: `server/finder/pdf.rs` holds the view's state; `finder/mod.rs` has the worker's PDF jobs, the keys
      and the drawing.
  - **Size:** about 1,300 lines of code and 450 lines of tests, against the 700–900 estimate.
  - **Tests:**
    - 7 in `finder-core` run poppler on four fixtures (`crates/finder-core/tests/data`, rebuilt by `make.sh`).
    - 6 in `finder-core` cover running a tool: output cap, errors, not installed, cancel, time limit.
    - 9 unit tests cover the view's state.
    - The end-to-end test `pdf_text_pages_and_zoom` goes through text, pages, zoom, `]`, back to text, and
      the scanned fallback.
    - `pdf_page_view_with_kitty_images` checks that the page is placed as a real picture and stays placed
      while scrolling onto page 2.
    - Whether blocks flash between scroll steps is tested on `images::still_shown`, not end to end. The
      flash lasts a few milliseconds, and an end-to-end test that sampled the screen every 10 ms passed even
      with the keeping turned off.
  - **Step 7 measured** ([finding](../findings/2026-11-05-pdf-scroll-step.md)): a scroll step costs 3–25 ms,
    under the 50 ms threshold, so neither fix was needed.
    - kitty and iTerm2 send 0.3–1.6 MB per step, which may be slow over SSH.
    - kitty's source rectangle is the fix if that shows up.
  - **Additions to the plan:**
    - `]` and `[` jump between pages in the text view too.
    - Runs of blank lines in the text become one, and long lines wrap with their indent.
    - The server keeps the picture on screen until the next scroll step's is encoded (`ImageSlot::keep`,
      `images::still_shown`), instead of flashing half-blocks.
    - A header too long for the pane's frame is cut at its end (the title), because the frame drops a label
      that doesn't fit.
    - While a page renders, the header says `rendering…` (or `rendering without smoothing…`). The body says
      `rendering page N…` only when there's no page picture yet; otherwise the last one stays up.
    - `pdfinfo` lists every page's size, for the pixel cap: +0.5–6 ms, measured in the scroll-step finding.
    - In the text view, the header's page number is the page at the top of the screen. On a short document
      that fits the pane, it stays at 1.
