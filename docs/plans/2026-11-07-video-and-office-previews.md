# Plan: video and Office previews in `tb`'s finder

**Status:** built (2026-11-11); the checks by hand (V8, O8) are yours · **Date:** 2026-11-07 · **Related:** [PDF preview](2026-11-05-pdf-preview.md) (the same
pattern of separate programs), [the finder as two panes](2026-11-05-finder-divider.md),
[rebuild plan](2026-11-03-rebuild-plan.md) (finder and previews) · **Findings:**
[video facts and frames with ffmpeg](../findings/2026-11-07-video-preview-ffmpeg.md),
[calamine: licences, dependencies and speed](../findings/2026-11-11-calamine.md),
[dependency licences](../findings/2026-11-03-dependency-licenses.md)

## Decision

Written before the rename, so it says `tb`; the command is now `tos`. What was built differs in a few places,
listed in the change log under 2026-11-11.

Two new kinds of preview, which can be built in either order. **Video comes first,** because it needs no new
crate. **Office needs you to run `cargo fetch`** for calamine.

### Video: a still frame, the duration and the size; Enter plays it in mpv

- **Tools:** ffprobe and ffmpeg (package `ffmpeg`), run as separate programs, as poppler is for PDFs.
  - They're optional and not bundled. Without them, the preview says "video preview needs `ffmpeg` (package
    ffmpeg)", and the header still shows the file's size.
  - Running them as separate programs keeps their LGPL/GPL off `tb`. A stuck one is killed when you move on
    (`finder_core::tool`), and a crash can't take down the server.
- **Which files:** by extension only: `mp4 m4v mkv webm mov avi wmv flv mpg mpeg m2ts 3gp ogv`.
  - `.ts` is left out: it's TypeScript far more often than MPEG transport stream.
  - Sniffing `ftyp` bytes would also catch HEIC and AVIF images and `.m4a` audio, so it isn't done.
- **Header:** `video · 1:23 · 1920×1080 · h264 · 35.6 MB`.
  - The size is the shown size, after the rotation phones record. A portrait clip says `1080×1920`.
  - Without a duration (some broken or streamed files), it's left out.
- **Body:** one frame, drawn like an image preview: kitty, iTerm2 or sixel, or blocks elsewhere.
- **How it loads, in two steps:**
  1. ffprobe (30–110 ms in the finding). The header appears, and the body says `loading frame…`.
  2. One frame, **10% of the way in**, from the keyframe there:
     ```
     ffmpeg -v error -nostdin -ss T -noaccurate_seek -skip_frame nokey -threads 1 -i FILE \
            -frames:v 1 -f image2pipe -c:v ppm -
     ```
     - 34–232 ms in the finding.
     - **10% skips black and faded-in openings,** as thumbnailers do. It costs up to about 0.2 s more than the
       first frame (finding: 64–270 ms in all, against 35–152 ms).
     - **Keyframes only:** accurate seeking took up to 579 ms on 4K HEVC.
     - **One thread:** 1.4–2.8× faster than ffmpeg's default for a single frame.
  - **Shrinking:** the frame comes back full size, and `tb` shrinks it to the image preview's 1600-pixel longest
    side. ffmpeg's scaler added 9–82 ms.
    - Frames over 4K (3840×2160 pixels) are the exception: ffmpeg shrinks those, to keep the pipe under about
      25 MB. That's an **estimate** (8K wasn't measured).
  - **Cancelling:** like PDF pages, a newer request kills a running ffprobe or ffmpeg within about 10 ms.
- **Enter (in the list or the preview) plays the video in a window:**
  - It runs `mpv --force-window=immediate -- FILE` as a separate program. mpv isn't installed on the dev
    machine; you install it.
  - **Without mpv,** it uses `ffplay`, which comes with ffmpeg. The status bar says `playing in ffplay (install
    mpv for a better player)`.
  - **With neither,** it says `Enter plays videos with mpv (package mpv)`.
  - **The player is detached:** its own session, with stdin, stdout and stderr on `/dev/null`. `tb` doesn't
    track it, and closing the finder doesn't stop it.
- **Which screen it opens on: the screen of the client that pressed Enter.**
  - Today the server has the environment of whichever client started it, which could be an SSH login with no
    display, or a desktop you've since walked away from.
  - **The change:** a client sends its `DISPLAY`, `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` when it attaches.
    The player gets those of the client the key came from (the server already knows it).
  - **No display** (an SSH login without `-X`): the status bar says `no display to play on: you're attached
    over SSH`, and nothing starts.
  - **Protocol version:** this changes the attach message, so the version goes from 3 to 4. A session started by
    an older `tb` refuses new clients until `tb kill` (attaching again restores it). If the
    [PDF scrolling plan](2026-11-07-pdf-scroll-source-rect.md) is built at the same time, the two share one
    change.
