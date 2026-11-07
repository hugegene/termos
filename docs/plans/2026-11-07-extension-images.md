# Plan: pictures from extensions

**Status:** proposed, waiting for the user's choice · **Date:** 2026-11-07 · **Related:**
[the CLI as the extension API](2026-11-05-cli-extensions.md), [the finder as two panes](2026-11-05-finder-divider.md)
(why the finder is built into the server), [rebuild plan](2026-11-03-rebuild-plan.md) · **Findings:**
[image escapes in panes](../findings/2026-11-07-image-escapes-in-panes.md),
[pane pixel sizes](../findings/2026-11-04-pane-pixel-sizes.md), [dependency licences](../findings/2026-11-03-dependency-licenses.md)

## Summary

**Programs in a `tb` pane can't show pictures today; only the finder can.** A program's output goes through the
pane's `alacritty_terminal` fork, and the parser under it drops image escapes
([finding](../findings/2026-11-07-image-escapes-in-panes.md)). The finder is code inside the server, so it never
passes through a terminal pane. Extension authors should be able to show pictures too.

**Recommendation** (the user hasn't chosen yet):
- **A. Pictures inside terminal panes, sixel first.** Programs that already write sixel (chafa, timg, lsix,
  yazi) work unchanged, and there's no new API to keep stable. tmux (3.4 and later, built with sixel) and zellij
  handle sixel the same way.
- **B. Picture overlays from the CLI:** a small, optional first step for extensions written for `tb`.
- **C. A canvas pane:** only if a real need comes up that A and B can't meet.
- **D. Sending a program's output straight to your terminal:** can't work (see
  [Options considered](#options-considered)).
- **The finder stays in the server.**
  - Moving it onto C would bring back the costs the [finder plan](2026-11-05-finder-divider.md) rejected on
    2026-11-05: rebuilding its state, saving and the embedded Neovim outside the server, and breaking the phone
    plan's shared `finder-core`.
  - Its one real gain would be crash isolation: today a panic in finder code ends the session server. Catching
    panics around the finder's calls gets that more cheaply.

## How each option works

### What all of them share

The hard half already exists for the finder: fitting a picture into a rectangle, encoding it in each client's
format (kitty, iTerm2, sixel or blocks) and placing it on screen. Each option is only a new way to produce an
`ImageSlot` for it.

```
 today's finder    render_preview() ─────────────────────┐
                                                         │
 option A          a picture in a pane's grid, ──────────┤
                   sent by the program as sixel          │
                                                         ├──▶ ImageSlot ──▶ compose(): fit + encode
 option B          an overlay set with ──────────────────┤                  for each client
                   tb action show-image                  │                        ▼
                                                         │                  your terminal
 option C          a picture message from ───────────────┘
                   a canvas program
```

### Today: the finder

The picture is made inside the server, so it never touches a terminal pane.

```
 tb server process
 ┌──────────────────────────────────────────────────────────────────────────┐
 │ finder code (server/finder/)                                             │
 │   preview worker thread                                                  │
 │     image file → decoded with the `image` crate → RgbImage               │
 │     PDF page   → pdftoppm (poppler)              → RgbImage              │
 │       ▼                                                                  │
 │   render_preview(rect) → cells + ImageSlot { id, img, area }             │
 │       ▼                                                                  │
 │ compose(), for each client: fit → encode kitty | iTerm2 | sixel | blocks │
 └───────┬──────────────────────────────────────────────────────────────────┘
         ▼ ServerMsg::Output
   tb attach ──▶ your terminal
```

### Option A: pictures inside terminal panes

The picture travels inside the program's normal output. The pane's terminal learns to keep it instead of
dropping it.

```
 program in a pane: chafa, timg, lsix, yazi in sixel mode, or an extension
   │ writes standard sixel: ESC P q <pixels> ESC \      nothing tb-specific
   ▼
 PTY
   ▼
 tb server process
 ┌──────────────────────────────────────────────────────────────────────────┐
 │ vte, vendored like the fork                                        NEW   │
 │   hook / put / unhook hand over the sixel bytes instead of dropping them │
 │       ▼                                                                  │
 │ alacritty_terminal fork                                            NEW   │
 │   decode the sixel → RgbImage                                            │
 │   store it in the grid at the cursor: "picture 12 at line 40, col 0,     │
 │   20×10 cells". It scrolls with the text, goes into the scrollback,      │
 │   and goes away when the screen is cleared                               │
 │       ▼                                                                  │
 │ draw_term(rect) → cells as today + an ImageSlot per visible picture  NEW │
 │       ▼                                                                  │
 │ compose(), for each client: fit → encode kitty | iTerm2 | sixel | blocks │
 └───────┬──────────────────────────────────────────────────────────────────┘
         ▼ ServerMsg::Output
   tb attach ──▶ your terminal
```

- **Changes on both sides of the fork's boundary:**
  - `vte` and the fork learn to keep pictures.
  - `draw_term` (`tb`'s code in `server/render.rs`, which reads the fork's grid) learns to return them.
- **Panes must say they support sixel.** Today the fork answers device attributes with `ESC[?6c`, which has no
  `4`, so programs don't try sixel. Programs also ask for the cell size (`CSI 16 t`), which `vte` 0.15 drops
  ([finding](../findings/2026-11-04-pane-pixel-sizes.md)).
- **Each client still gets its own format:** the server decodes the sixel and encodes it again for each client.
  This answers the finder plan's objection that "a program in a pane would draw one format for all".
- **kitty graphics in panes is harder:** `vte` skips kitty's escapes without reporting them at all. Sixel comes
  first; kitty comes later or never.

### Option B: picture overlays from the CLI

Text and pictures take separate roads. The pane's terminal stays unchanged, and the picture arrives through a
`tb action` call.

```
 extension in a pane: a ueberzug-style previewer
   │ text, as today                     │ a picture: a separate command
   ▼                                    ▼
 PTY                       tb action show-image --pane 3 --at 40,2 \
   │                                        --size 30x15 cat.png
   │                                    │ the session socket, like any tb action
   ▼                                    ▼
 tb server process
 ┌──────────────────────────────────────────────────────────────────────────┐
 │ vte + fork: unchanged            pane 3's overlays:                NEW   │
 │ grid: text cells only              cat.png decoded, col 40, row 2,       │
 │                                    30×15 cells                           │
 │       ▼                                  ▼                               │
 │ draw_term(rect) → cells          an ImageSlot at the pane's        NEW   │
 │       │                          corner + (40, 2)                        │
 │       └──────────────┬───────────────────┘                               │
 │                      ▼                                                   │
 │ compose(), for each client: fit → encode kitty | iTerm2 | sixel | blocks │
 └──────────────────────┬───────────────────────────────────────────────────┘
                        ▼ ServerMsg::Output
                  tb attach ──▶ your terminal
```

- **The fork is used, unchanged, for the text.** Only the picture skips it.
- **Everything new is `tb`'s code:** one action, a list of overlays per pane, and a few lines in `draw_pane`.
- **The picture is pinned to the pane, not the text.** The extension moves or removes it when its text moves,
  as ueberzug previewers in lf, ranger and yazi already do.
- **An idea (not checked):** a small program that speaks ueberzugpp's commands and calls `tb` could let those
  file managers use B unchanged.

### Option C: a canvas pane

The program never writes escape codes. It sends the server messages describing cells and pictures, and the
server keeps the pane's content. This is the closest to "a `tb` pane that doesn't go through
`alacritty_terminal`".

```
 extension, started with: tb canvas -- my-viewer
   │ stdout: messages to the server              ▲ stdin: messages from the server
   │   cells    {"at":[0,0],"text":"…"}          │   key     {"key":"j"}
   │   picture  {"id":1,"png":"<base64>",        │   mouse   {"click":[12,3]}
   │             "at":[40,2],"size":[30,15]}     │   resize  {"cols":80,"rows":24}
   │   remove   {"picture":1}                    │
   ▼ pipes, no PTY                               │
 ┌───────────────────────────────────────────────┴──────────────────────────┐
 │ tb server process                                                        │
 │ no vte, no fork                                                          │
 │ Pane::Canvas: the pane's cells and pictures, kept by the server    NEW   │
 │   updated from each message; kept for redraws and reattaching            │
 │       ▼                                                                  │
 │ draw_pane → copy the cells + an ImageSlot per picture              NEW   │
 │       ▼                                                                  │
 │ compose(), for each client: fit → encode kitty | iTerm2 | sixel | blocks │
 └───────┬──────────────────────────────────────────────────────────────────┘
         ▼ ServerMsg::Output
   tb attach ──▶ your terminal
```

The messages above only show the idea; their format isn't designed.

- **Ordinary terminal libraries don't work as-is.** ratatui with crossterm, curses and the like write escape
  codes. We would ship a ratatui backend for Rust; other languages would write the messages themselves.
- **The server keeps the content,** so redraws, reattaching, several clients and each client's picture format
  all work.
- **No scrollback or copy mode** unless we build them for canvas panes. `dump-screen` could read the cells.
- **The largest API to keep stable:** under the extension API's rules, messages can be added but never changed.
- **The finder could move onto it.** See the summary for why it shouldn't, for now.

### When the text scrolls

```
 A: the picture is part of the pane's text     B: the picture is pinned to the pane
    before       after 3 more lines               before       after 3 more lines
    ┌──────────┐ ┌──────────┐                    ┌──────────┐ ┌──────────┐
    │ $ chafa  │ │ ▓▓▓▓▓▓   │                    │ $ ls     │ │ b.txt    │
    │ ▓▓▓▓▓▓   │ │ $ ls     │                    │ ▓▓▓▓▓▓   │ │ ▓▓▓▓▓▓   │
    │ ▓▓▓▓▓▓   │ │ a.txt    │                    │ ▓▓▓▓▓▓   │ │ ▓▓▓▓▓▓   │
    │ $        │ │ b.txt    │                    │ a.txt    │ │ $        │
    └──────────┘ └──────────┘                    └──────────┘ └──────────┘
    moves up with the text                       stays put; text slides under it
                                                 until the extension moves it
```

In C there's no scrolling text: the program redraws whatever it wants, the way the finder does.

### What goes through `alacritty_terminal`

| | Text | Picture |
|---|---|---|
| Today's finder | no (the server draws it) | no |
| A | yes | yes (the fork learns sixel) |
| B | yes | no (an overlay from `tb action`) |
| C | no (cells sent as messages) | no (pictures sent as messages) |

## Options considered

| Option | What extension authors do | Size (**estimate**) | Verdict |
|---|---|---|---|
| **A. Pictures inside terminal panes** | Nothing new: write sixel, as chafa, timg, lsix and yazi already do | 1.5–3k lines, including the `vte` changes | ✅ Recommended. Existing tools work, and sixel is a standard, so there's no API to keep stable |
| **B. Picture overlays from the CLI** | Call `tb action show-image` and move the picture when the text moves | 400–700 lines | ✅ A cheap first step that fits "the CLI is the API". Only programs written for `tb` benefit |
| **C. A canvas pane** | Send cells and pictures as messages, or use a ratatui backend we provide | 2–4k lines, plus the backend and a protocol we can never break | ⚠️ Only if A and B can't meet a real need |
| **D. A pane whose output goes straight to your terminal** | – | – | ❌ Can't work. A program's cursor moves and clears assume it owns the whole screen, so `ESC[2J` would wipe every pane, and translating that is a terminal emulator's job. One byte stream can't suit clients of different sizes and picture formats. The server would have nothing to redraw from on reattach, and scrollback, copy mode, `dump-screen` and `subscribe -p` would have no grid to read |
| **Move the finder out of the server, onto C** | – | a rewrite of the finder's glue | ❌ Not now: the costs the finder plan listed on 2026-11-05 still apply. Crash isolation is cheaper with caught panics |

## Steps

Only the steps for the option the user chooses are carried out.

### A: pictures inside terminal panes

- [ ] **Measure sixel decoders** and record a finding:
  - candidates: `icy_sixel` 0.7.0 (does it decode, or only encode?), and zellij's `sixel-image` and
    `sixel-tokenizer`
  - for each: its version, licence and dependencies, and how long it takes to decode a 1000×1000 picture
  - none is in the local cargo registry, so the user runs `cargo fetch` for the chosen one
- [ ] **Vendor `vte` 0.15.0** as `crates/vte`, unmodified at first, with an `UPSTREAM.md` like the fork's. Check
  `cargo deny check licenses`.
- [ ] **In `vte`'s `ansi` layer:** pass sixel (DCS ending in `q`) to new `Handler` methods instead of logging it
  as unhandled. Record the change in its `UPSTREAM.md`.
- [ ] **In the fork** (record every change in its `UPSTREAM.md`):
  - decode the sixel into an `RgbImage`
  - store pictures by the grid cells they cover, so they scroll with the text, go into the scrollback, and are
    removed when cleared or written over
  - say sixel is supported: add `4` to the device-attributes reply (today `ESC[?6c`), and answer `CSI 16 t`
    (cell size) and XTSMGRAPHICS (colour registers)
- [ ] **In `tb`:**
  - `draw_term` returns an `ImageSlot` for each visible picture; `compose` already does the rest
  - copy mode and `dump-screen` leave pictures out
- [ ] **Limits:** the largest picture, and the total picture memory per pane. The numbers are to be measured.
- [ ] **Tests:**
  - unit tests for decoding, and for pictures scrolling with the text
  - an end-to-end test that prints a sixel file in a pane and checks the client gets a picture
    (`TB_IMAGES=kitty`) and blocks (`TB_IMAGES=blocks`)
  - a check by hand with chafa and yazi in xterm, iTerm2 and kitty
- [ ] **Docs:** a section on pictures in panes in `docs/cli.md`.

### B: picture overlays from the CLI

- [ ] **Commands:**
  - `tb action show-image [-p ID] --at COL,ROW --size COLSxROWS [--id NAME] FILE` (`-` reads stdin); prints the
    picture's id
  - `tb action hide-image [-p ID] [--id NAME]`; without `--id`, removes all of the pane's pictures
- [ ] **Server:**
  - a list of overlays for each terminal pane, decoded with the `image` crate (already a dependency)
  - `draw_pane` adds an `ImageSlot` for each overlay, clipped to the pane
- [ ] **Lifetime:** overlays go when their pane closes. They aren't saved with the session.
- [ ] **Docs:**
  - run `TB_UPDATE_DOCS=1 cargo test -p tb --bin tb cli_docs`
  - add overlays and their ids to `docs/cli.md`
- [ ] **Tests:** an end-to-end test that shows, moves and hides a picture, and checks it goes when the pane
  closes.

### C: a canvas pane

No steps unless it's chosen. It would need its message format designed first.

## Open questions

1. **Which to build:** A, B then A, or something else?
2. **Formats in panes:** sixel only, or kitty graphics later? kitty needs `vte` to report its escapes at all.
3. **Saving:** should pictures in panes come back with a restored session? The proposal is no: programs draw
   them again.

## Change log

- **2026-11-07:** written. The user asked whether a pane that doesn't go through `alacritty_terminal` could let
  the finder and extensions show pictures.
