# Finding: pixel sizes seen inside `tb` panes and zellij panes (2026-11-04)

## Question

The user saw an image preview in Ghostty that was "very pixelated, low resolution and stretched horizontally".
What draws it, and what cell size in pixels do programs see? Whether to change `tb`, the fork or the prototype
depends on it.

## Method

Dev machine (Ubuntu, Linux 6.8, X11 at 2048×1152), Ghostty 1.3.1 with default settings, zellij 0.44.1 (snap),
chafa 1.14.0, `tb` at commit `62dcc2f`.

1. **The session log** `~/.local/state/tb/main.log` for the image method each client got.
2. **A screenshot of the Ghostty window only:** `DISPLAY=:1 import -window <id> ghostty.png`.
3. **The window size each terminal reports:** `TIOCGWINSZ` on the pseudo-terminals of the `tb` client, of `tb`'s
   panes, and of the panes of zellij sessions running inside `tb` and inside Terminator.
4. **`tb`'s own kitty output:** `tb attach` in a pseudo-terminal of 79×26 cells at 10×21 pixels
   (`TB_IMAGES=kitty`), opening a finder on a 4736×2656 JPEG. The kitty sequence was captured and decoded.
5. **A probe program** (`sizes.py`: `TIOCGWINSZ`, then `CSI 14 t`, `CSI 16 t`, `CSI 18 t`, DA1) run in a
   throwaway session `px` (`TB_SOCKET_DIR=/tmp/tbt-px`):
   - directly in a `tb` pane
   - inside zellij session `tftest-px` in a `tb` pane
   - The client was 197×50 cells at 10×21 pixels, like the user's Ghostty window.
6. **chafa as `preview.py` runs it** (`-f symbols -c full --size 120x42 --polite on`, standard input from
   `/dev/null`), on the user's 480×832 frame, with a controlling terminal of 120×42 cells reporting various
   pixel sizes.

## Results

**Session log:** two clients were attached to `main`.

```
client 2 shows images with Graphics { protocol: Blocks, cell_px: None }          (158x39)   ← Terminator
client 4 shows images with Graphics { protocol: Kitty, cell_px: Some((10, 21)) }  (79x26)   ← Ghostty
```

**Screenshot:** Ghostty runs `tb attach main`. Its tabs 1 and 2 run **zellij with the prototype finder** (yazi
plus `preview.py`). The 480×832 frame was drawn by `preview.py` (chafa symbols) as a band the full width of the
preview and about 14 rows high.

**Reported sizes:**

```
Ghostty → tb client (pts/15)            197x50 cells, 1978x1070 px  (cell 10.04 x 21.40)
tb panes (fixed 8x16 cells)             195x46 cells, 1560x736 px   (cell 8 x 16)
zellij in tb: preview.py / yazi panes   120x42 cells, 240x1260 px   (cell 2 x 30)
zellij in Terminator (session prbot)    216x46 cells, 432x1242 px   (cell 2 x 27)
Terminator → zellij client              218x50 cells, 0x0 px
```

**`tb`'s kitty output, 79×26 cells at 10×21:**

```
kitty control: a=T,f=100,t=d,i=2,c=48,r=13,C=1,q=2,m=1
source (4736, 2656) aspect 1.783 · png sent (480, 269) aspect 1.784 · cells (48, 13) -> box 480x273, aspect 1.758
```

**Probe**, before and after giving panes the client's cell size (`tb` change) and answering `CSI 16 t` (a
trial change to the fork):

```
before, tb pane:       ioctl 195x46 cells, 1560x736 px, cell 8.00x16.00
                       replies: \e[4;736;1560t \e[8;46;195t \e[?6c                 (no answer to 16t)
before, zellij in tb:  ioctl 193x42 cells, 386x924 px, cell 2.00x22.00
                       replies: \e[4;924;386t \e[6;22;2t \e[8;42;193t \e[?62;4;52c
after, tb pane:        ioctl 195x46 cells, 1950x966 px, cell 10.00x21.00
                       replies: \e[4;966;1950t \e[8;46;195t \e[?6c \e[6;21;10t
after, zellij in tb:   ioctl 193x42 cells, 386x924 px, cell 2.00x22.00              (unchanged)
                       replies: \e[4;924;386t \e[6;22;2t \e[8;42;193t \e[?62;4;52c
```

**chafa, 480×832 image into 120×42 cells:**

```
zellij's 240x1260 px (2x30 cells), no ratio:  120 cols x 14 rows
zellij's 240x1260 px, --font-ratio 1/2:       48 cols x 42 rows
real 1200x882 px (10x21 cells), no ratio:     51 cols x 42 rows
```

## Conclusions

- **The stretched picture comes from zellij, not `tb`'s finder.** zellij 0.44.1 tells its panes their cells
  are 2 pixels wide, in Terminator as well as in `tb`. chafa believes it and draws portrait images as wide
  strips. The blockiness is chafa's symbol output, which is all a preview inside zellij can get.
- **Fix in the prototype:** `preview.py` passes chafa `--font-ratio`, taken from the terminal's pixel size
  when it's plausible (cell width ÷ height between 0.3 and 0.8), and 1/2 otherwise.
- **`tb` reported fixed 8×16 cells** to its panes. It now reports the active client's real cell size, in the
  pane size and in the `CSI 14 t` answer. This only matters for programs run directly in a `tb` pane; zellij
  ignores it.
- **Answering `CSI 16 t`** needs a change to the fork, because vte 0.15 drops that query. zellij's result
  didn't change, so the trial change was **not kept**; the fork stays unmodified.
- **`tb`'s own finder is right on paper:** in a kitty-protocol terminal it sends a full-resolution image
  whose shape is within 2% of the source.
- **Unknown:** how `tb`'s finder images look in the real Ghostty window. The user hasn't opened `tb`'s own
  finder there yet.
- **Not measured:** how zellij computes its 2-pixel width.
