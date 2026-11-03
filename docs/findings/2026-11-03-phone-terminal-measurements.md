# Finding: the finder prototype at phone-sized terminals

**Date:** 2026-11-03 · **Machine:** the Ubuntu dev box (Linux 6.8), zellij 0.44.1, yazi 26.9.1 ·
**Used by:** [phone interface plan](../plans/2026-11-03-phone-interface.md)

## Question

Should the phone interface be a **terminal** (SSH or a web terminal showing the same screen) or a
**phone UI** fed with structured data? To decide, we needed two measurements:

1. **Readability:** how readable is today's two-pane finder at phone terminal sizes?
2. **Traffic:** how much data does each action send to the phone when the interface is a terminal stream, compared
   with sending the underlying data (file list, file contents, a resized image) to a phone UI?

## Method

- **Script:** [`scripts/2026-11-03-phone_measure.py`](scripts/2026-11-03-phone_measure.py), run with
  `python3 -I phone_measure.py <sample> <out> 40x18 50x36 100x24 150x40`.
- **Setup:** for each terminal size, the script starts a throwaway zellij session in a pseudo-terminal of that
  size and launches `finder` into it. It then drives scripted keys and counts every byte the zellij client
  writes to the terminal. That byte stream is what SSH carries, before encryption.
- **Compressed size:** each action's bytes go through one stateful zlib stream, which approximates `ssh -C`.
  OpenSSH does **not** compress by default.
- **Screens:** pane contents are captured with `zellij action dump-screen`, and pane sizes come from
  `zellij action list-panes -j`.
- **Sizes tested:**
  - **40×18:** portrait, keyboard up
  - **50×36:** portrait, keyboard down
  - **100×24:** landscape
  - **150×40:** desktop baseline

  These are **assumed** phone sizes; real devices vary with font size (see "Not measured").
- **Sample folder (7 files):**
  - `a.md` 135 B, `notes.md` 243 B, `long.md` 71,929 B (1,692 lines)
  - `b.txt` 33 B, `c.csv` 24 B, `page.html` 635 B
  - `photo.png` 510,826 B (1024×1024)
- **Actions measured:**
  - open the finder
  - move to the next file six times (the preview re-renders each time)
  - scroll the preview of `long.md` by 1 line ×20 and by a page ×5
  - open `notes.md` in Neovim and type 63 characters
  - idle for 5 s
- **Structured payloads:** the same content as data. That's the directory listing as JSON, the file sources
  (gzip with Python `gzip`), and phone-sized thumbnails of `photo.png` made with Pillow 12.3.0.
- **Raw data:** [`data/2026-11-03-phone/`](data/2026-11-03-phone/) holds `results.json` plus the pane dumps
  for 40×18 and 50×36.

## Results

### 1. Readability: pane sizes (columns × rows of content)

| Terminal | File list | Preview |
|---|---|---|
| 40×18 (portrait, keyboard up) | **12×14** | **24×14** |
| 50×36 (portrait) | 16×32 | 30×32 |
| 100×24 (landscape) | 33×20 | 63×20 |
| 150×40 (desktop) | 51×36 | 95×36 |

At **40×18** the file list shows names reduced to their extension, and Markdown wraps at about 22 columns:

```
files (12 cols)      preview (24 cols)
│…hone-sample        │ long.md   markdown · 1…
│ … │….md            │   "Netplan reference"
│ … │ ….txt          │
│ …│ ….csv           │  • YAML configuration
│ … │ ….md           │    • Top-level
│ … │ ….md           │    configuration
│ … │….html          │    structure
│ … │ ….png          │    • Properties for
```

At **50×36**, names are still cut (`lo….md`, `p….png`). The two-pane layout only becomes readable at
**landscape ~100 columns**.

### 2. Bytes sent to the phone per action (terminal stream)

The values are raw bytes, with the zlib-compressed figure in brackets.

