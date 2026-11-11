# Finding: `[` and `]` past a spreadsheet's first or last sheet, and the status bar's caption (2026-11-11)

## Question

You reported that on a spreadsheet, pressing `[` or `]` past the first or last sheet loses the caption for good.
The [grid copy and open in app plan](../plans/2026-11-11-grid-copy-and-open-in-app.md) needs to know what actually
happens, to decide what to fix.

## Method

Same machine and build as the [captions finding](2026-11-11-finder-captions.md): the working tree, built with
`cargo build --offline` (debug) at 19:17. Your own session runs `target/release/tos`, built at 19:19 from the same
working tree; its server started at 19:43.

A throwaway session in tmux, read back with `capture-pane`, with `crates/finder-core/tests/data/sheet.xlsx` (two
sheets, Budget and Notes):

```
mkdir -p /tmp/tq/s /tmp/tq/st /tmp/tq/demo && cp crates/finder-core/tests/data/sheet.xlsx /tmp/tq/demo/
tmux -L tq new-session -d -x 200 -y 20 -e TOS_SOCKET_DIR=/tmp/tq/s -e TOS_STATE_DIR=/tmp/tq/st \
     "cd /tmp/tq/demo && target/debug/tos finder /tmp/tq/demo"
tmux -L tq send-keys -t 0 l     # focus the preview
tmux -L tq send-keys -t 0 '['   # on sheet 1: past the first
tmux -L tq capture-pane -p | tail -1; date +%T      # at once, then again 4 s later
# the same with ']' on sheet 2, in the grid (Enter, then [ and ]), and at 100 columns
tmux -L tq resize-window -t 0 -x 100 -y 20
tmux -L tq capture-pane -p | sed -n 2p | sed 's/─//g'   # the frame title
```

## Results

**Preview, 200 columns:**

```
before:            … <q> QUIT        spreadsheet · j/k scroll · [ ] sheet · Enter grid · h back
[ on sheet 1:      first sheet                                                    (21:17:58)
4 s later:         … <q> QUIT        spreadsheet · j/k scroll · [ ] sheet · Enter grid · h back   (21:18:02)
] on sheet 2:      last sheet                                                     (21:18:19)
5 s later:         … <q> QUIT        spreadsheet · j/k scroll · [ ] sheet · Enter grid · h back   (21:18:24)
```

The preview's frame title stays right throughout: `xlsx · sheet 1/2 Budget · 3 rows · 6.6 KB`, then
`xlsx · sheet 2/2 Notes · 1 rows · 6.6 KB`.

**Grid, 200 columns:** the status bar keeps `read-only · arrows move · [ ] sheet · g/G top/bottom · q close`. The
grid's own last row says `first sheet` or `last sheet` until the next key. The frame title says
`sheet 1/2 Budget · row 0/2 · col 1 · read-only`, or `sheet 2/2 Notes …`.

**Preview, 100 columns:**

```
before:            Ctrl +  <g> LOCK  <p> PANE  <t> TAB  <n> RESIZE  <h> MOVE  <s> SCROLL  <o> SESSION  <q> QUIT spread
] on sheet 2:      last sheet                                                     (21:18:56)
5 s later:         Ctrl +  <g> LOCK  <p> PANE  <t> TAB  <n> RESIZE  <h> MOVE  <s> SCROLL  <o> SESSION  <q> QUIT spread
                                                                                  (21:19:01)
```

**PDFs, for comparison** (from the code): `[` and `]` at the first or last page do nothing and say nothing.
`PdfView::go_to` clamps the page (`finder/pdf.rs:275–283`), and the text view's jump stops at the top or bottom
(`finder/mod.rs:686–695`).

## Conclusions

1. **Not reproduced as "forever".** In both the preview and the grid, at 200 and 100 columns, the message replaced
   the status bar's caption for the 3 seconds a message lasts (`MESSAGE_TIME`, `server/mod.rs:82`), and then the
   caption came back.
2. **A possible cause of what you saw:** the status bar keeps tos's full key list (93 columns), so in a terminal
   narrower than about 130 columns, little or nothing of the caption is left.
   - At 100 columns only `spread` (the start of `spreadsheet`) shows, before and after the message. That's a hint
     cut mid-word, which is when the first hint alone is too wide (`server/keys.rs:609–618`).
   - At 93 and 94 columns, nothing shows at all.
   - Below 93, the key list switches to its short form (64 columns), and some room comes back.

   This is from the code's widths, not tried.
3. **Either way, the message isn't needed.** The frame title already says `sheet 2/2`, and PDFs say nothing at their
   first and last page. So the plan stops sending it, and nothing replaces the caption.

**Still unknown:** whether your "forever" came from something this didn't cover: another width, an SSH session, or
more than one client attached.
