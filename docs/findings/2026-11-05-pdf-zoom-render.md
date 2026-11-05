# Finding: rendering zoomed PDF pages with `pdftoppm` (2026-11-05)

## Question

The [PDF preview plan](../plans/2026-11-05-pdf-preview.md) now opens pages at fit-to-width, with zoom. A
zoomed page is bigger than the pane, so the finder can either:

- **render the whole page** at the zoomed size once, and cut out the visible part as you scroll, or
- **render only the visible slice** again on every scroll step.

The choice depends on how render time and memory grow with zoom, and on how fast a slice renders.

## Method

Same machine, tools and test files as [the poppler finding](2026-11-05-pdf-preview-poppler.md). The load
average was about 1.0–1.4.

- **Timings:**
  `python3 docs/findings/scripts/2026-11-05-pdf_measure.py zoom DIR`
  ([script](scripts/2026-11-05-pdf_measure.py)), with the same timing method: median of 15 runs after 2
  warm-ups.
- **The pane:** assumed to be 1000×900 pixels, for example 100×45 cells of 10×20 pixels. Fit-to-width is
  therefore 1000 pixels wide, and zoom ×2, ×3 and ×4 are 2000, 3000 and 4000.
- **Whole page:** `pdftoppm -f 1 -l 1 -scale-to-x W -scale-to-y -1 FILE`.
- **Slice:** the same command at width 4000, with `-x 1500 -y 2000 -W 1000 -H 900`, which gives a pane-sized
  piece from the middle of the page.
- **Peak memory:** checked separately with GNU time 1.9 (`/usr/bin/time -f "%M KiB %e s" CMD > /dev/null`),
  one run each.
  - The script's "max RSS" column turned out to include the Python script's own memory at the moment it starts
    each tool. On Linux, a child's `ru_maxrss` covers the parent's memory before `exec`: `/bin/true` shows
    14 MiB there but 1.1 MiB under GNU time.
  - So that column is only an upper limit. That applies here and in the poppler finding.

## Results

**Timings** (`zoom`; the RSS column is an upper limit, see Method):

| case | median ms | p90 ms | min ms | max RSS MiB | stdout bytes | exit | runs |
|---|---|---|---|---|---|---|---|
| testpage: whole page, width 1000 (×1) | 25.9 | 26.7 | 24.4 | 21 | 3987017 | [0] | 15 |
| testpage: whole page, width 2000 (×2) | 47.7 | 51.8 | 43.8 | 44 | 15948017 | [0] | 15 |
| testpage: whole page, width 3000 (×3) | 76.0 | 77.0 | 72.9 | 62 | 35874017 | [0] | 15 |
| testpage: whole page, width 4000 (×4) | 120.1 | 124.7 | 115.1 | 98 | 63780017 | [0] | 15 |
| testpage: 1000×900 slice of width 4000 (×4), from the middle | 25.3 | 26.3 | 23.5 | 89 | 2700016 | [0] | 15 |
| spec: whole page, width 1000 (×1) | 17.3 | 17.8 | 16.4 | 89 | 3885017 | [0] | 15 |
| spec: whole page, width 2000 (×2) | 36.9 | 39.1 | 35.2 | 89 | 15534017 | [0] | 15 |
| spec: whole page, width 3000 (×3) | 61.2 | 63.9 | 58.1 | 89 | 34947017 | [0] | 15 |
| spec: whole page, width 4000 (×4) | 93.8 | 96.1 | 89.0 | 95 | 62124017 | [0] | 15 |
| spec: 1000×900 slice of width 4000 (×4), from the middle | 14.8 | 16.3 | 13.7 | 89 | 2700016 | [0] | 15 |
| manual: whole page, width 1000 (×1) | 15.8 | 18.6 | 14.3 | 89 | 4248017 | [0] | 15 |
| manual: whole page, width 2000 (×2) | 34.2 | 36.3 | 31.1 | 89 | 16986017 | [0] | 15 |
| manual: whole page, width 3000 (×3) | 59.1 | 62.0 | 56.4 | 89 | 38214017 | [0] | 15 |
| manual: whole page, width 4000 (×4) | 91.4 | 96.4 | 89.8 | 102 | 67932017 | [0] | 15 |
| manual: 1000×900 slice of width 4000 (×4), from the middle | 13.2 | 14.6 | 11.0 | 93 | 2700016 | [0] | 15 |
| longtext: whole page, width 1000 (×1) | 26.5 | 27.3 | 24.9 | 93 | 3885017 | [0] | 15 |
| longtext: whole page, width 2000 (×2) | 52.8 | 54.0 | 51.5 | 93 | 15534017 | [0] | 15 |
| longtext: whole page, width 3000 (×3) | 83.9 | 86.9 | 80.5 | 93 | 34947017 | [0] | 15 |
| longtext: whole page, width 4000 (×4) | 117.8 | 120.1 | 114.2 | 96 | 62124017 | [0] | 15 |
| longtext: 1000×900 slice of width 4000 (×4), from the middle | 25.5 | 26.5 | 23.8 | 93 | 2700016 | [0] | 15 |
| scan20: whole page, width 1000 (×1) | 57.7 | 59.2 | 54.7 | 93 | 4245017 | [0] | 15 |
| scan20: whole page, width 2000 (×2) | 107.9 | 110.5 | 104.2 | 93 | 16980017 | [0] | 15 |
| scan20: whole page, width 3000 (×3) | 245.9 | 252.2 | 238.1 | 100 | 38196017 | [0] | 15 |
| scan20: whole page, width 4000 (×4) | 397.1 | 411.0 | 369.8 | 167 | 67908017 | [0] | 15 |
| scan20: 1000×900 slice of width 4000 (×4), from the middle | 292.3 | 298.8 | 281.1 | 93 | 2700016 | [0] | 15 |

