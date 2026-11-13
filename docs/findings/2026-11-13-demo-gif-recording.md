# Finding: recording the finder demo GIF (2026-11-13)

## Question

The README needed a GIF of the finder previewing and editing files. Three decisions depended on facts that
could be checked:
- Can a recording show real images (sixel) without the user's own terminal?
- How wide can the terminal be and still be sharp in the README? This depends on the cell size.
- How big is a GIF of the whole tour, and does it keep every step?

## Method

Dev machine (Ubuntu, Linux 6.8), `tos` release build of `c5a1a39` (0.4.1), xterm 390, Xvfb, xdotool,
ffmpeg 6.1.1, LibreOffice 24.2.7.2, Python 3.12.3 with Pillow, openpyxl and matplotlib.

- **Sample files:** `python3 scripts/demo/make_files.py <folder>`.
- **The recording:** `python3 scripts/demo/record.py --out <gif> --keep`. It starts Xvfb, then
  `xterm -ti vt340 -fa "DejaVu Sans Mono" -fs 10 -geometry 104x34`, with `numColorRegisters: 256` and
  `metaSendsEscape: true`. It drives the tour with `xdotool`, records with
  `ffmpeg -f x11grab -framerate 15` (lossless), and adds captions and a palette to make the GIF.
- **Checks:** window size from `xwininfo`; GIF and raw duration from `ffprobe`; frames from
  `ffmpeg -ss <t> -frames:v 1`, looked at by eye.
- **First try:** ran at 132×38 cells with screenshots (`import -window root`) of each kind of preview.

## Results

**Cell size:** DejaVu Sans Mono at size 10 gives 8×17-pixel cells. 132×38 cells made a 1056×646 window, and
104×34 cells made 832×578.

**Images:** every preview was right, including real sixel pictures for `sunset.png`, the video frame and the
PDF pages (`t`). These were the 132×38 screenshots and the GIF's frames.

**GIF, run 1** (`mpdecimate=hi=512:lo=128:frac=0.1`, no `max`):

```
script: 70 s     raw.mkv: 69.934 s, 1049 frames
GIF: 1.0 MB, 66.54 s, 832x618 (578 + 40 caption)
last frame of raw.mkv: sunset.png shown      last frame of the GIF: sales.csv (the step before)
```

**GIF, run 2** (the same with `max=15`, and 4.5 s instead of 3.4 s on the last step):

```
script: 71 s     GIF: 0.97 MB (967 861 bytes), 70.74 s, 163 frames, 832x618
last frame of the GIF: sunset.png with its caption
```

The whole run, from the sample files to the GIF, took 81 s of wall time.

## Conclusions

- **xterm on Xvfb is enough.** With sixel it shows real images, so the demo needs no real terminal or desktop.
  It runs the same on any Linux machine with these packages.
- **104 columns is 832 pixels.** GitHub's README column is roughly 830–880 pixels wide on a desktop browser
  (an **estimate**, not measured), so the text shouldn't be scaled down. 132 columns (1056 pixels) would be
  scaled to about 80%.
- **Size is no constraint.** A 71-second tour is about 1 MB, because the terminal rarely changes much between
  frames.
- **Dropping repeated frames needs `max`.** Without it the GIF's last frame flashes by and the last step is
  lost. The last frame's delay comes from the next frame's time, and there is none.
- **Unknown:** how the GIF looks on GitHub itself (it was viewed only as extracted frames), and whether the
  timings hold on a slower machine. The script waits fixed times, not for the screen.