- **Size:** about 400–600 lines plus tests (**estimate**).

### Office: xlsx in the table and grid; docx and pptx as text

- **Spreadsheets** (`xlsx xlsm xlsb xls ods`), read with the **calamine** crate, as you chose:
  - **Preview:** the first sheet, drawn by the CSV preview's table code.
    - Header: `xlsx · sheet 1/3 Budget · 120 rows · 45 KB`.
    - `[` and `]` change sheet, the keys that change PDF pages.
  - **Cells:**
    - **Values as they're stored:** numbers plain, text as is. Excel's number formats (currency, percentages)
      aren't applied.
    - **Formulas** show their last calculated value, which calamine reads from the file.
    - **Dates:** cells formatted as dates show as `2026-11-07` (calamine's `dates` feature).
  - **Size:** at most `Limits::max_rows` (2000) rows in the preview, as for CSV.
    - For xlsx, read row by row and stop there: calamine's cell reader rather than loading the whole sheet.
    - How fast either is wasn't measured (the crate isn't downloaded). Step 1 measures it.
  - **Enter** opens the grid **read-only**.
    - Arrows, `/` and scrolling work, and `[` `]` change sheet.
    - Editing keys say `read-only: tb can't save .xlsx`.
    - The grid's title says `read-only`.
- **Word (`docx`):** turned into Markdown and shown by the Markdown renderer, as HTML already is.
  - **What's kept:**
    - headings (paragraph styles `Heading1`–`Heading6` and `Title`)
    - paragraphs, bulleted and numbered lists, and tables
    - bold and italic
  - **Left out:** headers, footers, footnotes, comments, tracked deletions, and pictures. A picture becomes
    `[image]`.
- **PowerPoint (`pptx`):** each slide in the presentation's order, as Markdown.
  - Each slide is `## 3 · Its title`, then its text, one paragraph per line, with bullets kept.
  - Speaker notes come later.
- **How:** both formats are zip files of XML.
  - They're read with the `zip` and `quick-xml` crates that calamine already brings in, at the same versions, so
    there's nothing extra to fetch.
  - At most 50 MB of XML is unpacked per part, which guards against zip bombs; that's an **estimate**. The
    Markdown is cut at `Limits::max_bytes` (2 MB), as text files are.
- **Problems are shown in the preview:**
  - **Password-protected files:** "password-protected spreadsheet".
  - **Broken files:** the reader's error.
  - **Old binary `.doc` and `.ppt`:** "`.doc` isn't supported, only `.docx`".
- **Size:** about 600–900 lines plus tests (**estimate**).

### Keys

| Where | Key | Does |
|---|---|---|
| list or preview, on a video | `Enter` | play it in mpv (or ffplay) |
| preview, spreadsheet | `[` / `]` | previous / next sheet |
| list or preview, spreadsheet | `Enter` | the grid, read-only |
| grid, spreadsheet | `[` / `]` | previous / next sheet |

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| Video tools | **ffprobe + ffmpeg as separate programs** | ✅ Already installed here; can be cancelled by killing them; a crash can't take down the server; no licence on `tb` |
| | `ffmpegthumbnailer` | ❌ One more package, and it still needs ffprobe for the facts |
| | Linking libav (`ffmpeg-next`) | ❌ A large C library to build and ship, LGPL/GPL to handle, and a crash would take down the server |
| | A pure-Rust MP4 parser for the facts | ❌ Only some containers, and still no frame |
| Which frame | The first | ❌ Often black or faded in. It's faster (35–152 ms), but by at most about 0.2 s |
| | **10% in, keyframes only, one thread** | ✅ 34–232 ms; skips openings |
| | 10% in, accurate seek | ❌ Up to 579 ms on 4K HEVC |
| Shrinking the frame | In ffmpeg (`-vf scale`) | ❌ Adds 9–82 ms; used only above 4K |
| | **In `tb`, like images** | ✅ |
| Player | **mpv in a window, falling back to ffplay** | ✅ mpv is your choice; ffplay is there whenever ffmpeg is |
| | `xdg-open` | ❌ Whatever app is the default; the prototype's "open" did this |
| | mpv inside a pane (`--vo=tct`, or `--vo=kitty`) | Later, maybe. It would work over SSH. kitty output needs pictures in panes ([extension pictures plan](2026-11-07-extension-images.md), option A) |
| Player's display | The server's own environment | ❌ The wrong screen, or none, when you attach from elsewhere |
| | **The pressing client's display** | ✅ Needs the protocol change |
| Spreadsheets | **calamine** | ✅ Your choice: reads xlsx, xlsm, xlsb, xls and ods. MIT (to be confirmed by `cargo deny` in step 1) |
| Editing spreadsheets | Write back with `rust_xlsxwriter` | ❌ It writes new files, and would drop the original's formatting, charts and macros |
| | `umya-spreadsheet` (reads and writes) | Later, if you want editing; much larger |
| | **Read-only** | ✅ |
| Word and PowerPoint | **Our own zip + XML reading, into Markdown** | ✅ Reuses calamine's crates and the Markdown renderer |
| | `docx-rs` and similar crates | ❌ More dependencies, and none covers pptx |
| | `soffice --convert-to txt` | ❌ Starting LibreOffice takes seconds, and it's a large install |
| Old `.doc` and `.ppt` | Through `antiword` or `soffice` | Later, if needed |

