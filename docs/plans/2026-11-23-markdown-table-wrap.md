# Plan: Markdown tables show all of every cell, wrapping long cells onto more lines

## Decision

In the finder's Markdown preview and the docs pane, a table cell that doesn't fit its column **wraps onto more
lines** instead of being cut with "…". This covers `.md` and `.html` files, and Word and PowerPoint files, which
the preview turns into Markdown first.

- **Columns:** each starts as wide as its widest cell, with no 40-column cap. While the table is wider than
  the pane, the widest column loses one column at a time, down to 3, as now.
- **Cells:** wrap at spaces. A word longer than its column is split at the edge, as in paragraphs.
- **`<br>` in a cell** (`<br>`, `<br/>` or `<br />`) starts a new line in the cell. GitHub tables use it for
  this. Until now the preview showed it as the text `<br>`.
- **Rows:** when any row of a table takes more than one line, a dim rule separates every row, so you can see
  where each one ends. A table where every cell fits looks as it does now.
- **Links** in a wrapped cell can be clicked on each line they cover.
- **CSV files and spreadsheets don't change.** They keep one line per row and cut long cells. They can have
  thousands of rows, and the grid editor shows a whole cell.

Nothing was measured; this rests on reading `crates/tos/src/server/finder/view.rs`.

## Example

`docs/plans/x.md` has this table, in a pane 44 columns wide:

```markdown
| Option | Pros | Cons |
|---|---|---|
| A | fast to build, no new dependency | cuts long cells |
| B | shows everything<br>links still work | taller tables |
```

The columns' natural widths are 6, 32 and 15 (the `<br>` splits B's Pros into two lines of 16). That's 53, plus
6 for the two " │ " gaps, or 59, which doesn't fit 44. The widest column, Pros, shrinks one column at a time until
the table fits, at 17: 6 + 17 + 15 + 6 = 44.

**Before:** each cell on one line, cut. Before this change `<br>` was text, so B's Pros was 36 wide.

```
Option │ Pros              │ Cons
───────┼───────────────────┼────────────────
A      │ fast to build, n… │ cuts long cells
B      │ shows everything… │ taller tables
```

**After:** the cells wrap, and rules separate the rows. This is the test `table_cells_wrap` in `view.rs`.

```
Option │ Pros              │ Cons
───────┼───────────────────┼────────────────
A      │ fast to build, no │ cuts long cells
       │ new dependency    │
───────┼───────────────────┼────────────────
B      │ shows everything  │ taller tables
       │ links still work  │
```

How a table is drawn, inside `view.rs`:

```
  pulldown-cmark events            Md (the renderer)                     preview lines
 ┌────────────────────┐  (1) Text, Link, <br>  ┌────────────────────┐  (4) one line per wrapped
 │ Start(TableCell) … │───────────────────────▶│ cell text + link   │      line of the row, then
 │ End(TableCell)     │                        │ byte ranges        │      a rule if rows wrap
 └────────────────────┘                        │                    │─────────────────────────▶
                          (2) End(Table):      │ (3) wrap each cell │      + Link { line, start,
                              column widths    │     to its column  │        end } per piece
                                               └────────────────────┘
```

## Options considered

| Option | Why not |
|---|---|
| **Wrap cells (chosen)** | Tables get taller, but everything is visible without new keys. |
| Raise or drop the 40-column cap only | Cells are still cut when the pane is narrower than the table. |
| Scroll the preview sideways | The preview has no sideways scrolling; it would need new keys, and you'd still scroll to read one cell. |
| Show the whole cell under the cursor in a status line | The preview has no cell cursor. |

## Steps

1. `view.rs`: a `wrap(text, width)` that returns each line's byte range in the cell, breaking at spaces and at
   `\n` and splitting words that are too long.
2. `view.rs`: in table cells, turn `<br>` into `\n`. On `End(Table)`, size the columns without the cap, wrap
   each cell, draw each row as tall as its tallest cell, add rules between rows if any row wraps, and place
   each link on every line it covers.
3. Keep `table()` (CSV and spreadsheets) as it is. Share the shrinking loop with it.
4. Tests: the old table tests still pass. New ones cover a wrapped row, the row rules, `<br>`, and a link
   that wraps.
