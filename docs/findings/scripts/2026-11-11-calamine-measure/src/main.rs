//! Times how long calamine takes to give the finder's preview what it needs: the sheet names, and the first
//! sheet's first 2000 rows as text. Each case is run 9 times; the median, minimum and maximum are printed.
//!
//! - `names`: open the workbook and list its sheets.
//! - `cells`: xlsx only. Open, then read the first sheet cell by cell with `worksheet_cells_reader`, stopping
//!   at row 2000.
//! - `range`: open, then load the whole first sheet with `worksheet_range`, and take its first 2000 rows.

use std::path::Path;
use std::time::{Duration, Instant};

use calamine::{Data, DataRef, Reader, Xlsx, open_workbook, open_workbook_auto};

const ROWS: u32 = 2000;
const RUNS: usize = 9;

fn time(what: &str, path: &Path, f: impl Fn() -> usize) {
    let mut times: Vec<Duration> = Vec::new();
    let mut cells = 0;
    for _ in 0..RUNS {
        let start = Instant::now();
        cells = f();
        times.push(start.elapsed());
    }
    times.sort();
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "{:<12} {:<6} median {:>8.1} ms  min {:>8.1}  max {:>8.1}  cells read {cells}",
        path.file_name().unwrap().to_string_lossy(),
        what,
        ms(times[RUNS / 2]),
        ms(times[0]),
        ms(times[RUNS - 1]),
    );
}

fn text(d: &DataRef) -> String {
    match d {
        DataRef::DateTime(dt) => {
            let (y, m, day, ..) = dt.to_ymd_hms_milli();
            format!("{y:04}-{m:02}-{day:02}")
        }
        DataRef::Int(n) => n.to_string(),
        DataRef::Float(f) => f.to_string(),
        DataRef::String(s) | DataRef::DateTimeIso(s) | DataRef::DurationIso(s) => s.clone(),
        DataRef::SharedString(s) => (*s).to_owned(),
        DataRef::Bool(b) => b.to_string(),
        DataRef::Error(e) => e.to_string(),
        DataRef::Empty => String::new(),
    }
}

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        time("names", path, || open_workbook_auto(path).unwrap().sheet_names().len());
        if path.extension().is_some_and(|e| e == "xlsx") {
            time("cells", path, || {
                let mut wb: Xlsx<_> = open_workbook(path).unwrap();
                let first = wb.sheet_names()[0].clone();
                let mut reader = wb.worksheet_cells_reader(&first).unwrap();
                let mut rows: Vec<Vec<String>> = Vec::new();
                let mut n = 0;
                while let Some(cell) = reader.next_cell().unwrap() {
                    let (r, c) = cell.get_position();
                    if r >= ROWS {
                        break;
                    }
                    let (r, c) = (r as usize, c as usize);
                    if rows.len() <= r {
                        rows.resize(r + 1, Vec::new());
                    }
                    if rows[r].len() <= c {
                        rows[r].resize(c + 1, String::new());
                    }
                    rows[r][c] = text(cell.get_value());
                    n += 1;
                }
                n
            });
        }
        time("range", path, || {
            let mut wb = open_workbook_auto(path).unwrap();
            let first = wb.sheet_names()[0].clone();
            let range = wb.worksheet_range(&first).unwrap();
            let rows: Vec<Vec<String>> = range
                .rows()
                .take(ROWS as usize)
                .map(|r| r.iter().map(|d: &Data| d.to_string()).collect())
                .collect();
            rows.iter().map(Vec::len).sum()
        });
    }
}
