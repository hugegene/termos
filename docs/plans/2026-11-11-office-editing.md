# Plan: edit xlsx, docx and pptx in tos

**Status:** planned, not built · **Date:** 2026-11-11 · **Related:**
[grid copy and open in app](2026-11-11-grid-copy-and-open-in-app.md) (its `o` key is where every refusal here points),
[video and Office previews](2026-11-07-video-and-office-previews.md) (the readers this builds on) · **Findings:**
[saving xlsx edits cell by cell](../findings/2026-11-11-xlsx-cell-patching.md),
[calamine: licences, dependencies and speed](../findings/2026-11-11-calamine.md)

## Decision

**One rule for all three formats: change only what you edited, inside the file's zip, and copy every other part
byte for byte.** Formatting, pictures, charts, comments and macros stay as they were. When tos can't make a change
safely, it refuses, says why, and points to `o` (open in the app, from the
[other plan](2026-11-11-grid-copy-and-open-in-app.md)).

| | Format | What you can edit | Where | Size (**estimate**) |
|---|---|---|---|---|
| **X** | `.xlsx`, `.xlsm` | cell values | the grid, as for CSV | 500–700 lines + tests |
| **W** | `.docx` | the text of paragraphs and table cells; add and delete paragraphs | Neovim, on a text version: one paragraph per line | 700–900 lines + tests |
| **P** | `.pptx` | the text in each slide's shapes and tables; add and delete paragraphs; bullet levels | the same text version, by slide and shape | 300–400 lines on top of W |

- **Order:** X first, because it's designed and checked ([finding](../findings/2026-11-11-xlsx-cell-patching.md)).
  Then W, then P, which reuses W's editor, diff and writer.
- **Not here,** so `o` opens the app for it:
  - formatting (bold, styles, fonts, colours)
  - adding or removing table rows, slides and pictures
  - xlsx rows and columns, and formulas
- **Enter afterwards,** following your rules from the [other plan](2026-11-11-grid-copy-and-open-in-app.md) (E):
  - **The rules:** Enter in the file list only focuses the preview; Enter in the preview edits in tos; `o` opens the
    app.
  - **On a spreadsheet,** Enter in the preview opens the grid, now editable for xlsx and xlsm.
  - **On a docx or pptx,** Enter in the preview opens the text version in Neovim. Until then, Enter there does
    nothing, and the preview's caption doesn't show it.
  - **Their preview captions then show Enter:** `Enter edit · o app · [ ] page · ←→ focus`.

### X. Edit cell values in .xlsx and .xlsm

**What changes for you:** a spreadsheet's grid uses the CSV grid's keys:

- `e` or `Enter` edits a cell, `u` undoes, `Alt+s` saves, and `q` with unsaved changes asks whether to save.
- The title says `● unsaved` instead of `read-only`.

These stay off, and each says why and points to `o`:

- **Adding or deleting rows and columns** (`a A d D`). Moving cells means rewriting every formula, merged range,
  chart range and named range that points past them.
- **Editing a formula cell.** It says `D2 is a formula (=B2*C2): o opens it in your spreadsheet app`.
- **`[` and `]` with unsaved changes.** It says `unsaved changes: Alt+s saves them first`.

`.xlsb`, `.xls` and `.ods` stay read-only. `.xlsb` and `.xls` are binary formats. `.ods` could be done the same way
later, with its own XML.

**How saving works.** Say you change C2 (Price) in `budget.xlsx` from 0.5 to 2. D2 holds `=B2*C2`:

```
budget.xlsx (a zip)                       saved as
  [Content_Types].xml, docProps/*    ──►  copied byte for byte, not recompressed (zip's raw_copy_file)
  xl/styles.xml, xl/theme/*          ──►  copied byte for byte
  xl/sharedStrings.xml               ──►  copied byte for byte (new text goes in the cell itself)
  xl/charts/*, xl/media/*            ──►  copied byte for byte
  xl/vbaProject.bin (.xlsm macros)   ──►  copied byte for byte
  xl/workbook.xml                    ──►  one attribute added: <calcPr fullCalcOnLoad="1" …>
  xl/worksheets/sheet1.xml           ──►  rewritten as a stream:
                                            <c r="C2" s="0" t="n"><v>0.5</v></c>  →  <c r="C2" s="0" t="n"><v>2</v></c>
                                            <c r="D2" …><f>B2*C2</f><v>1.5</v></c> →  <c r="D2" …><f>B2*C2</f></c>
  xl/worksheets/sheet2.xml           ──►  only if it has formulas: their saved values removed the same way
```

Step by step:

