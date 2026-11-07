# Plan: video and Office previews in `tb`'s finder

**Status:** proposed · **Date:** 2026-11-07 · **Related:** [PDF preview](2026-11-05-pdf-preview.md) (the same
pattern of separate programs), [the finder as two panes](2026-11-05-finder-divider.md),
[rebuild plan](2026-11-03-rebuild-plan.md) (finder and previews) · **Findings:**
[video facts and frames with ffmpeg](../findings/2026-11-07-video-preview-ffmpeg.md),
[dependency licences](../findings/2026-11-03-dependency-licenses.md)

## Decision

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

- [ ] **V1. File type** (`finder-core`): `Kind::Video` by extension, with the label "video". It isn't editable.
- [ ] **V2. Facts and frame** (`finder-core`, new `video.rs`), using `tool.rs` to run and stop programs:
  - `video_info(path)`: duration, the shown width and height (rotation applied), and the codec
  - `video_frame(path, info, cancel)`: the PPM frame, shrunk to 1600 pixels
  - **Fixtures**, made by `crates/finder-core/tests/data/make.sh` with ffmpeg's `testsrc`:
    - a 2-second 64×48 H.264 `.mp4`
    - the same rotated 90° (`-display_rotation 90`)
    - a truncated copy
  - Tests that need ffmpeg say so and skip when it's missing.
- [ ] **V3. Worker** (`tb`): a video job sends the facts first, then the frame, both carrying the request number
  (as PDF jobs do).
- [ ] **V4. Playing:**
  - `Action::Play(path)`: mpv, then ffplay, then the message
  - detached with `setsid`, with null stdio
  - the display variables taken from the active client
- [ ] **V5. Client display** (`protocol.rs`, `client.rs`):
  - `Attach` and `Watch` carry `DISPLAY`, `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR`
  - `PROTOCOL_VERSION` becomes 4
- [ ] **V6. Tests:**
  - **Unit:** the facts and frame on the fixtures, rotation, cancelling, and ffmpeg not installed (an empty
    `PATH`).
  - **End to end:** a finder on the fixtures shows `video · 0:02 · 64×48`.
  - **Enter, end to end:** with `PATH` holding fake `mpv` and `ffplay` scripts that write down their arguments
    and environment, Enter runs mpv with the client's `DISPLAY`.
  - **CI:** install `ffmpeg` in the `check` job, beside poppler-utils.
- [ ] **V7. Docs:**
  - rebuild plan: ffmpeg and mpv under "External programs" (optional, not bundled), a video row under
    "Finder and previews", and a change-log line
  - README's finder keys
- [ ] **V8. Check by hand (you):**
  - install mpv (`sudo apt install mpv`)
  - your phone clips in Ghostty: the frame, the header, Enter
  - attached over SSH: the "no display" message

### Office

- [ ] **O1. Fetch and check calamine** (you run `cargo fetch` after it's added to the workspace):
  - Record a finding with:
    - its version, licence and dependency tree
    - whether its `zip` and `quick-xml` can be used directly
    - `cargo deny check licenses`
  - **Measure the time to the first 2000 rows** of a 100,000-row × 20-column xlsx (made with openpyxl, which
    is installed). Compare the cell reader with `worksheet_range`, and record it in the same finding.
  - If calamine fails the licence check, stop and come back to this plan.
- [ ] **O2. File types** (`finder-core`): `Kind::Sheet` for spreadsheets, and `Kind::Docx` and `Kind::Pptx`,
  labelled by their extension.
- [ ] **O3. Spreadsheets** (`finder-core`, new `sheet.rs`):
  - sheet names, and one sheet's rows as text (up to the row limit), with dates as ISO 8601
  - `Preview::Sheet { sheets, current, rows, truncated }`
- [ ] **O4. docx and pptx** (`finder-core`, new `office.rs`):
  - turn each into Markdown, with the limits above
  - **fixtures:** a small docx and pptx made with `soffice --convert-to` from text sources kept beside them,
    plus a truncated docx
- [ ] **O5. Finder** (`tb`):
  - sheet switching in the preview, and the header
  - the grid's read-only mode, filled from calamine's rows, with sheet switching
- [ ] **O6. Tests:**
  - **Unit:** cells and dates; sheet names; the row limit; headings, lists and tables from docx; slide order
    and titles from pptx; broken and password-protected files.
  - **End to end:** a spreadsheet's preview shows `sheet 1/2`, and `]` shows sheet 2; Enter opens the grid,
    and `e` says read-only; a docx shows its heading.
- [ ] **O7. Docs:** calamine in the rebuild plan's "Finder and previews" table, with its version and licence;
  README.
- [ ] **O8. Check by hand (you):** real xlsx, docx and pptx files of yours.

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tb -p finder-core`, `cargo deny check licenses`.

## Open questions

1. **A frame 10% in, or the first frame?** The plan says 10% (it skips black openings, at up to about 0.2 s
   more). Is that the right trade?
2. **Read-only spreadsheets:** is that enough for now, or do you want editing (a much larger job)?

## Change log

- **2026-11-07:** written, from the [ffmpeg finding](../findings/2026-11-07-video-preview-ffmpeg.md). The user
  asked for video previews (a frame, the duration and size; Enter opens mpv in a window), xlsx in the CSV grid
  with calamine, and docx and pptx as text.