## Steps

### Video

- [x] **V1. File type** (`finder-core`): `Kind::Video` by extension, with the label "video". It isn't editable.
- [x] **V2. Facts and frame** (`finder-core`, new `video.rs`), using `tool.rs` to run and stop programs:
  - `video_info(path)`: duration, the shown width and height (rotation applied), and the codec
  - `video_frame(path, info, cancel)`: the PPM frame, shrunk to 1600 pixels
  - **Fixtures**, made by `crates/finder-core/tests/data/make.sh` with ffmpeg's `testsrc`:
    - a 2-second 64×48 H.264 `.mp4`
    - the same rotated 90° (`-display_rotation 90`)
    - a truncated copy
  - Tests that need ffmpeg say so and skip when it's missing.
- [x] **V3. Worker** (`tb`): a video job sends the facts first, then the frame, both carrying the request number
  (as PDF jobs do).
- [x] **V4. Playing:**
  - `Action::Play(path)`: mpv, then ffplay, then the message
  - detached with `setsid`, with null stdio
  - the display variables taken from the active client
- [x] **V5. Client display** (`protocol.rs`, `client.rs`):
  - `Attach` and `Watch` carry `DISPLAY`, `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR`
  - `PROTOCOL_VERSION` becomes 4
- [x] **V6. Tests:**
  - **Unit:** the facts and frame on the fixtures, rotation, cancelling, and ffmpeg not installed (an empty
    `PATH`).
  - **End to end:** a finder on the fixtures shows `video · 0:02 · 64×48`.
  - **Enter, end to end:** with `PATH` holding fake `mpv` and `ffplay` scripts that write down their arguments
    and environment, Enter runs mpv with the client's `DISPLAY`.
  - **CI:** install `ffmpeg` in the `check` job, beside poppler-utils.
- [x] **V7. Docs:**
  - rebuild plan: ffmpeg and mpv under "External programs" (optional, not bundled), a video row under
    "Finder and previews", and a change-log line
  - README's finder keys
- [ ] **V8. Check by hand (you):**
  - install mpv (`sudo apt install mpv`)
  - your phone clips in Ghostty: the frame, the header, Enter
  - attached over SSH: the "no display" message

### Office