1. **Check it's safe to write** (shared with W and P).
   - **Open elsewhere:** a lock file next to it (`~$budget.xlsx` from Excel or Word, `.~lock.budget.xlsx#` from
     LibreOffice) means the file is open in an app. Saving says
     `budget.xlsx is open in another app; close it there first`.
   - **Changed since:** if the file changed on disk after tos read it, saving says `budget.xlsx changed on disk;
     close and open it again`.
2. **Find the sheet's part.** The sheet's name leads to its `r:id` in `xl/workbook.xml`, and that leads to its path
   in `xl/_rels/workbook.xml.rels`, here `xl/worksheets/sheet1.xml`. Also read `date1904` from `workbook.xml`, for
   dates.
3. **Rewrite that sheet as a stream** with quick-xml. Each changed cell keeps its `s` (its formatting) and gets a new
   value; nothing else in the sheet changes.
   - **A cell that wasn't in the file** (an empty one you typed into) is inserted in column order, in a new
     `<row>` if needed. It takes the row's style if the row has one (`customFormat="1"`), otherwise its column's
     (`<col style>`). `<dimension ref>` grows to cover it.
   - **Cells without `r` attributes** (allowed, and written by some programs) are counted by position.
4. **Remove formulas' saved values** (`<v>` after `<f>`) in every sheet that has formulas. You chose this on
   2026-11-11.
   - **Why:** with its default settings, LibreOffice keeps showing a formula's old value even with `fullCalcOnLoad`.
     It only works out D2 = 6 once the old value is gone ([finding](../findings/2026-11-11-xlsx-cell-patching.md)).
   - **Every sheet,** because a formula on another sheet can point at C2.
5. **Set `fullCalcOnLoad="1"`** in `workbook.xml`, so Excel recalculates everything on open (not checked here; no
   Excel on this machine).
6. **Write to `.budget.xlsx.tos-save` next to it**, with the file's permissions, then rename it over the original, as
   the CSV grid does.

**What you type and what's saved:**

| You type | Saved as | Why |
|---|---|---|
| `2`, `-3.5`, `1e6` | a number | plain decimal or exponent form |
| `007`, `1,000`, `$5`, `inf` | text | a leading zero, separators or symbols aren't parsed |
| `2026-11-08` in a cell that showed a date | a date (serial 46334, or 1904-based) | the cell's style keeps its date format |
| `2026-11-08` in any other cell | text | there's no date format to show it with |
| `TRUE` / `FALSE` in a cell that showed one | true / false | |
| `Plums` | text, inline in the cell (`t="inlineStr"`) | `sharedStrings.xml` stays as it is |
| nothing | the value is removed; the cell and its formatting stay | |

**Formulas in tos after saving:** their saved values are gone until Excel or LibreOffice saves the file again.
- **What tos shows instead:** the reader reads each cell's value and formula in one pass (calamine's
  `next_cell_with_formula`). A formula without a value shows as its formula, `=B2*C2`, instead of blank.
- **Refusing formula edits:** the same pass tells the grid which cells are formulas.
- **Other programs** that read saved values without calculating (pandas with openpyxl, for example) see those cells
  as empty until an app saves the file.

### W. docx: edit the text in Neovim

**What you see.** Enter in `office.docx`'s preview (the test file) writes this text version and opens it in Neovim in the
preview pane, as for a `.md` file:

```
# Trip report
We went to Lisbon in November, and it was 3 * warmer.
## What we saw
- The castle
- Trams
  - Line 28
1. First day
1. Second day
| Item | Cost |
| Hotel | 420 |
| Food \| drink | 180 |
The end.
```

**How to read it:**

