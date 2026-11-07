# Finding: kitty graphics source rectangles in Ghostty, and telling terminals apart (2026-11-07)

## Question

The [PDF scrolling plan](../plans/2026-11-07-pdf-scroll-source-rect.md) sends a PDF page to the terminal once,
then shows each scroll step by placing only part of it: a "source rectangle" (`x`, `y`, `w`, `h` in a placement
`a=p`). Today every step sends a new picture, 0.3–1.6 MB for kitty
([scroll-step finding](2026-11-05-pdf-scroll-step.md)). Before planning it, we needed to know:

1. **Does Ghostty, the user's terminal, draw source rectangles,** scaled into `c`×`r` cells?
2. **Does placing again with the same placement id (`p=`) replace the picture** instead of adding a second one?
   Scrolling depends on that.
3. **Can `tb` tell Ghostty from other kitty-graphics terminals over SSH?** `tb` picks kitty graphics when the
   terminal answers kitty's query, but other terminals answer it too, and not all of them may support source
   rectangles. `TERM_PROGRAM` usually isn't passed over SSH.
4. **How many bytes is one scroll step** with a source rectangle?

## Method

Dev machine: 12th Gen Intel Core i7-12650H, Ubuntu, Linux 6.8.0-147, X11 display `:1`. Ghostty 1.3.1 (GTK 4.14.5,
OpenGL renderer), with an empty user config. ImageMagick's `import`, `xdotool`, Pillow 12.3.0, Python 3.12.3.

The script is [scripts/2026-11-07-kitty_source_rect.py](scripts/2026-11-07-kitty_source_rect.py):

```
python3 -I docs/findings/scripts/2026-11-07-kitty_source_rect.py ghostty OUTDIR
```

1. **XTVERSION:** it opens a Ghostty window that sends `CSI > 0 q` and then `CSI c`, which acts as an end marker,
   and records the raw reply.
2. **Source rectangles:** it opens a second Ghostty window. That window sends a 400×400 test picture once
   (`a=t,f=24`), in four 200×200 quadrants: red top left, green top right, blue bottom left and yellow bottom
   right. Then it places it three times at row 3, column 3, all with the same image id and placement `p=1`:
   - step 1: `x=200,y=200,w=200,h=200,c=20,r=10` (the yellow quadrant)
   - step 2: `x=0,y=0,w=200,h=200,c=20,r=10` (the red quadrant)
   - step 3: `x=0,y=200,w=400,h=200,c=40,r=10` (the bottom half)

   After each step, it takes a screenshot of the window and counts the pixels of each pure colour (±8 per
   channel).
3. **Bytes per step:** the length of one placement, with a cursor move, for a 1000×900-pixel view
   (100×45 cells), counted with Python.

## Results

**XTVERSION and device attributes**, as Ghostty sent them:

```
b'\x1bP>|ghostty 1.3.1\x1b\\\x1b[?62;22;52c'
```

**Source rectangles** (window 926×546 px; cells were 10×21 px):

```
step 1: pixels of each colour: {'red': 0, 'green': 0, 'blue': 0, 'yellow': 42000}
step 2: pixels of each colour: {'red': 41800, 'green': 0, 'blue': 0, 'yellow': 0}
step 3: pixels of each colour: {'red': 0, 'green': 0, 'blue': 41800, 'yellow': 42000}
```

- **Step 1:** 42,000 yellow pixels is 200×210: the 200×200 quadrant scaled into 20×10 cells of 10×21 pixels.
- **Step 2:** no yellow is left, so the placement was replaced, not added.
- **Step 3:** the screenshot shows blue and yellow side by side, 400×210 pixels.

**One scroll step's placement** (68 bytes):

```
'\x1b[3;3H\x1b_Ga=p,i=1234,p=1,x=0,y=1200,w=1000,h=900,c=100,r=45,C=1,q=2\x1b\\'
```

## Conclusions

- **Ghostty 1.3.1 draws source rectangles,** scaled into the cells given. **Placing again with the same `p=`
  replaces the picture.** So a scroll step can be one 68-byte placement instead of 0.3–1.6 MB.
- **XTVERSION names the terminal and its version** (`ghostty 1.3.1`). The reply comes back over SSH, since it's
  the terminal answering, not the environment. `tb` can use source rectangles only for terminals known to support
  them, and keep cutting out pictures for the rest.
- **The whole page costs more to send, once.** At fit width, a whole page is about 1.44–1.57× the 1000×900
  cut-out: about 0.4–2.5 MB. Zoomed ×2, it's about 4× that (1.7–10 MB), and at ×4 about 16× (7–40 MB).
  - These are **estimates**, scaled by area from the scroll-step finding. They're not measured.
  - Above some size, cutting out stays the better choice.
- **Not measured:**
  - **kitty itself** isn't installed here. It wrote the protocol, so it should support source rectangles, but
    that's an **estimate**.
  - **WezTerm, Konsole and other terminals that answer kitty's query** weren't tested.
  - **Sub-cell offsets** (`X`, `Y`) and unicode placeholders weren't tested; the plan doesn't need them.
  - **Ghostty's memory limit for pictures**, and what it does when a held picture is evicted.
  - **Speed over a real SSH link.**
