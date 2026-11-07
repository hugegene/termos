# Plan: faster PDF page scrolling over SSH, with kitty's source rectangle

**Status:** proposed · **Date:** 2026-11-07 · **Related:** [PDF preview](2026-11-05-pdf-preview.md) (step 7 named
this fix) · **Findings:** [source rectangles in Ghostty](../findings/2026-11-07-kitty-source-rect.md),
[a PDF scroll step](../findings/2026-11-05-pdf-scroll-step.md), [rendering zoomed pages](../findings/2026-11-05-pdf-zoom-render.md)

## Decision

**For terminals that support it, `tb` sends a PDF page's picture once, and each scroll step only moves the
visible part.**

- **Today:** every step in the page view cuts out the visible part and sends it as a new picture: 0.3–1.6 MB
  for kitty (scroll-step finding). That's fine locally, but at 10 MB/s over SSH it's up to 160 ms a step
  (**estimate**).
- **With this plan:** a step is one 68-byte placement (`a=p` with `x,y,w,h`), whatever the page holds.

- **Which terminals:** those that speak kitty graphics **and** are known to draw source rectangles.
  - **Ghostty** passed the check in the finding (1.3.1): it draws the rectangle scaled into its cells, and placing
    again with the same placement id replaces the picture.
  - **kitty**, the protocol's author, is assumed to work. It isn't installed here, so that's an **estimate**,
    checked by hand in step 8.
  - **How `tb` knows:** the client adds the XTVERSION query (`CSI > 0 q`) to the questions it already asks at
    start. Ghostty answers `ghostty 1.3.1`. This works over SSH, because the terminal answers, not the
    environment. `TERM_PROGRAM` usually doesn't reach the far side.
  - **Everyone else** (other kitty-graphics terminals, iTerm2 and sixel) keeps today's cut-outs. iTerm2's and
    sixel's protocols have no source rectangle.
  - **`TB_SOURCE_RECT=on|off`** overrides the choice, to try another terminal or to turn it off.
