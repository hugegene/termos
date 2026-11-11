# Finding: saving xlsx edits by changing only the edited cells (2026-11-11)

## Question

The [Office editing plan](../plans/2026-11-11-office-editing.md) saves an xlsx edit by
rewriting only the edited cells inside the file's zip and copying every other part unchanged. Four decisions depend
on facts checked here:

1. **Can the other parts be copied without recompressing them?** That keeps them byte for byte and makes saving fast.
   It depends on the `zip` crate we already have (9.5.1, brought in by calamine).
2. **Can a new text value be written as an inline string** (`<c t="inlineStr"><is><t>…</t></is></c>`), so the shared
   string table (`xl/sharedStrings.xml`) is left alone? It depends on calamine and LibreOffice reading inline strings.
3. **Do formulas that depend on an edited cell show the new result?** A formula cell keeps the value it last worked
   out (`<v>`). The plan has to decide between setting the workbook's `fullCalcOnLoad="1"` flag and removing those
   saved values.
4. **Does a cell keep its formatting** when only its value changes? This matters for dates in particular.

## Method

Dev machine: 12th Gen Intel Core i7-12650H, Ubuntu, Linux 6.8.0-149. LibreOffice 24.2.7.2, Python 3.12.3,
zip 9.5.1, calamine 0.38.0. Excel isn't available here, so nothing below was checked in Excel.

**1 and 2: the crates' source.**

```
R=~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f
grep -n "pub fn raw_copy_file" $R/zip-9.5.1/src/write.rs
grep -rn "inlineStr" $R/calamine-0.38.0/src/
grep -n "pub fn next_cell_with_formula\b" -B6 $R/calamine-0.38.0/src/xlsx/cells_reader.rs
```

**2, 3 and 4: patching a file and reading it back in LibreOffice.** The scripts are in
[scripts/2026-11-11-xlsx-patch/](scripts/2026-11-11-xlsx-patch/).

- `patch.py` copies `crates/finder-core/tests/data/sheet.xlsx` and changes four cells in `sheet1.xml`, each keeping
  its style attribute `s`. Every other zip entry is copied unchanged.

  | Cell | Before | After |
  |---|---|---|
  | A2 | `Apples` | inline string `Plums` |
  | C2 | 0.5 | 2 |
  | D3 | formula `B3*C3` | the plain value 99 |
  | E2 | date serial 46333 | 46334 (2026-11-08) |

  D2 holds the formula `B2*C2`, saved with its last value 1.5, so it should become 3 × 2 = 6.
- `--calc` also sets `fullCalcOnLoad="1"` on `<calcPr>` in `xl/workbook.xml`.
- `--drop-cached` also removes the `<v>` of every formula cell left in the sheet.
- `run.sh` makes the three variants and has LibreOffice convert each to CSV. It does this twice: with the default
  settings, and with a throwaway profile (`always-recalc.xcu`) that sets `OOXMLRecalcMode` to 0, "always
  recalculate".

```
docs/findings/scripts/2026-11-11-xlsx-patch/run.sh /path/to/empty/folder
# LibreOffice's built-in default for the setting:
python3 -I -c "s=open('/usr/lib/libreoffice/share/registry/main.xcd').read(); i=s.find('name=\"OOXMLRecalcMode\"'); print(s[i:i+400])"
grep -o 'OOXMLRecalcMode.\{0,120\}' ~/.config/libreoffice/4/user/registrymodifications.xcu   # your own override, if any
```

## Results

**Source checks:**

```
zip-9.5.1/src/write.rs:1388:    pub fn raw_copy_file<R: Read>(&mut self, file: ZipFile<'_, R>) -> ZipResult<()> {
calamine-0.38.0/src/xlsx/cells_reader.rs:725:        "v" if matches!(type_attr, Some("inlineStr") | Some("is")) => {
calamine-0.38.0/src/xlsx/cells_reader.rs:  /// Return the next cell record, exposing cached/literal value plus expanded
                                           /// per-cell formula text. ...
                                           pub fn next_cell_with_formula(&mut self) -> Result<Option<XlsxCellFormula<'a>>, XlsxError>
```

**What LibreOffice shows:**

```
== original
Item,Qty,Price,Total,Bought
Apples,3,0.5,1.5,2026-11-07
Pears,10,0.25,2.5,2026-01-02
== default/plain.csv            (cells only)
Plums,3,2,1.5,2026-11-08
Pears,10,0.25,99,2026-01-02
== default/calc.csv             (+ fullCalcOnLoad="1")
Plums,3,2,1.5,2026-11-08
Pears,10,0.25,99,2026-01-02
== default/dropped.csv          (+ formulas' saved values removed)
Plums,3,2,6,2026-11-08
Pears,10,0.25,99,2026-01-02
== always/plain.csv             (cells only, LibreOffice set to "always recalculate")
Plums,3,2,6,2026-11-08
Pears,10,0.25,99,2026-01-02
```

(The header row, the same in every file, is trimmed from the patched ones.)

**Which zip entries changed:**

```
plain.xlsx ['xl/worksheets/sheet1.xml'] of 11
calc.xlsx ['xl/workbook.xml', 'xl/worksheets/sheet1.xml'] of 11
```

**LibreOffice's setting:**

```
name="OOXMLRecalcMode" oor:type="xs:int" oor:nillable="false">
  <info>Specifies whether to force a hard recalc after load on OOXML-based Excel documents (2007 and newer).</info>
  <value>1</value>
```

1 means "never recalculate". Your own profile (`~/.config/libreoffice/4/user/registrymodifications.xcu`) doesn't
change it.

## Conclusions

1. **Copying parts unchanged works.** `zip` 9.5.1 has `raw_copy_file`, which copies an entry without recompressing it.
   Only the changed sheet (and `workbook.xml`, if the flag is set) differ from the original.
2. **Inline strings work.** LibreOffice shows `Plums`, and calamine reads `inlineStr` cells. `sharedStrings.xml`
   can stay as it is.
3. **The `fullCalcOnLoad` flag is not enough.** With LibreOffice's default settings, D2 still shows its old value 1.5
   with or without the flag. LibreOffice recalculated only when the formulas' saved values were removed (D2 = 6), or
   when its "always recalculate" setting was on. So the plan removes formulas' saved values when it saves.
   - **Cost:** until Excel or LibreOffice saves the file again, any program that reads saved values without
     calculating sees those formula cells as empty. That includes tos's own preview, calamine, and pandas with
     openpyxl.
   - **calamine 0.38 can read each cell's value and formula in one pass** (`next_cell_with_formula`). tos can use it
     to show such a cell as its formula (`=B2*C2`) instead of blank, and to know which cells hold formulas.
4. **Formatting stays.** Changing only `<v>` and keeping `s` keeps the cell's format: E2 shows as the date
   2026-11-08. Replacing a formula with a value (D3) also opened without complaint in LibreOffice.

**Still unknown:**

- **Excel.**
  - Whether Excel recalculates on open, with the flag or with the saved values removed. Files written by openpyxl
    have no saved values and are known to show correctly in Excel, but that wasn't checked here.
  - Whether Excel complains about a stale `xl/calcChain.xml` after a formula is replaced (this test file has none).
    The plan avoids the question by not letting tos edit formula cells.
- **Speed** of `next_cell_with_formula` against `next_cell`, and of patching a 100,000-row sheet: step X1 of the plan
  measures both.
