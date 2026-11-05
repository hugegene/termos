# Finding: PDF preview with poppler's command-line tools (2026-11-05)

## Question

The finder should preview PDFs. Should `tb` run poppler's command-line tools (`pdfinfo`, `pdftoppm`,
`pdftotext`) as separate programs, or link a PDF library such as `pdfium-render`? That depends on:

- how long a page preview takes with the tools, and how much of it is the fixed cost of starting a program,
  which a library would save
- how they behave on large, scanned, heavy and broken files
- which output format to read the image in

## Method

Dev machine: 12th Gen Intel Core i7-12650H (16 CPUs), 31 GiB RAM, Ubuntu, Linux 6.8.0-147. CPU governor
`powersave`; load average 1.5–2.6 from other work during the runs, so these are numbers from a busy laptop.
Versions: `poppler-utils` 24.02.0-1ubuntu9.11, Ghostscript 10.02.1, LibreOffice 24.2.7.2, Python 3.12.3,
Pillow 12.3.0.

Script: [`scripts/2026-11-05-pdf_measure.py`](scripts/2026-11-05-pdf_measure.py).

```
python3 docs/findings/scripts/2026-11-05-pdf_measure.py corpus DIR   # build the test PDFs
python3 docs/findings/scripts/2026-11-05-pdf_measure.py run DIR      # main table
python3 docs/findings/scripts/2026-11-05-pdf_measure.py slow DIR     # follow-up on the vector page
```

**Test files** (DIR was in the scratchpad, not committed):

| file | pages | bytes | what it is |
|---|---|---|---|
| `testpage.pdf` | 1 | 31,694 | `/usr/share/cups/data/default-testpage.pdf` (pdfTeX, vector graphics) |
| `spec.pdf` | 19 | 147,924 | `/usr/share/doc/shared-mime-info/shared-mime-info-spec.pdf` (Ghostscript, text) |
| `manual.pdf` | 93 | 295,259 | `/usr/share/doc/printer-driver-foo2zjs/manual.pdf` |
| `longtext.pdf` | 1004 | 10,831,554 | 50,000 lines of text converted by LibreOffice (`soffice --headless --convert-to pdf`) |
| `scan20.pdf` | 20 | 17,448,591 | 20 A4 pages at 300 dpi (2480×3508 JPEG, quality 75, about 870 KB a page), made with Pillow |
| `vector.pdf` | 1 | 3,424,574 | stress test: 200,000 coloured line segments, PostScript through `ps2pdf` |
| `truncated.pdf` | – | 177,155 | the first 60% of `manual.pdf` |
| `garbage.pdf` | – | 200,009 | `%PDF-1.7` followed by 200,000 random bytes |

**Timing:** each command ran through `subprocess.Popen`, its stdout read to the end through a pipe, and reaped
with `os.wait4`. Elapsed time therefore covers starting the program, opening the PDF, rendering and moving the
output through the pipe. Each case had 2 warm-up runs, then 15 timed runs (5 if a warm-up took over 1 s).
Max RSS is the child's `ru_maxrss`. All runs had the files in the page cache.

*Note added later the same day:* `ru_maxrss` also counts the Python script's memory at the moment each tool
starts, so the RSS column is only an upper limit. The 21–28 MiB peaks for page renders were later confirmed
with GNU time (23.8–28.0 MiB). The smaller values, such as `pdfinfo` and `/bin/true`, are inflated. See
[the zoom finding](2026-11-05-pdf-zoom-render.md).

**Render size:** `-scale-to 1600` matches the finder's `Limits::max_image_side` (1600) in
`crates/finder-core/src/lib.rs`. `-scale-to 16` renders a tiny page, which leaves mostly start-up and parsing.

## Results

**Main run** (`run`):