| Action | 40×18 | 50×36 | 100×24 | 150×40 |
|---|---|---|---|---|
| Open finder (first draw) | 16,167 (1,302) | 38,294 (2,403) | 33,652 (2,244) | 65,331 (3,492) |
| Move to next file + preview, mean of 5 text files | 9,215 (334) | 20,525 (627) | 22,580 (635)¹ | 30,527 (973) |
| Move to image (block-character preview) | 11,936 (1,054) | 26,054 (2,074) | 24,858 (2,554) | 43,740 (4,027) |
| Scroll preview 1 line | 1,094 (50) | 2,798 (130) | 2,561 (104) | 5,710 (196) |
| Page down | 1,168 (117) | 2,714 (333) | 2,470 (267) | 5,901 (583) |
| Open Neovim | 19,777 (1,439) | 47,925 (2,788) | 39,707 (2,486) | 70,611 (3,699) |
| **Type 1 character in Neovim** | **1,158 (26)** | **1,088 (27)** | **747 (19)** | **906 (40)** |
| Idle 5 s | 26 | 0 | 0 | 0 |

¹ At 100×24 one move (to `page.html`) sent 46,481 B. The other four were 15.8–17.2 KB.

**Per cell:** at 40×18 (720 cells), moving the cursor one file sends about 9.2 KB, roughly 13 bytes per cell.
**zellij effectively repaints the whole screen**, colours included, on every move.

### 3. The same content sent as data

| Content | Raw | gzip |
|---|---|---|
| Directory listing JSON (7 entries: name, kind, size, mtime) | 537 B | 201 B |
| `a.md` / `notes.md` / `page.html` source | 135 / 243 / 635 B | 132 / 205 / 410 B |
| `long.md` source (sent once; then scrolling is local) | 71,929 B | 19,075 B |
| `photo.png` original | 510,826 B | n/a |
| `photo.png` as 400 px WebP / JPEG (q80), a **real image** | 8,050 / 18,850 B | n/a |
| `photo.png` as 800 px WebP / JPEG (q80) | 17,540 / 45,502 B | n/a |
| Scrolling, typing (handled locally on the phone) | **0 B, no round trip** | |
| Saving an edit | file size | gzip of file |

## Conclusions

1. **The current two-pane terminal layout doesn't work in portrait.** The file list gets 12–16 columns. Any
   terminal-based phone interface, including our rebuild, needs a **single-pane phone layout**. Portrait use
   should be designed for 40–50 columns.
2. **Every interaction through a terminal is a network round trip.** Typing one character in Neovim sends
   0.75–1.2 KB back. Scrolling one line sends 1–6 KB. Nothing is drawn until the server answers. A phone UI
   holding the data scrolls and types locally, with zero traffic and no wait.
3. **Without compression, terminal streams are heavy.** Plain SSH (compression off by default) sends 9–31 KB
   per file move and 16–65 KB per first draw. With compression this falls 20–40× (to 0.3–1 KB per move). A
   terminal approach should use `ssh -C` or Mosh. Our rebuild should send screen diffs, not full repaints;
   we can't measure that until it exists.
4. **Images favour a phone UI clearly.** A real 400 px WebP image is 8 KB. The blocky character preview
   costs 12–44 KB raw (1–4 KB compressed), and still looks worse.
5. **One big file is cheaper once as data, but only up front.** `long.md` costs 19 KB compressed one time. In
   a terminal it's about 0.6 KB compressed per screen, but every screen requires a round trip.
6. **Rich content needs meaning, not characters.** A terminal stream is a grid of characters with no meaning
   attached: the phone can't tell a filename from a heading. "Display nicely in a phone UI" (real fonts,
   tappable files, real images, native text editing) therefore requires **structured data from our server**.
   Re-styling the SSH output can't provide it.

**Not measured (needs a real phone):**
- actual columns × rows in Termux, Blink and Termius at their default fonts
- mobile network round-trip time and how typing feels
- touch and soft-keyboard ergonomics
- battery use
- how Mosh's predictive echo changes the typing experience