- **One line per paragraph.** A long paragraph wraps on screen (tos's Neovim config already sets `wrap`) but stays
  one line.
- **The marker at the start shows the paragraph's kind:**
  - `#` to `######`: a heading, from its style
  - `- ` and `1. `: a list item, indented two spaces per level
  - `| … |`: a table row. Cells are split by `|`; a `|` in the text is written `\|`.
- **The text is plain.** There are no bold or italic markers, and formatting stays where it was (see below).
- **Things that aren't text show as tokens, which must stay where they are:**
  - `[image]`: a picture, as in the preview
  - `[field]`: a page number, a date or another field
  - `[note 3]`: a footnote reference
  - `⏎`: a line break inside the paragraph
- **Not shown, and left as they are,** as in the preview: headers, footers, footnotes' text, comments and text
  boxes.

**Example.** You change `Lisbon` to `Porto`, add `  - Line 15` under `  - Line 28`, and press `Alt+s`:

```
┌──────────────────────────────── tos server ─────────────────────────────────────┐
│ finder                                                                          │
│  1. Enter in office.docx's preview: writes the text version to                  │
│     ~/.local/state/termos/edit/<id>/office.docx.txt  and keeps a copy in memory │
│  4. sees the file's new modification time (checked every 0.5 s while open)      │
│  5. diffs the lines, patches word/document.xml, writes office.docx              │
│  6. status bar: "saved office.docx: 1 paragraph changed, 1 added"               │
└────────────▲───────────────────────────────────────┬────────────────────────────┘
             │ 3. Alt+s writes the text file          │ 2. opens Neovim on it, in the preview pane
             │                                        ▼
       ┌─────┴──────────────────────────────────────────────┐
       │ Neovim (a terminal pane the server runs)           │
       └────────────────────────────────────────────────────┘
```

**Step 5 in detail:**

1. **Diff.** tos compares your text with the version it wrote, line by line: matching lines at the start and end
   are set aside, and the middle is compared. Here line 2 changed, and a line was added after line 6.
2. **The changed line keeps its formatting outside the edit.** In the file, line 2 is five runs:

   ```
   "We went to " | "Lisbon" (bold) | " in " | "November" (italic) | ", and it was 3 * warmer."
   ```

   The old and new text share the start `We went to ` and the end ` in November, and it was 3 * warmer.`, so only
   the middle, `Lisbon` → `Porto`, changes. It sits inside the bold run, so only that run's text changes:

   ```
   <w:r><w:rPr><w:b/></w:rPr><w:t>Lisbon</w:t></w:r>  →  <w:r><w:rPr><w:b/></w:rPr><w:t>Porto</w:t></w:r>
   ```

   Porto is still bold. When a change spans several runs, the new text goes into the first of them, with that run's
   formatting, and the replaced text is removed from the others.
3. **The added line** becomes a new `<w:p>` after `Line 28`. It copies that paragraph's properties (its style, list
   and level) and its first run's formatting.
   - Its marker (`  - `) must match the paragraph above or, failing that, the one below; it copies the one it matches.
4. **A deleted line** removes its `<w:p>`.
5. **Writing the file:** `word/document.xml` is rewritten as a stream, and the other parts are copied byte for byte.
   The lock-file and changed-on-disk checks, the temporary file and the rename are the same as for xlsx (X step 1
   and 6).
6. **Afterwards,** the saved text becomes the version later saves compare with.

**Refused,** with the line number, and nothing is saved. Your text stays in Neovim, to fix and save again:

- **Changing a token,** or a change that crosses one (`[image]`, `[field]`, `[note 3]`, `⏎`). The text on either
  side can still change.
- **Paragraphs with tracked changes** (`w:ins`, `w:del`) or inside content controls (`w:sdt`).
- **Changing a marker,** except a heading's level (`#` → `##` sets the style Heading 2, if the document has it).
- **Changing the shape of a table:** adding or removing a `| … |` row, or a row's number of cells.
- **Deleting a paragraph the document needs:** the one holding the section's page settings (`w:sectPr`), or a table
  cell's only paragraph.
- **A new line whose marker matches neither neighbour.**

**Closing Neovim** deletes the text file, unless the last save was refused. In that case the status bar says where
the file is, so your text isn't lost.

### P. pptx: the same, by slide and shape

**What you see.** Slide 2 of `deck.pptx` (the test file) has two shapes, named `PlaceHolder 1` and
`PlaceHolder 2` in the file:

```
## Slide 2
### PlaceHolder 1
Results
### PlaceHolder 2
- Revenue up
  - Mostly in Europe
- Costs flat
```

- **Fixed lines:** `## Slide N` and `### <shape name>` mark where each line belongs. Changing, adding or removing them
  is refused.
- **Under a shape,** each line is one of its paragraphs, with W's rules. Runs are `<a:r>`, with text in `<a:t>`.
- **Bullet levels:** indenting a line changes its level. It copies the paragraph properties (`<a:pPr>`, which hold
  the indent and the bullet) of a paragraph at that level in the same shape. If there's none, the change is refused.
  Setting `lvl` alone would leave the indent and bullet of the old level.
- **A shape needs one paragraph,** so deleting all of a shape's lines leaves one empty paragraph.
- **Tables** (`a:tbl`) are `| … |` rows, as in docx.
- **Speaker notes:** later, as in the preview.

**Example:** change `Costs flat` to `Costs down 3%`, and only `<a:t>Costs flat</a:t>` in `ppt/slides/slide2.xml`
changes.

## Options considered