| case | median ms | p90 ms | min ms | max RSS MiB | stdout bytes | exit | runs |
|---|---|---|---|---|---|---|---|
| baseline: /bin/true | 0.4 | 0.5 | 0.4 | 14 | 0 | [0] | 15 |
| testpage (1 p): pdfinfo | 7.2 | 8.5 | 5.3 | 15 | 402 | [0] | 15 |
| testpage (1 p): pdftoppm p1 @16px (overhead) | 17.0 | 17.8 | 15.8 | 16 | 637 | [0] | 15 |
| testpage (1 p): pdftoppm p1 @800px | 21.1 | 22.8 | 18.8 | 17 | 1447215 | [0] | 15 |
| testpage (1 p): pdftoppm p1 @1600px | 28.4 | 29.2 | 26.7 | 23 | 5784017 | [0] | 15 |
| testpage (1 p): pdftoppm -png p1 @1600px | 132.8 | 136.3 | 127.3 | 23 | 99286 | [0] | 15 |
| testpage (1 p): pdftotext p1 | 15.5 | 17.4 | 12.7 | 21 | 19 | [0] | 15 |
| spec (19 p): pdfinfo | 7.6 | 8.6 | 6.5 | 21 | 545 | [0] | 15 |
| spec (19 p): pdftoppm p1 @16px (overhead) | 10.8 | 11.8 | 9.2 | 21 | 637 | [0] | 15 |
| spec (19 p): pdftoppm p1 @800px | 13.6 | 14.4 | 12.6 | 21 | 1485615 | [0] | 15 |
| spec (19 p): pdftoppm p1 @1600px | 22.2 | 23.8 | 20.7 | 24 | 5937617 | [0] | 15 |
| spec (19 p): pdftoppm -png p1 @1600px | 170.5 | 173.9 | 163.3 | 24 | 176852 | [0] | 15 |
| spec (19 p): pdftoppm p9 @1600px | 29.5 | 30.1 | 28.3 | 24 | 5937617 | [0] | 15 |
| spec (19 p): pdftoppm p19 @1600px | 17.7 | 18.8 | 16.7 | 23 | 5937617 | [0] | 15 |
| spec (19 p): pdftotext p1 | 8.6 | 9.5 | 7.2 | 21 | 1757 | [0] | 15 |
| spec (19 p): pdftotext all | 35.8 | 37.1 | 33.9 | 21 | 46881 | [0] | 15 |
| manual (93 p): pdfinfo | 7.6 | 8.4 | 5.7 | 21 | 578 | [0] | 15 |
| manual (93 p): pdftoppm p1 @16px (overhead) | 9.2 | 11.7 | 7.8 | 21 | 589 | [0] | 15 |
| manual (93 p): pdftoppm p1 @800px | 11.9 | 13.2 | 10.6 | 21 | 1358415 | [0] | 15 |
| manual (93 p): pdftoppm p1 @1600px | 17.3 | 18.6 | 15.4 | 22 | 5428817 | [0] | 15 |
| manual (93 p): pdftoppm -png p1 @1600px | 114.9 | 195.1 | 108.6 | 23 | 102908 | [0] | 15 |
| manual (93 p): pdftoppm p46 @1600px | 16.5 | 17.9 | 15.5 | 22 | 5428817 | [0] | 15 |
| manual (93 p): pdftoppm p93 @1600px | 14.4 | 14.9 | 12.4 | 22 | 5428817 | [0] | 15 |
| manual (93 p): pdftotext p1 | 8.2 | 8.4 | 6.6 | 21 | 947 | [0] | 15 |
| manual (93 p): pdftotext all | 89.4 | 91.0 | 80.2 | 21 | 193431 | [0] | 15 |
| longtext (1004 p): pdfinfo | 19.1 | 21.6 | 17.5 | 21 | 421 | [0] | 15 |
| longtext (1004 p): pdftoppm p1 @16px (overhead) | 20.3 | 21.6 | 19.1 | 21 | 637 | [0] | 15 |
| longtext (1004 p): pdftoppm p1 @800px | 23.4 | 24.3 | 21.3 | 21 | 1485615 | [0] | 15 |
| longtext (1004 p): pdftoppm p1 @1600px | 32.7 | 34.5 | 30.4 | 25 | 5937617 | [0] | 15 |
| longtext (1004 p): pdftoppm -png p1 @1600px | 327.2 | 331.2 | 316.1 | 25 | 441720 | [0] | 15 |
| longtext (1004 p): pdftoppm p502 @1600px | 36.9 | 37.9 | 34.9 | 26 | 5937617 | [0] | 15 |
| longtext (1004 p): pdftoppm p1004 @1600px | 35.2 | 36.2 | 33.1 | 26 | 5937617 | [0] | 15 |
| longtext (1004 p): pdftotext p1 | 20.9 | 22.8 | 19.1 | 21 | 4027 | [0] | 15 |
| longtext (1004 p): pdftotext all | 797.7 | 823.1 | 777.9 | 21 | 3819337 | [0] | 15 |
| scan20 (20 p): pdfinfo | 6.0 | 8.5 | 5.2 | 21 | 431 | [0] | 15 |
| scan20 (20 p): pdftoppm p1 @16px (overhead) | 42.1 | 44.8 | 39.9 | 21 | 589 | [0] | 15 |
| scan20 (20 p): pdftoppm p1 @800px | 47.3 | 48.4 | 45.1 | 21 | 1358415 | [0] | 15 |
| scan20 (20 p): pdftoppm p1 @1600px | 61.9 | 64.5 | 59.0 | 28 | 5433617 | [0] | 15 |
| scan20 (20 p): pdftoppm -png p1 @1600px | 2183.0 | 2192.7 | 2151.8 | 28 | 1018422 | [0] | 5 |
| scan20 (20 p): pdftoppm p10 @1600px | 62.0 | 65.0 | 59.9 | 28 | 5433617 | [0] | 15 |
| scan20 (20 p): pdftoppm p20 @1600px | 67.3 | 72.4 | 61.5 | 28 | 5433617 | [0] | 15 |
| scan20 (20 p): pdftotext p1 | 7.0 | 9.5 | 4.9 | 21 | 1 | [0] | 15 |
| scan20 (20 p): pdftotext all | 7.4 | 9.0 | 6.0 | 21 | 20 | [0] | 15 |
| vector (1 p): pdfinfo | 7.0 | 9.4 | 5.4 | 21 | 442 | [0] | 15 |
| vector (1 p): pdftoppm p1 @16px (overhead) | 578.3 | 600.6 | 547.2 | 21 | 589 | [0] | 15 |
| vector (1 p): pdftoppm p1 @800px | 7948.8 | 7997.9 | 7737.0 | 21 | 1358415 | [0] | 5 |
| vector (1 p): pdftoppm p1 @1600px | 15861.4 | 16053.7 | 15807.3 | 23 | 5428817 | [0] | 5 |
| vector (1 p): pdftoppm -png p1 @1600px | 16392.8 | 16854.8 | 15972.5 | 23 | 5132115 | [0] | 5 |
| vector (1 p): pdftotext p1 | 255.7 | 270.8 | 243.1 | 21 | 1 | [0] | 15 |
| truncated (0 p): pdfinfo | 7.7 | 9.1 | 6.1 | 21 | 0 | [1] | 15 |
| truncated (0 p): pdftoppm p1 @16px (overhead) | 7.6 | 9.2 | 6.5 | 21 | 0 | [1] | 15 |
| truncated (0 p): pdftoppm p1 @800px | 7.8 | 8.6 | 6.7 | 21 | 0 | [1] | 15 |
| truncated (0 p): pdftoppm p1 @1600px | 8.4 | 9.5 | 6.2 | 21 | 0 | [1] | 15 |
| truncated (0 p): pdftoppm -png p1 @1600px | 9.2 | 11.6 | 6.6 | 21 | 0 | [1] | 15 |
| truncated (0 p): pdftotext p1 | 7.2 | 9.7 | 6.3 | 21 | 0 | [1] | 15 |
| garbage (0 p): pdfinfo | 7.6 | 10.2 | 6.4 | 21 | 0 | [1] | 15 |
| garbage (0 p): pdftoppm p1 @16px (overhead) | 7.7 | 8.8 | 6.2 | 21 | 0 | [1] | 15 |
| garbage (0 p): pdftoppm p1 @800px | 8.3 | 9.7 | 5.9 | 21 | 0 | [1] | 15 |
| garbage (0 p): pdftoppm p1 @1600px | 8.4 | 9.5 | 6.4 | 21 | 0 | [1] | 15 |
| garbage (0 p): pdftoppm -png p1 @1600px | 8.3 | 10.8 | 7.5 | 21 | 0 | [1] | 15 |
| garbage (0 p): pdftotext p1 | 8.1 | 9.3 | 6.1 | 21 | 0 | [1] | 15 |