- **How it draws, for each client:**
  1. **On a new page or zoom:**
     - delete the old page's picture from the terminal (`a=d,d=I`)
     - send the whole page once, without showing it (`a=t`, PNG, as today's encoder makes it)
     - place it (`a=p,i=PAGE,p=1,x=LEFT,y=TOP,w,h,c,r,C=1,q=2`) at the pane's top-left
  2. **Each scroll or pan step:** the same placement with the new `x` and `y`. It replaces the last one, so
     nothing flashes and nothing is deleted.
  3. **While a new page is still being encoded:** the old placement stays up. `still_shown` already does this.
  4. **A full redraw** deletes every picture (`a=d,d=A`, as now), so the page is sent again.
- **Size cap:** the whole page is sent only up to **6 megapixels** (**estimate**).
  - That covers fit width and 150–200% zoom on a 1000-pixel pane.
  - Bigger pages (300% and 400%) keep today's cut-outs. Sending the whole of one would be about 4–40 MB, also an
    **estimate**, scaled by area from the finding.
  - Step 1 measures the real sizes and sets the cap.
- **Cost:**
  - **A page turn sends somewhat more than today:** the whole page at fit width is about 0.4–2.5 MB, against
    0.3–1.6 MB for a cut-out (**estimate**).
  - **The scroll steps on that page then cost almost nothing.** A fit-width page is about 7–9 steps of `j`
    (1295–1415 pixels tall, a 900-pixel view, 60 pixels a step). So a page read top to bottom drops from about
    2–16 MB to 0.4–2.5 MB (**estimate**).
- **Memory in the terminal:** one page per previewing finder, per client. At the cap that's about 24 MB as RGBA
  (**estimate**; Ghostty's own storage and its limit weren't measured).
- **Not changed:** image previews, fit-page zoom (the whole page is already one picture), the encoder thread, and
  every non-kitty client.
- **Protocol:** the attach message gains the terminal's name, so the version goes to 4. It shares that change
  with the [video plan](2026-11-07-video-and-office-previews.md) and the [preview action](2026-11-07-preview-action.md)
  if they're built together.
- **Size:** about 300–500 lines plus tests (**estimate**).

## Options considered

| Option | Verdict |
|---|---|
| Cut-outs, as today | ❌ 0.3–1.6 MB a step over SSH |
| **Whole page once, then a source rectangle per step, up to a size cap** | ✅ 68 bytes a step; one placement; works in Ghostty (finding) |
| Tiles: send the page in pieces as they come into view, placing up to four per step | Later, if zoomed scrolling over SSH is slow. It needs no cap, but several placements a step, and per-client tile bookkeeping |
| Send a cut-out first, then the whole page behind it, for a faster first view over SSH | Later, if page turns feel slow over SSH |
| Use source rectangles for every terminal that answers kitty's query | ❌ WezTerm, Konsole and others weren't tested. A terminal that ignores `x,y,w,h` would show the whole page squashed into the pane |
| Recognise terminals by `TERM_PROGRAM` or `TERM` | ❌ Usually lost over SSH, which is exactly where this matters |
| **XTVERSION** | ✅ Answered by the terminal itself (finding) |
| Something similar for sixel or iTerm2 | ❌ Neither protocol has a source rectangle. Sixel steps are already smaller (0.1–0.4 MB) |

## Steps

- [ ] **1. Measure the whole page** and record a finding:
  - Extend the ignored test `scroll_step_cost` (`crates/tb/src/server/images.rs`) to encode the whole page at
    each zoom, as kitty would.
  - Record the bytes and encode time on `spec.pdf`, `longtext.pdf` and `scan20.pdf`, from the PDF findings.
  - Set the cap from it.
- [ ] **2. Client** (`client.rs`, `protocol.rs`):
  - add `CSI > 0 q` before the device-attributes query, and parse `DCS > | NAME VERSION ST`
  - `Graphics` gains `source_rect: bool`: Ghostty or kitty, or `TB_SOURCE_RECT`
  - bump `PROTOCOL_VERSION` (shared, see above)
  - unit tests on recorded replies, Ghostty's from the finding among them
- [ ] **3. Encoding** (`images.rs`):
  - `kitty_transmit(png, id)` (`a=t`) and `kitty_place(id, rect, cols, rows)` (`a=p` with `p=1`)
  - unit tests of their exact bytes
- [ ] **4. Server** (`server/mod.rs`, `images.rs`):
  - An `ImageSlot` can carry the whole page and the visible rectangle, beside the cut-out.
  - For a `source_rect` client under the cap: transmit once, then place.
    - Each client remembers which pages its terminal holds; a full redraw forgets them.
    - `changes()` sends only a placement when only the rectangle moved.
    - The old page is deleted when a new one is sent.
  - Other clients use the cut-out, as now. The cut-out is only made when some client needs it.
- [ ] **5. Finder** (`finder/mod.rs`, `render_pdf`): pass the page picture, its id and the viewport rectangle.
- [ ] **6. Tests:**
  - **Unit:**
    - `changes()`: a moved rectangle gives a placement, no delete and no transmit
    - a page turn gives delete, transmit and place
    - a full redraw transmits again
  - **End to end,** with `TB_IMAGES=kitty TB_SOURCE_RECT=on`, on the two-page fixture:
    - after `t`, one `a=t` and one `a=p`
    - each `j` sends only `a=p`, with `y` growing
    - `]` sends `a=d,d=I` for page 1, then `a=t` for page 2
    - at 400% (over the cap), steps send `a=T` cut-outs again
    - with `TB_SOURCE_RECT=off`, steps send `a=T` cut-outs, as today
- [ ] **7. Docs:**
  - a change-log line in the [PDF plan](2026-11-05-pdf-preview.md)
  - `TB_SOURCE_RECT` in the README, beside `TB_IMAGES`
- [ ] **8. Check by hand (you):**
  - Ghostty, locally and attached over SSH: a long PDF in the page view, holding `j`, at fit width and 200%;
    then 400% (cut-outs)
  - kitty, if you have it anywhere

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tb -p finder-core`.

## Open questions

None. The terminal check that could have changed the plan was done (finding). kitty and other terminals are
checked by hand in step 8, or opted in with `TB_SOURCE_RECT=on`.

## Change log

- **2026-11-07:** written, from the [source rectangle finding](../findings/2026-11-07-kitty-source-rect.md). The
  user asked for faster PDF scrolling over SSH, using kitty's source rectangle so each step doesn't resend
  0.3–1.6 MB.