| Topic | Option | Verdict |
|---|---|---|
| **xlsx:** saving | **Change only the edited cells in the zip** | ✅ Everything else stays byte for byte ([finding](../findings/2026-11-11-xlsx-cell-patching.md)); uses the `zip` and `quick-xml` we have |
| | `umya-spreadsheet` | ❌ Rebuilds the whole file from its own model, so what it doesn't model can be lost (not checked); a large dependency |
| | `rust_xlsxwriter` | ❌ Writes new files only; loses formatting, charts and macros |
| | Recalculate with `soffice --convert-to xlsx` | ❌ LibreOffice rewrites the whole file; seconds to start |
| **xlsx:** new text | **Inline string in the cell** | ✅ LibreOffice and calamine read it (finding); `sharedStrings.xml` untouched |
| | Append to `sharedStrings.xml` | ❌ Rewrites one more part, and its count attributes, for no gain |
| **xlsx:** formulas after a save | **Remove their saved values, and set `fullCalcOnLoad`** | ✅ Your choice. LibreOffice and (**not checked**) Excel show the right results; tos shows `=B2*C2` |
| | Only set `fullCalcOnLoad` | ❌ LibreOffice by default shows the old results, silently wrong (finding) |
| | Only remove the values of formulas that depend on edited cells | ❌ Needs a formula parser that follows ranges, other sheets and names |
| **xlsx:** formula cells | **Not editable in tos** | ✅ Avoids `calcChain.xml` and shared formulas, where a stale entry can make Excel repair the file |
| | Editable; the value replaces the formula | Later, if wanted; needs the shared-formula and calc-chain handling |
| **docx, pptx:** how you edit | **Neovim, on a text version with one paragraph per line** | ✅ A real editor you know; tos already runs Neovim in the preview pane |
| | tos's own paragraph editor, like the grid's cell editor | ❌ For now: the grid's editor is one line with no wrapping, and paragraphs are long |
| | Markdown with `**bold**` and `*italic*` | ❌ For now: editing a marker would mean changing formatting. Later, if wanted |
| **docx, pptx:** writing back | **Change only the edited runs and paragraphs in the zip** | ✅ The same rule as xlsx; formatting outside the edit stays |
| | pandoc: docx → Markdown → docx, with the original as `--reference-doc` | ❌ Rebuilds the document: loses comments, tracked changes, fields, text boxes and direct formatting. Not installed here (`which pandoc` finds nothing) |
| | `docx-rs` and similar crates | ❌ They rebuild the file from their own model; none covers pptx |
| **docx, pptx:** matching lines to paragraphs | **A line diff against the text tos wrote** | ✅ No ids cluttering the text |
| | An id on each line (`¶12 We went to…`) | ❌ Clutter, and easy to break by editing |
| **docx, pptx:** the diff | **Our own: set aside matching start and end, then a longest-common-subsequence on the middle** | ✅ About 100 lines (**estimate**); edits are local, so the middle is small |
| | The `similar` crate | ❌ A new dependency for about 100 lines |
| **docx, pptx:** noticing a save | **Check the text file's modification time every 0.5 s while Neovim is open** | ✅ `Alt+s` saves the document, as you'd expect; little code |
| | Apply when Neovim closes | ❌ `Alt+s` wouldn't really save |
| | Neovim runs a `tos` command after each write | Later, if checking the time costs anything; needs a new CLI command |
| **docx, pptx:** a save with a refused change | **Refuse the whole save, and say which line** | ✅ The file is never half-changed |
| | Save the rest, skip the refused lines | ❌ The document then differs from your text in ways that are easy to miss |

## Steps

### xlsx

- [ ] **X1. Measure** (new finding):
  - the speed of `next_cell_with_formula` against `next_cell`, on the 100,000-row file from the
    [calamine finding](../findings/2026-11-11-calamine.md)'s scripts. If it's much slower, previews keep `next_cell`
    and only the grid uses it.
  - saving one changed cell in that file.
- [ ] **X2. Reader** (`finder-core/src/sheet.rs`): read values and formulas in one pass for xlsx and xlsm. `Sheet`
  gains the positions of formula cells. A formula with no saved value shows as `=…`.
- [ ] **X3. Writer** (`finder-core`, new `xlsx_write.rs`):
  `save(path, sheet_name, changes: &[(row, col, text)], opened_at: SystemTime)` does steps 1–6 above. It also holds
  the typing rules. The lock-file check, the changed-on-disk check, raw copying and the temporary-file rename go in
  a shared `zip_write.rs` for W and P.
- [ ] **X4. Grid:** `Grid::from_sheet` makes xlsx and xlsm grids editable. It keeps the cells as read, so saving
  passes only the cells that differ, and keeps the formula positions to refuse editing them. `a A d D` and `[ ]`
  with unsaved changes say why not. The title and hints change. Other formats stay as they are.