`pdfinfo` errors: `truncated.pdf` "Syntax Error: Couldn't find trailer dictionary"; `garbage.pdf` "Syntax
Error (14): Illegal character <92> in hex string".

**Follow-up on the vector page** (`slow`). `pdftocairo` writes its TIFF to a file, because through a pipe only
the 8-byte header arrived. Each run wrote a complete TIFF (5,433,804 bytes for `vector`, 5,942,604 bytes for
`spec`).

| case | median ms | p90 ms | min ms | max RSS MiB | exit | runs |
|---|---|---|---|---|---|---|
| vector: pdftoppm -aaVector no p1 @1600px | 4470.8 | 4495.6 | 4424.3 | 22 | [0] | 5 |
| vector: pdftocairo -tiff (uncompressed) p1 @1600px | 6603.3 | 6635.6 | 6486.0 | 24 | [0] | 5 |
| vector: pdftocairo -tiff -antialias none p1 @1600px | 1792.9 | 1909.2 | 1720.9 | 24 | [0] | 5 |
| spec: pdftoppm -aaVector no p1 @1600px | 23.1 | 24.3 | 21.6 | 24 | [0] | 15 |
| spec: pdftocairo -tiff (uncompressed) p1 @1600px | 28.2 | 29.0 | 27.3 | 27 | [0] | 15 |
| spec: pdftocairo -tiff -antialias none p1 @1600px | 26.6 | 28.2 | 24.7 | 27 | [0] | 15 |