- [x] **O1. Fetch and check calamine** (you run `cargo fetch` after it's added to the workspace):
  - Record a finding with:
    - its version, licence and dependency tree
    - whether its `zip` and `quick-xml` can be used directly
    - `cargo deny check licenses`
  - **Measure the time to the first 2000 rows** of a 100,000-row × 20-column xlsx (made with openpyxl, which
    is installed). Compare the cell reader with `worksheet_range`, and record it in the same finding.
  - If calamine fails the licence check, stop and come back to this plan.
- [x] **O2. File types** (`finder-core`): `Kind::Sheet` for spreadsheets, and `Kind::Docx` and `Kind::Pptx`,
  labelled by their extension.
- [x] **O3. Spreadsheets** (`finder-core`, new `sheet.rs`):
  - sheet names, and one sheet's rows as text (up to the row limit), with dates as ISO 8601
  - `Preview::Sheet { sheets, current, rows, truncated }`
- [x] **O4. docx and pptx** (`finder-core`, new `office.rs`):
  - turn each into Markdown, with the limits above
  - **fixtures:** a small docx and pptx made with `soffice --convert-to` from text sources kept beside them,
    plus a truncated docx
- [x] **O5. Finder** (`tb`):
  - sheet switching in the preview, and the header
  - the grid's read-only mode, filled from calamine's rows, with sheet switching
- [x] **O6. Tests:**
  - **Unit:** cells and dates; sheet names; the row limit; headings, lists and tables from docx; slide order
    and titles from pptx; broken and password-protected files.
  - **End to end:** a spreadsheet's preview shows `sheet 1/2`, and `]` shows sheet 2; Enter opens the grid,
    and `e` says read-only; a docx shows its heading.
- [x] **O7. Docs:** calamine in the rebuild plan's "Finder and previews" table, with its version and licence;
  README.
- [ ] **O8. Check by hand (you):** real xlsx, docx and pptx files of yours.

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tb -p finder-core`, `cargo deny check licenses`.

## Open questions

Both were answered on 2026-11-11, when you asked for the plan to be built as written:

1. **A frame 10% in, or the first frame?** 10% in.
2. **Read-only spreadsheets, or editing?** Read-only for now.

## Change log

- **2026-11-07:** written, from the [ffmpeg finding](../findings/2026-11-07-video-preview-ffmpeg.md). The user
  asked for video previews (a frame, the duration and size; Enter opens mpv in a window), xlsx in the CSV grid
  with calamine, and docx and pptx as text.
- **2026-11-11:** you asked for the whole plan to be built, as written. **Video is built** (V1–V7; V8 is yours).
  Where it differs from the text above:
  - **Only `Attach` carries the display,** not `Watch`: a watching client's keys are ignored, so it can't press
    Enter.
  - **The client also says whether it's an SSH login** (`SSH_CONNECTION` or `SSH_TTY`). The "attached over SSH"
    message needs it; without a display and without SSH (a Linux console), it says `no display to play on`.
  - **macOS** needs no display variables to open a window, so there it only refuses a client attached over SSH.
  - **ffplay** gets `-autoexit`, so its window closes at the end, as mpv's does.
  - **The fallback frame:** if ffmpeg writes nothing at 10% (seeking past the last keyframe of an oddly cut
    file), it takes the first frame instead.
  - **calamine's `dates` feature isn't needed:** date cells come back as dates without it
    (`ExcelDateTime::to_ymd_hms_milli`), which saves building chrono.
- **2026-11-11:** **Office is built** (O1–O7; O8 is yours). The [calamine
  finding](../findings/2026-11-11-calamine.md) settled step 1: calamine is MIT and adds 12 crates under licences
  already allowed. Its cell reader gives the first 2000 rows of a 100,000-row xlsx in 21 ms, against 804 ms to
  load the sheet whole. Where it differs from the text above:
  - **Only xlsx and xlsm are read cell by cell.** xls and ods have no cell reader in calamine, so their sheets
    load whole: 1.1 s for a 100,000-row ods, during which the preview worker is busy. xlsb has a cell reader,
    but LibreOffice can't write xlsb, so there's no test file; it loads whole too, for now.
  - **Numbers** show at most 15 significant digits, as Excel does in a general cell, so a stored
    `0.30000000000000004` shows as `0.3`. Booleans show as `TRUE` and `FALSE`.
  - **The grid has no `/` search,** in CSV files either, so read-only it has arrows, `g`/`G`, `0`/`$`, paging,
    `[` `]` and `q`. A sheet with more than 100,000 rows shows the first 100,000 and says so.
  - **Headings in docx** are found by their style's name in `styles.xml` ("heading 1"), which Word keeps in
    English in every language; without `styles.xml`, by the ids `Heading1` and `Title`.
  - **pptx:** dates, footers and slide numbers are left out; tables come through as tables, pictures as
    `[image]`, bold and italic are kept, and a hidden slide's heading says `(hidden)`.
  - **Password-protected files** (an OLE file with an `EncryptionInfo` stream, not a zip) say "password-protected
    spreadsheet", "… document" or "… presentation". An old OLE file renamed to `.docx` says so instead.
  - **The test files are made through LibreOffice's Python bridge** (`make_office.py`), not `soffice
    --convert-to`: it's the only way to set a password, and to build slides with placeholders and bullet levels.
    `office.html` is the docx's source.
  - **A frame cuts any preview's header with `…`** instead of dropping it when it doesn't fit, as it already did
    for PDFs: a spreadsheet's header (`xlsx · sheet 1/2 Budget · 3 rows · 6.6 KB`) is long.
- **2026-11-11 (later):** follow-up plans. Until they're built, spreadsheets stay read-only as above.
  - Copying a range from the grid, and `o` to open any file in its default app:
    [grid copy and open in app](2026-11-11-grid-copy-and-open-in-app.md). It also makes Enter only edit in tos,
    so Enter will no longer play a video; `o` opens it in the default video player instead.
  - Editing xlsx values, and docx and pptx text: [editing xlsx, docx and pptx](2026-11-11-office-editing.md).
- **2026-11-12:** the [grid copy and open in app plan](2026-11-11-grid-copy-and-open-in-app.md) is built.
  - **Videos:** Enter no longer plays a video in mpv; `o` opens it in the default video player, through
    `xdg-open` (or `open` on macOS), as it opens any file.
  - **Spreadsheets:** Enter on a spreadsheet in the list now only focuses the preview, and Enter there opens the
    grid. `[ ]` past the first or last sheet says nothing.
