//! Spreadsheets (`xlsx`, `xlsm`, `xlsb`, `xls`, `ods`), read with calamine: the sheet names, and one sheet's
//! rows as text. Values are shown as they're stored, without Excel's number formats; formulas show the value
//! last calculated, which the file keeps; dates show as `2026-11-07`.
//!
//! xlsx and xlsm are read cell by cell and stop at the row limit: 21 ms for the first 2000 rows of a
//! 100,000-row sheet, against 804 ms to load it whole (docs/findings/2026-11-11-calamine.md). The other formats
//! have no such reader in calamine, so their sheets are loaded whole.

use std::path::Path;

use calamine::{Data, DataRef, ExcelDateTime, Reader, Sheets, Xlsx, open_workbook, open_workbook_auto};

use crate::office;

/// The file extensions read as spreadsheets.
pub const EXTENSIONS: &[&str] = &["xlsx", "xlsm", "xlsb", "xls", "ods"];

/// One sheet of a workbook, as text.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    /// Every sheet's name, in order.
    pub names: Vec<String>,
    /// Which of them this is.
    pub current: usize,
    /// Its rows from row 1, each from column A. Cells keep their place: an empty row or column stays empty.
    pub rows: Vec<Vec<String>>,
    /// The sheet has more rows than were read.
    pub truncated: bool,
}

/// Reads sheet `index` (0-based; past the last means the last) of `path`, up to `max_rows` rows.
pub fn read(path: &Path, index: usize, max_rows: usize) -> Result<Sheet, String> {
    office::check_locked(path, "spreadsheet")?;
    let xlsx = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xlsx") || e.eq_ignore_ascii_case("xlsm"));
    if xlsx {
        let mut book: Xlsx<_> = open_workbook(path).map_err(|e: calamine::XlsxError| e.to_string())?;
        let names = book.sheet_names();
        let current = pick(&names, index)?;
        let mut cells = book.worksheet_cells_reader(&names[current]).map_err(|e| e.to_string())?;
        let mut grid = Grid::new(max_rows);
        while let Some(cell) = cells.next_cell().map_err(|e| e.to_string())? {
            let (row, col) = cell.get_position();
            if !grid.put(row, col, text_ref(cell.get_value())) {
                break;
            }
        }
        return Ok(grid.into_sheet(names, current));
    }
    let mut book: Sheets<_> = open_workbook_auto(path).map_err(|e| e.to_string())?;
    let names = book.sheet_names();
    let current = pick(&names, index)?;
    let range = book.worksheet_range(&names[current]).map_err(|e| e.to_string())?;
    let mut grid = Grid::new(max_rows);
    if let Some((top, left)) = range.start() {
        'rows: for (r, row) in range.rows().enumerate() {
            for (c, value) in row.iter().enumerate() {
                if !grid.put(top + r as u32, left + c as u32, text(value)) {
                    break 'rows;
                }
            }
        }
    }
    Ok(grid.into_sheet(names, current))
}

fn pick(names: &[String], index: usize) -> Result<usize, String> {
    if names.is_empty() {
        return Err("no sheets in this file".into());
    }
    Ok(index.min(names.len() - 1))
}

/// Rows filled cell by cell, up to a row limit.
struct Grid {
    rows: Vec<Vec<String>>,
    max_rows: usize,
    truncated: bool,
}

impl Grid {
    fn new(max_rows: usize) -> Grid {
        Grid { rows: Vec::new(), max_rows, truncated: false }
    }

    /// Puts a cell in its place. Returns false at the row limit: the cell is left out and reading can stop.
    fn put(&mut self, row: u32, col: u32, text: String) -> bool {
        let (row, col) = (row as usize, col as usize);
        if row >= self.max_rows {
            self.truncated = true;
            return false;
        }
        if text.is_empty() {
            return true;
        }
        if self.rows.len() <= row {
            self.rows.resize(row + 1, Vec::new());
        }
        let cells = &mut self.rows[row];
        if cells.len() <= col {
            cells.resize(col + 1, String::new());
        }
        cells[col] = text;
        true
    }

    fn into_sheet(self, names: Vec<String>, current: usize) -> Sheet {
        Sheet { names, current, rows: self.rows, truncated: self.truncated }
    }
}

/// A cell's text, from the cell reader.
fn text_ref(value: &DataRef) -> String {
    match value {
        DataRef::SharedString(s) => (*s).to_owned(),
        DataRef::String(s) => s.clone(),
        other => text(&Data::from(other.clone())),
    }
}

/// A cell's text.
pub fn text(value: &Data) -> String {
    match value {
        Data::Int(n) => n.to_string(),
        Data::Float(f) => number(*f),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Data::DateTime(dt) => date(dt),
        Data::Error(e) => e.to_string(),
        Data::Empty => String::new(),
    }
}

