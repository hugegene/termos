# Finding: what a PDF scroll step costs in `tb`, and listing page sizes (2026-11-05)

## Question

Two facts the [PDF preview plan](../plans/2026-11-05-pdf-preview.md) needed while it was being built:

1. **Step 7 of the plan:** when the page view scrolls, `tb` cuts the visible part out of the rendered page and
   encodes it for the terminal. If a step took over about 50 ms, the plan called for two fixes:
   - for kitty, sending the whole page once and moving the visible part with the protocol's source rectangle
   - for the other protocols, dropping steps
2. **The pixel cap needs each page's shape.** Can `pdfinfo` list every page's size cheaply, so the worker can
   get them all at once?

## Method

Dev machine: Intel Core i7-12650H (16 CPUs), 31 GiB RAM, Ubuntu, Linux 6.8.0-147, CPU governor `powersave`.
`poppler-utils` 24.02.0. The test files are the ones from
[the poppler finding](2026-11-05-pdf-preview-poppler.md): `spec.pdf`, `longtext.pdf` and `scan20.pdf`.

**1. Scroll step:** the ignored test `scroll_step_cost` in `crates/tb/src/server/images.rs`, release build,
`tb` at this commit's code:

```
TB_BENCH_PDF=<file> cargo test --offline --release -p tb --bin tb scroll_step_cost -- --ignored --nocapture
```

- **The page:** page 1 of the file, rendered 1000 pixels wide with `finder_core::pdf::page`, which is the
  fit-to-width render for a 100-column pane of 10-pixel cells.
- **The pieces:** 10 pane-sized cut-outs (1000×900) at the offsets `j` scrolls to: 3 rows of 20 pixels each,
  from the top.
- **Timed:** the cut-out (`image::imageops::crop_imm(…).to_image()`), and `images::encode` for each protocol
  at 1000×900 px and 100×45 cells.
  - kitty and iTerm2 are PNG (`CompressionType::Fast`) plus base64.
  - Sixel is `tb`'s own encoder.
- **Reported:** the median of the 10 runs, and the median size of the encoded bytes that would be sent.
- **Load:** the load average was 4.1 just after the release build, so these numbers are on the high side.

**2. Page sizes:** median of 9 runs of each command through `subprocess.run` (Python 3.12):

```python
for args in (["pdfinfo", f], ["pdfinfo", "-f", "1", "-l", str(pages), f]):
    times = []
    for _ in range(9):
        s = time.perf_counter(); subprocess.run(args, capture_output=True); times.append(time.perf_counter() - s)
```

The output format was checked with `pdfinfo -f 1 -l 3 spec.pdf`, which gives `Page    1 size:  609.714 x
789.041 pts` and `Page    1 rot:   0` for each page. A last page past the end (`-l 1000000` on a 2-page file)
lists every page and exits 0.

## Results

**1. One scroll step** (median of 10):

| file | page rendered | cut out | kitty | iTerm2 | sixel |
|---|---|---|---|---|---|
| `spec.pdf` (text) | 1000×1295 | 1.2 ms | 1.5 ms, 288,796 bytes | 1.5 ms, 288,196 bytes | 18.4 ms, 100,802 bytes |
| `longtext.pdf` (dense text) | 1000×1295 | 1.0 ms | 2.9 ms, 779,251 bytes | 3.0 ms, 777,580 bytes | 18.3 ms, 303,326 bytes |
| `scan20.pdf` (scanned) | 1000×1415 | 1.0 ms | 5.8 ms, 1,569,312 bytes | 5.9 ms, 1,565,905 bytes | 17.9 ms, 391,121 bytes |

**2. `pdfinfo`, with and without every page's size:**

```
spec         19 pages: pdfinfo 9.5 ms, pdfinfo -f 1 -l 19 11.4 ms
manual       93 pages: pdfinfo 6.9 ms, pdfinfo -f 1 -l 93 7.4 ms
longtext   1004 pages: pdfinfo 17.4 ms, pdfinfo -f 1 -l 1004 23.6 ms
scan20       20 pages: pdfinfo 6.5 ms, pdfinfo -f 1 -l 20 7.8 ms
vector        1 pages: pdfinfo 5.0 ms, pdfinfo -f 1 -l 1 7.0 ms
```

## Conclusions

- **A scroll step costs 3–25 ms** before sending: about 1 ms to cut out, then 1.5–6 ms to encode for kitty and
  iTerm2, or about 18 ms for sixel.
  - That's under the plan's 50 ms threshold, so the two fixes from step 7 aren't needed.
  - The encoder thread already skips steps that a newer one overtakes (`Images::new`).
- **What's sent per step is large for kitty and iTerm2:** 0.3–1.6 MB, against 0.1–0.4 MB for sixel.
  - Locally that doesn't matter. Over SSH or to a phone it might. At 10 MB/s, 1.6 MB is 160 ms; that's an
    **estimate**, since transfer wasn't measured.
  - If scrolling feels slow over a network, kitty's source rectangle is the fix: send the page once, then
    only move the visible part.
- **Listing every page's size adds 0.5–6 ms** to `pdfinfo`, so the worker gets all the sizes in its one
  `pdfinfo` call per file.
- **Not measured:**
  - how long the terminal takes to decode and draw each picture
  - scrolling over SSH