- [ ] **X5. Tests:**
  - **Unit:**
    - **Each typing rule:** number, date (1900 and 1904), boolean, text, empty.
    - **Changing one cell:** every other zip entry stays byte-identical, and calamine reads the new value back.
    - **Other cell edits:** inserting into a missing row and a missing cell, with `<dimension>` updated; cells
      without `r`.
    - **Formulas:** their saved values are removed in every sheet.
    - **Refusals:** a lock file, the file changed on disk, editing a formula cell.
  - **End to end:** open `sheet.xlsx`'s grid, change a cell, `Alt+s`, close and reopen, and see the new value and
    `=B2*C2` where D2 was.
  - Test files are copied to a temporary folder first, so `tests/data` isn't changed.
- [ ] **X6. Docs:** README (the grid's keys, and what xlsx editing keeps), the finder and previews table in the
  rebuild plan, and the module comments in `grid.rs` and `sheet.rs`.

### docx

- [ ] **W1. Text version** (`finder-core`, new `docx_edit.rs`): from `word/document.xml`, the lines, and for each
  line its paragraph and its runs' text offsets. It reuses `office.rs`'s XML walk, heading styles and list kinds.
- [ ] **W2. Diff** (`finder-core`, new `line_diff.rs`): lines in, changed / added / deleted out.
- [ ] **W3. Writer:** applies the diff to `word/document.xml` as a stream.
  - the smallest changed span in a line's runs
  - `xml:space="preserve"` where text starts or ends with a space
  - new paragraphs and deletions
  - every refusal listed above
  - then `zip_write.rs` (X3) for the checks, copying and rename
- [ ] **W4. Editor** (`tos`):
  - Enter in a docx's preview writes the text version to `~/.local/state/termos/edit/<id>/NAME.docx.txt` and opens Neovim on
    it (`Action::OpenEditor`).
  - While it's open, the server checks the file's modification time every 0.5 s and applies changes.
  - Status bar messages.
  - On close: delete the file, unless the last save was refused.
  - Captions: the preview of a docx (and a pptx after P) shows `Enter edit · o app · [ ] page · ←→ focus`.
- [ ] **W5. Tests:**
  - **Unit:**
    - the text version of `office.docx`
    - the example: Porto stays bold, and the new list item copies `Line 28`
    - a change across runs
    - each refusal
    - every zip entry other than `word/document.xml` stays byte-identical
    - the Markdown preview of the saved file shows the change
  - **End to end:** Enter in the preview of a copy of `office.docx`, type in Neovim, `Alt+s`, and see the status bar message and the
    preview change.
- [ ] **W6. Docs:** README, and the module comment in `office.rs`.

### pptx

- [ ] **P1. Text version:** slides in presentation order, `## Slide N` and `### <shape name>`, paragraphs indented by
  level, tables.
- [ ] **P2. Writer:** per slide part (`ppt/slides/slideN.xml`), with W's diff and run handling. Fixed lines are
  checked, and level changes copy a paragraph's properties.
- [ ] **P3. Tests:** like W5, with `deck.pptx`: `Costs flat` → `Costs down 3%`; a level change; a refused change to
  a `###` line.
- [ ] **P4. Docs:** README.

### Last

- [ ] **H. Check by hand (you):** edit your own xlsx, docx and pptx files, save, and open them in LibreOffice (and
  Excel, Word or PowerPoint if you have them): no repair prompt, formatting kept, totals updated.

Build and test as CLAUDE.md says: `cargo build --offline`, `cargo test --workspace`, clippy,
`cargo fmt -p tos -p finder-core`, `cargo deny check licenses`. No CLI command changes, so `docs/cli/` doesn't change.

## Open questions

1. **Text only for docx and pptx at first (recommended)?** That means no bold or italic changes, no style changes
   other than heading levels, and no new tables, slides or pictures. `o` covers the rest.

## Change log

- **2026-11-11:** written. Option A for xlsx moved here from
  [grid copy and open in app](2026-11-11-grid-copy-and-open-in-app.md), with your answer to its open question:
  formulas' saved values are removed on save. You asked for docx and pptx editing in the same plan.
- **2026-11-11 (later):** your rule for Enter answered the second open question.
  - **The rule:** Enter only edits in tos, so on docx and pptx it opens the text version. `o` (lowercase, not `O`)
    opens the app.
  - **Changed here:** the key in every refusal message and caption.
- **2026-11-11 (night):** Enter in the file list now only focuses the preview (other plan, E), so editing here starts
  with Enter in the preview.