**Peak memory under GNU time** (one run each):

```
/bin/true                                              =>   1152 KiB 0.00 s
pdftoppm -scale-to 1600 spec.pdf                       =>  23808 KiB 0.01 s
pdftoppm -scale-to 1600 scan20.pdf                     =>  28032 KiB 0.05 s
pdftoppm -scale-to 1600 longtext.pdf                   =>  25344 KiB 0.03 s
pdftoppm -scale-to 1600 vector.pdf                     =>  23388 KiB 15.54 s
pdftoppm width 4000 spec.pdf                           =>  97280 KiB 0.06 s
pdftoppm width 4000 scan20.pdf                         => 170240 KiB 0.33 s
pdftoppm width 4000 slice 1000×900 spec.pdf            =>  19712 KiB 0.01 s
pdftoppm width 4000 slice 1000×900 scan20.pdf          =>  85376 KiB 0.27 s
```

(All with `-f 1 -l 1`; "width 4000" is `-scale-to-x 4000 -scale-to-y -1`; the slice adds
`-x 1500 -y 2000 -W 1000 -H 900`.)

## Conclusions

- **Render time grows with the page's area.** Text pages take 16–27 ms at ×1, 34–53 ms at ×2, 59–84 ms at ×3
  and 91–120 ms at ×4. The scanned page takes 58, 108, 246 and 397 ms.
- **The page must be held in memory.**
  - **Size:** the whole page at ×4 is 62–68 MB of pixels (width × height × 3, the stdout bytes) for an A4
    page; ×2 is 16–17 MB.
  - **pdftoppm's own peak** while rendering is 97–170 MiB at ×4, against 24–28 MiB at the 1600-pixel size.
    That memory is freed when it exits.
- **Slices are fast for text pages and slow for scans.**
  - **Text pages:** a pane-sized slice of a ×4 page takes 13–26 ms, about the cost of a whole page at ×1.
  - **Scans:** a slice takes 292 ms, against 397 ms for the whole page, because poppler still decodes the
    whole scanned image.
  - So rendering a slice on every scroll step would make scrolling scans lag by about 0.3 s per step.
- **For the plan:**
  - **Render the whole page** at the current zoom, once per page and zoom change, and scroll by cutting from
    it. Changing zoom or page costs at most 0.4 s, on a scan at ×4; scrolling costs no poppler call at all.
  - **Cap the page size** so a tall or huge page can't use unbounded memory. Above the cap, render at the
    cap and accept some blur.
    - At ×4 on HiDPI cells (20 pixels wide), an A4 page would be 8000×11300 pixels, or about 270 MB.
- **Still unknown:**
  - **Encoding cost in `tb`:** what one scroll step costs to cut out, encode and send to the terminal: PNG
    and base64 for kitty and iTerm2, `tb`'s own encoder for sixel. That needs `tb`'s code, so it's measured
    while building.
  - **Times at other pane sizes:** these figures assume a 1000-pixel-wide pane.