**Side numbers:**
- **Raw PPM output:** `pdftoppm` with no output name writes binary PPM (`P6`) to stdout, checked with
  `pdftoppm -f 1 -l 1 -scale-to 100 default-testpage.pdf | xxd` (`P6\n76 100\n255\n`).
- **LibreOffice:** converting `longtext.txt` to the 1004-page PDF took 6.5 s, in a single run with a fresh
  profile. This is not a measure of Office-file preview.
- **No PPM decoding in `tb` yet:** the workspace `image` dependency (0.25) enables `png`, `jpeg`, `gif`, `webp`
  and `bmp`, but not `pnm`.

## Conclusions

- **Run the poppler tools as separate programs; don't link a library.**
  - **Start-up cost:** about 6–8 ms (`pdfinfo` on the small files; starting `/bin/true` is 0.4 ms, so the
    rest is loading poppler and opening the file).
  - **Large documents** cost more: 19 ms on the 1004-page file, because every call re-reads the page tree.
  - **What a library would save:** by keeping the file open, at most that start-up cost per page. Normal pages
    take 14–67 ms in all at 1600 px, so the saving is small, and it doesn't justify shipping pdfium with `tb`.
- **Read raw PPM from stdout; never use `-png`.** PNG encoding adds 100–300 ms on the text files and 2.1 s on
  a scanned page (62 ms becomes 2,183 ms). PPM is 5.4–5.9 MB per 1600-pixel page through the pipe.
  - `tb` either turns on the `image` crate's `pnm` feature or reads the `P6` header itself, which takes a few
    lines.
  - Untested: whether `pnm` builds `--offline`. It's code inside the `image` crate, but its dependency list
    wasn't checked.
- **Page position doesn't matter:** page 1004 of 1004 takes about as long as page 1 (35 vs 33 ms), so page up
  and down can render any page on demand.
- **Text:** extract only the pages on screen (`-f`/`-l`). That takes 7–21 ms; the whole 1004-page document takes
  0.8 s.
  - Scanned PDFs have no text (`scan20` gave 1 byte), so the text view has to say so and show the page image
    instead. No OCR.
- **Broken files fail cleanly:** exit code 1 in about 8 ms, nothing on stdout, no hang. The preview shows
  poppler's error.
- **Memory:** 21–28 MiB per run, all freed when the program exits.
- **The worst case needs background rendering and cancelling.** The 200,000-line page takes 16 s at 1600 px,
  roughly proportional to the render size (8 s at 800 px).
  - Turning off anti-aliasing helps: `pdftoppm -aaVector no` takes 4.5 s; `pdftocairo -antialias none` takes
    1.8 s.
  - `pdftocairo` is no faster on normal pages (28 vs 22 ms) and can't send TIFF through a pipe, so `pdftoppm`
    stays the default.
  - So rendering has to:
    - happen off the UI path
    - be killed when the selection moves
    - have a timeout, after which the finder could retry without anti-aliasing or fall back to text
  - Killing a program is the simple way to cancel. A library inside the server would need its own cancel
    mechanism, and pdfium's progressive-render API with a pause callback is from memory, not checked here.
- **Still unknown:**
  - pdfium's speed on these files. It couldn't be installed, because network access from this shell is
    sandboxed.
  - Times with a cold page cache: the first open after a reboot, or files on slow disks.
  - Encrypted PDFs.
  - How often real maps or CAD drawings reach the synthetic vector page's cost.
  - Numbers on other machines. This laptop was in `powersave` with other load.
