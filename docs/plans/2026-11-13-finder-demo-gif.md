# Plan: a demo animation for the README

**Status:** done · **Date:** 2026-11-13 · **Changed** (2026-11-13, at the user's request):
- it starts with `tos attach` from a plain shell, and ends with Claude Code in a new tab
- it's recorded in Ghostty instead of xterm, so pictures are in full colour
- it's an animated WebP instead of a GIF: lossless, because a GIF's 256 colours undid Ghostty's full-colour
  pictures. The file name keeps "gif" so links to this plan still work.
- the video and picture steps show the user's Ghibli-style video and a generated picture from `~/ghibli-video`,
  and each recording writes both a WebP and a GIF
- the README shows the GIF, by the user's choice; the WebP stays beside it
- the same recording also gives a 28 s highlights GIF, `docs/images/demo-short.gif`, for the top of the README;
  the full tour folds away below it ([README plan](2026-11-13-readme-marketing.md))
- the order is now: `tos attach`, a finder tab, Claude Code in a new tab writing `server.py`, back to the
  finder where `/server` finds it, then the walk through every file (the user's request). Recorded at 99 s (WebP
  1.5 MB, GIF 1.2 MB) and 33 s for the short GIF (0.8 MB); Claude's 12 s plays at 2× and 6×

**Findings:**
[recording the finder demo GIF](../findings/2026-11-13-demo-gif-recording.md),
[Claude Code in the demo](../findings/2026-11-13-claude-in-demo.md),
[recording in Ghostty](../findings/2026-11-13-ghostty-demo.md),
[WebP instead of GIF](../findings/2026-11-13-demo-webp.md),
[the Ghibli video and picture](../findings/2026-11-13-demo-ghibli-media.md),
[`tb`'s finder images in xterm](../findings/2026-11-05-xterm-sixel.md)

## Decision

**A script records the demo, so it can be recorded again when the finder changes.** `scripts/demo/record.py` runs
Ghostty on a virtual display and types `tos attach` into its shell. It presses the keys with `xdotool`, records with
ffmpeg and adds a caption bar. Each recording writes a GIF, `docs/images/finder-demo.gif`, which the README shows before
**Install**, and a lossless animated WebP beside it, `finder-demo.webp`.

```
 ┌ Xvfb :N (virtual display, nothing on your screen) ──────────────┐
 │  Ghostty (kitty images), own D-Bus; 104×34 cells, ~836×582 px   │
 │   └ bash ── tos attach ──── tos server, session "main"          │
 │   tabs: finder · 1 bakery (bash) · 2 bakery (claude)            │
 │   HOME, sockets, state and Claude's config in /tmp/tosdemo.*    │
 └─────────────────────────────────────────────────────────────────┘
      ▲ 1 key presses (xdotool)   │ 2 frames, 15 a second (ffmpeg x11grab)
      │  ▲ 3 tos dump-screen:     ▼
      │  │   is Claude done?
 ┌────┴──┴──────────┐  4 captions,     ┌──────────────────────────────────────┐
 │ record.py script ├─────────────────▶│ ffmpeg: caption bar, speed up        │
 │ (the tour)       │  fast-forwards   │ Claude's work, 15 frames a second,   │
 └──────────────────┘  and their times │ lossless ──▶ the WebP                │
                                       └──────────────────────────────────────┘
```

**What it shows** (about 93 s, 832×618 or 836×622 px; WebP about 1.4 MB, GIF about 1.1 MB):

| Step | What happens |
|---|---|
| shell | `tos attach` typed in a plain shell starts session "main"; `tos finder` opens a finder tab |
| `budget.xlsx` | the table; `→` then `]` shows the second sheet |
| `index.html` | the HTML as formatted text; `→` `Enter` opens Neovim, a line is added, `Alt+s` `Alt+q`, and the preview shows it |
| `movie.mp4` | the length, size and a frame: the user's Ghibli-style video (made up: `mandelbrot.mp4`) |
| `notes.docx` | the Word file as Markdown |
| `README.md` | rendered; `→` `Enter`, a box is ticked, `Alt+s` `Alt+q` |
| `report.pdf` | the text; `t` shows the page as an image, `]` the next page, `t` back |
| `sales.csv` | the table; `→` `Enter` opens the grid, `e` changes a cell, `Alt+s` saves, `q` closes |
| `snapshot.png` | the picture, in full colour (the kitty protocol): a generated frame of a baby in a cape (made up: `sunset.png`) |
| new tab | `Ctrl+t` `n`; `claude`; a prompt for a simple web server; Claude's work, sped up (the caption says how much) |
| back | `Ctrl+t` `Tab` `Esc` to the finder, which lists `server.py`; `↑` shows it highlighted |

- **The sample files are made up** (`scripts/demo/make_files.py`): a bakery's folder.
  - **The video and the picture:** `--video` and `--image` give them. The README's recording uses
    `~/ghibli-video/output/full_v5_depth8/babyplayful_ghibli.mp4` and a frame of `output/t2v/flying_baby.mp4`
    ([finding](../findings/2026-11-13-demo-ghibli-media.md)).
  - **Without them,** the script draws a sunset and makes a Mandelbrot video, so no real files appear.
- **Claude's part varies from run to run.** Its code and reply differ, and so does how long it takes. The WebP in
  the repo is one run.

## Example: the Claude step, key by key

1. **`Ctrl+t` `n`:** a new tab, "2 bakery", opens with a shell.
2. **`claude`, `Enter`:** Claude Code starts. Its config folder is the script's own:
   - a copy of your login
   - the notices you've already seen
   - the sample folder trusted

   So it starts without first-run notices.
3. **The script waits for Claude's prompt.** It polls `tos dump-screen` until `❯` shows.
4. **It types the request and presses `Enter`:**
   `Write a simple web server in server.py that says "Hello from the bakery!"`.
5. **The script waits for Claude to finish.** Every 1.5 s it reads the screen again. It stops when all three hold:
   - `server.py` exists
   - "esc to interrupt" is gone
   - the screen is unchanged since the last read

   In the recorded run this took 12 s, shown at 2×. The factor is the time taken ÷ 6 s, at least 2.
6. **"Done: Claude's reply" shows for 4.5 s.**
7. **`Ctrl+t` `Tab` `Esc`, then `↑`:** back in the finder, the list keeps the picture (`snapshot.png` or
   `sunset.png`) selected by name, so `↑` lands on the new `server.py`, which sorts just before it.

## Options considered

- **Recording tools:**
  - **vhs, or asciinema with agg:** neither is installed. Neither draws sixel or kitty images, so the image,
    video and PDF steps would show only coloured blocks.
  - **The user's own terminal:** it shows real images, but it can't be re-run and it takes over the desktop
    while it records.
  - **The end-to-end test harness** (`crates/tos/tests/e2e.rs`): it reads the screen as text and has no images.
  - **A terminal on Xvfb (chosen):** it runs headless, and it's driven by key presses.
- **Which terminal:**
  - **xterm** (the first version): real images, but sixel, with 216 colours
    ([finding](../findings/2026-11-05-xterm-sixel.md)). Kept as `--terminal xterm`.
  - **Ghostty (chosen, at the user's request):** full-colour pictures through the kitty protocol. It needs its own
    D-Bus and some care ([finding](../findings/2026-11-13-ghostty-demo.md)).
- **How Claude Code signs in:**
  - **Your real `~/.claude`:** your settings, plugins and hooks would show, and the session would land in your
    history.
  - **An API key:** there isn't one on this machine.
  - **A copy of your login in a config folder of its own (chosen):**
    - it leaves out the keys that say who you are
    - it's deleted afterwards, also with `--keep`
    - it refuses to run if the token has less than 30 minutes left, so the copy never refreshes it and can't
      sign out the original
- **Waiting for Claude:**
  - **A fixed time:** Claude's time varies.
  - **Watching the screen (chosen):** shown sped up, so the demo stays short.
- **Format:**
  - **GIF** (the first versions): plays everywhere, but its 256 colours band the pictures. Kept as `.gif`.
  - **gifski:** better GIF palettes, but it isn't installed or packaged here, and its files are bigger
    (an **estimate**).
  - **Lossy WebP:** at quality 90 it's barely smaller than lossless.
  - **Lossless animated WebP (chosen, by the user):** every pixel exact, and about 1.4 MB
    ([finding](../findings/2026-11-13-demo-webp.md)). Whether GitHub plays it in a README isn't checked yet.
  - **Both (the user's request):** one recording writes the WebP and the GIF. The README shows the GIF, the
    user's choice: it plays in every viewer, though its pictures are a little posterised.
- **The picture:**
  - **The baby-walker PNGs in `~/ghibli-video`:** they look like a third party's product photo, so they may not
    be free to publish.
  - **A frame of the generated `flying_baby.mp4` (chosen):** made by the user's own pipeline, from text only.

## Steps

1. Sample files: `scripts/demo/make_files.py`.
2. The recorder: `scripts/demo/record.py`.
   - **Its own folder:** HOME, `TOS_SOCKET_DIR`, `TOS_STATE_DIR` and `CLAUDE_CONFIG_DIR` are under a short
     `/tmp/tosdemo.*`, and `TOS_NO_UPDATE_CHECK=1` and `DISABLE_AUTOUPDATER=1` are set. The environment is built
     from scratch, so `ZELLIJ` or `TMUX` from the caller can't turn images off.
   - **The terminal:** Ghostty with its theme Catppuccin Mocha, size 10 and 104×34 cells. It runs under
     `dbus-run-session`, with `--gtk-single-instance=false` and `--config-default-files=false`, and with gvfs and
     accessibility off (`GIO_USE_VFS=local`, `GTK_A11Y=none`). Its shell is bash, so the tour begins by typing
     `tos attach`.
     - **`--terminal xterm` instead:** `-ti vt340`, 256 colour registers, `metaSendsEscape`, and the same colours.
   - **Keys:** sent through XTEST, to the focused window. xterm ignores events sent to a window.
   - **Claude's config:**
     - your `~/.claude.json` without its per-user keys
     - `hasCompletedOnboarding`
     - the sample folder trusted
     - `autoModeEnvSetup.dismissed`, so no dialog follows the task ([finding](../findings/2026-11-13-claude-in-demo.md))
   - **The animation:**
     - captions drawn with Pillow, with keys as small boxes
     - Claude's work cut out and sped up with `trim`, `setpts` and `concat`, then back to 15 frames a second
     - **WebP (the default):** `libwebp_anim`, lossless, at a steady 15 frames a second. libwebp merges
       repeated frames into longer ones, so the last frame keeps its time
       ([finding](../findings/2026-11-13-demo-webp.md)).
     - **GIF (`--out` ending in `.gif`):**
       - `mpdecimate` with `max=15`, so the last frame keeps its time ([finding](../findings/2026-11-13-demo-gif-recording.md))
       - **one palette,** made from frames the tour marks, plus the captions: the finder, the video (counted
         twice), the PDF page, the picture (three times), Claude's reply and the code
       - bayer dithering at scale 4, so the pictures band less and the background stays solid
         ([finding](../findings/2026-11-13-ghostty-demo.md))
       - `-fps_mode vfr`
   - **Clean-up:**
     - It ends the session, the terminal and Xvfb.
     - It ends every process that has the demo's `TOS_SOCKET_DIR` in its environment, or works in the folder:
       Ghostty's D-Bus services, or a server Claude started.
     - It unmounts anything under the folder, then deletes it.
   - **`--no-claude`:** stops after the finder.
3. Record to `docs/images/finder-demo.gif` and `.webp`, and show the GIF in the README.

**To record again** after a finder change: `cargo build --release`, then

```bash
python3 scripts/demo/record.py --video ~/ghibli-video/output/full_v5_depth8/babyplayful_ghibli.mp4 \
    --image ~/ghibli-video/output/t2v/flying_baby.mp4
```

Without `--video` and `--image`, it shows the made-up video and picture. If a step changes keys, edit `tour()` or
`claude()` in `record.py`. The finder's waits are fixed times, so a slow machine may need longer ones.
