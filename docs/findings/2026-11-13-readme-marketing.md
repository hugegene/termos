# Finding: the short demo GIF, badges and repo settings (2026-11-13)

## Question

The [README plan](../plans/2026-11-13-readme-marketing.md) put a short GIF at the top, and badges and the
repo's description and topics on GitHub. Four things decided whether that works:
- how long and big a highlights GIF cut from the same recording is
- whether it keeps the order of the tour
- whether the badges render
- whether the description and topics can be set without switching the user's active `gh` account

## Method

Dev machine (Ubuntu 24.04), Ghostty 1.3.1, ffmpeg 6.1.1, `tos` from `9a2d920` (0.4.1), gh as installed.

- **The recording:**

  ```
  python3 scripts/demo/record.py --video ~/ghibli-video/output/full_v5_depth8/babyplayful_ghibli.mp4 \
      --image ~/ghibli-video/output/t2v/flying_baby.mp4 --out <dir>/finder-demo.gif \
      --out <dir>/finder-demo.webp --short-out <dir>/demo-short.gif --keep
  ```

  The tour marked 15 highlight stretches (`d.highlight` and `d.highlighting`): a moment of each preview at
  normal speed, the edits at 3×, Claude's start at 3×, the prompt at 4×, and Claude's work sped to about 2 s.
- **The short GIF:** `ffprobe` for its length; one frame every 1.5 s (`fps=1/1.5`), looked at in order.
- **Badges:** each shields.io URL fetched with curl, reading the SVG's `<title>`.
- **Repo settings:**
  - `GH_TOKEN=$(gh auth token --user hugegene) gh repo edit hugegene/termos --description … --add-topic …`
  - then `gh repo view --json description,repositoryTopics`

## Results

**The recording:**

```
recorded 104 s; Claude worked 76.1–92.7 s (16.6 s): 3x in the full GIF, 8x in the short one
demo-short.gif      721 767 bytes, 28.0 s, 832x618
finder-demo.gif   1 079 148 bytes, 93 s
finder-demo.webp  1 396 850 bytes, 93 s
whole run: 2 min 6 s
```

**The short GIF's frames, in order:**

1. the spreadsheet
2. `index.html`, then Neovim with the line typed in, then the preview with "Rye loaf"
3. the video frame
4. the Word file
5. the PDF page as an image
6. the CSV, then the grid with the cell edited
7. the picture
8. Claude starting, the prompt, and Claude working (captioned "sped up 8×")
9. Claude's reply
10. `Ctrl+t` `Tab`, then `server.py` highlighted

**Badges:**

```
release: v0.4.1
license: MIT
runs on: Linux · macOS (Apple Silicon)
written in: Rust
```

**Repo settings** (active `gh` account: the work one, left as it was):

```
description: Drop all desktop apps, use tos: a terminal workspace with sessions, tabs, splits and a file finder
             that previews and edits files
topics: claude-code, cli, file-manager, file-preview, rust, terminal, terminal-multiplexer, tmux, tui, zellij
```

## Conclusions

- **The short GIF is 28 s and 0.7 MB.** It shows every kind of file and Claude, in the tour's order, so a visitor
  sees it all before it loops. It comes from the same recording as the full tour, and so stays in step with it.
- **Captions name the speed-up per animation:** "sped up 3×" in the full GIF and "8×" in the short one.
- **The badges work.** The release badge follows new releases by itself.
- **`GH_TOKEN` for `hugegene` writes the repo settings** without switching the account.
- **Not doable from here:** the social preview image (`docs/images/social-preview.png`, 1280×640). GitHub has no
  API for it, so it's uploaded by hand in Settings → General → Social preview.
- **Unknown:** how the README looks on GitHub. It hasn't been seen there yet: the `<details>` blocks, the badges'
  layout, and the GIFs' sizes. Check after pushing.