/// A number as Excel shows it in a general cell: at most 15 significant digits, so 0.1 + 0.2 is 0.3.
fn number(f: f64) -> String {
    if !f.is_finite() || f == 0.0 {
        return if f == 0.0 { "0".into() } else { f.to_string() };
    }
    let magnitude = f.abs().log10().floor() as i32;
    if !(-5..15).contains(&magnitude) {
        return f.to_string();
    }
    let decimals = (14 - magnitude).max(0) as usize;
    let s = format!("{f:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" { "0".into() } else { s.to_owned() }
}

/// A date as `2026-11-07`, with the time if it has one; a time alone as `14:30:00`; a duration as `26:15:00`.
fn date(dt: &ExcelDateTime) -> String {
    let value = dt.as_f64();
    if dt.is_duration() {
        let seconds = (value * 86_400.0).round() as i64;
        let sign = if seconds < 0 { "-" } else { "" };
        let s = seconds.abs();
        return format!("{sign}{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60);
    }
    let (y, m, d, h, min, s, _) = dt.to_ymd_hms_milli();
    let time = (h, min, s) != (0, 0, 0);
    if (0.0..1.0).contains(&value) && time {
        return format!("{h:02}:{min:02}:{s:02}");
    }
    if time { format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}") } else { format!("{y:04}-{m:02}-{d:02}") }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{CellErrorType, ExcelDateTimeType};
    use std::path::PathBuf;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
    }

    #[test]
    fn numbers_as_excel_shows_them() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(0.1 + 0.2), "0.3");
        assert_eq!(number(-1234.5), "-1234.5");
        assert_eq!(number(0.000123), "0.000123");
        assert_eq!(number(1e20), "100000000000000000000");
        assert_eq!(number(-0.0), "0");
    }

    #[test]
    fn cells_as_text() {
        let day = |v: f64| Data::DateTime(ExcelDateTime::new(v, ExcelDateTimeType::DateTime, false));
        assert_eq!(text(&day(46333.0)), "2026-11-07");
        assert_eq!(text(&day(46333.75)), "2026-11-07 18:00:00");
        assert_eq!(text(&day(0.5)), "12:00:00");
        let span = Data::DateTime(ExcelDateTime::new(1.09375, ExcelDateTimeType::TimeDelta, false));
        assert_eq!(text(&span), "26:15:00");
        assert_eq!(text(&Data::Bool(true)), "TRUE");
        assert_eq!(text(&Data::Error(CellErrorType::Div0)), "#DIV/0!");
        assert_eq!(text(&Data::Int(-4)), "-4");
    }

    /// Budget, as make_office.py writes it in each format.
    fn budget() -> Vec<Vec<String>> {
        [
            ["Item", "Qty", "Price", "Total", "Bought"],
            ["Apples", "3", "0.5", "1.5", "2026-11-07"],
            ["Pears", "10", "0.25", "2.5", "2026-01-02"],
        ]
        .iter()
        .map(|r| r.iter().map(|s| (*s).to_owned()).collect())
        .collect()
    }

    #[test]
    fn reads_each_format() {
        for name in ["sheet.xlsx", "sheet.ods", "sheet.xls"] {
            let sheet = read(&data(name), 0, 2000).unwrap();
            assert_eq!(sheet.names, ["Budget", "Notes"], "{name}");
            assert_eq!(sheet.current, 0);
            // Formulas show their value; dates show as dates.
            assert_eq!(sheet.rows, budget(), "{name}");
            assert!(!sheet.truncated);

            let notes = read(&data(name), 1, 2000).unwrap();
            assert_eq!((notes.current, notes.rows.clone()), (1, vec![vec!["Second sheet".to_owned()]]), "{name}");
            // Past the last sheet is the last.
            assert_eq!(read(&data(name), 9, 2000).unwrap().current, 1);
        }
    }

    #[test]
    fn stops_at_the_row_limit() {
        for name in ["sheet.xlsx", "sheet.ods"] {
            let sheet = read(&data(name), 0, 2).unwrap();
            assert_eq!(sheet.rows, budget()[..2], "{name}");
            assert!(sheet.truncated, "{name}");
        }
    }

    #[test]
    fn locked_and_broken_files() {
        assert_eq!(read(&data("locked.xlsx"), 0, 10), Err("password-protected spreadsheet".into()));
        let dir = tempfile::tempdir().unwrap();
        let cut = dir.path().join("cut.xlsx");
        let bytes = std::fs::read(data("sheet.xlsx")).unwrap();
        std::fs::write(&cut, &bytes[..bytes.len() * 6 / 10]).unwrap();
        assert!(read(&cut, 0, 10).is_err());
    }
}
