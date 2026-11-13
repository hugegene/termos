//! The CSV grid editor (step 5), opened from the finder with Enter on a `.csv` or `.tsv` file. It replaces
//! the prototype's VisiData. Saving keeps the file's delimiter, line endings and ragged rows.
//!
//! Spreadsheets (xlsx, ods and the like) open in it read-only, a sheet at a time: `tos` can't write them back
//! without losing their formatting (docs/plans/2026-11-07-video-and-office-previews.md).
//!
//! A range of cells can be selected (`v`, `V`, or a mouse drag) and copied with `y`, as tab-separated text that
//! spreadsheet apps paste as cells (docs/plans/2026-11-11-grid-copy-and-open-in-app.md).

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use super::super::render::{fill, put_str};

const MAX_BYTES: u64 = 20 << 20;
pub const MAX_ROWS: usize = 100_000;
const MAX_WIDTH: usize = 24;
const MAX_UNDO: usize = 100;
/// The most text `y` copies. Terminals limit what OSC 52 carries in different ways; this is an **estimate**.
const MAX_COPY: usize = 1 << 20;

/// One undoable change.
enum Change {
    /// A cell's old text; `None` if the row was shorter and the cell didn't exist.
    Cell {
        row: usize,
        col: usize,
        old: Option<String>,
    },
    InsertRow(usize),
    DeleteRow(usize, Vec<String>),
    InsertCol(usize),
    /// A deleted column's cells, per row (`None` where a short row didn't have it).
    DeleteCol(usize, Vec<Option<String>>),
}

/// Ctrl+s (or Alt+s, which on a Mac needs the terminal's Option-as-Alt setting) saves.
/// Plan: docs/plans/2026-11-13-editor-save-keys.md.
fn is_save(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('s') && key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Ctrl+e (or Alt+q) closes; so does a plain q outside a cell being edited.
fn is_close(key: &KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('e') => key.modifiers.contains(KeyModifiers::CONTROL),
        KeyCode::Char('q') => key.modifiers.contains(KeyModifiers::ALT),
        _ => false,
    }
}

/// What the finder should do after a key in the grid.
pub enum Outcome {
    Stay,
    Close,
    /// Show this sheet of the spreadsheet instead (0-based).
    Sheet(usize),
    /// Put this text on the clipboard; `what` says how many cells, for the status bar.
    Copy {
        text: String,
        what: String,
    },
    /// Open the file in its default app.
    OpenApp(PathBuf),
}

/// The spreadsheet a read-only grid shows a sheet of.
pub struct Book {
    pub names: Vec<String>,
    pub current: usize,
}

struct Edit {
    text: String,
    /// Cursor position, in characters.
    cursor: usize,
}

pub struct Grid {
    pub path: PathBuf,
    delimiter: u8,
    crlf: bool,
    trailing_newline: bool,
    /// Row 0 is the header. Rows keep their own lengths.
    rows: Vec<Vec<String>>,
    widths: Vec<usize>,
    row: usize,
    col: usize,
    /// First data row shown under the header, and first column shown.
    top: usize,
    left: usize,
    edit: Option<Edit>,
    /// Where a selected range starts; it runs to the cursor. `whole_rows` selects every cell of those rows.
    anchor: Option<(usize, usize)>,
    whole_rows: bool,
    /// The cell a left button went down on, while it's held: a drag from there selects.
    press: Option<(usize, usize)>,
    undo: Vec<Change>,
    pub dirty: bool,
    confirm_close: bool,
    pub message: Option<String>,
    /// The body size at the last draw, for scrolling.
    view: (u16, u16),
    /// A spreadsheet, shown read-only; `None` for a CSV file, which can be edited.
    pub book: Option<Book>,
}

fn width_of(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// A cell as spreadsheet apps write it to the clipboard: in quotes, with `"` doubled, if it holds a tab, a newline
/// or a quote.
fn quote(cell: &str) -> Cow<'_, str> {
    if cell.contains(['\t', '\n', '\r', '"']) {
        Cow::Owned(format!("\"{}\"", cell.replace('"', "\"\"")))
    } else {
        Cow::Borrowed(cell)
    }
}

impl Grid {
    pub fn open(path: &Path, delimiter: u8) -> Result<Grid, String> {
        let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err(format!("too big to edit as a grid ({} MB); E opens it in your editor", size >> 20));
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let text = String::from_utf8(bytes).map_err(|_| "not UTF-8 text; E opens it in your editor".to_owned())?;
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(delimiter)
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut rows = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|e| format!("can't read it as CSV: {e}"))?;
            rows.push(record.iter().map(str::to_owned).collect::<Vec<_>>());
            if rows.len() > MAX_ROWS {
                return Err(format!("more than {MAX_ROWS} rows; E opens it in your editor"));
            }
        }
        if rows.is_empty() {
            rows.push(vec![String::new()]);
        }
        let mut grid = Grid {
            path: path.to_path_buf(),
            delimiter,
            crlf: text.contains("\r\n"),
            trailing_newline: text.is_empty() || text.ends_with('\n'),
            rows,
            widths: Vec::new(),
            row: 0,
            col: 0,
            top: 1,
            left: 0,
            edit: None,
            anchor: None,
            whole_rows: false,
            press: None,
            undo: Vec::new(),
            dirty: false,
            confirm_close: false,
            message: None,
            view: (80, 24),
            book: None,
        };
        grid.measure();
        Ok(grid)
    }

    /// A spreadsheet's sheet, read-only.
    pub fn from_sheet(path: &Path, sheet: finder_core::sheet::Sheet) -> Grid {
        let mut rows = sheet.rows;
        if rows.is_empty() {
            rows.push(vec![String::new()]);
        }
        let message = sheet.truncated.then(|| format!("only the first {MAX_ROWS} rows are shown"));
        let mut grid = Grid {
            path: path.to_path_buf(),
            delimiter: b',',
            crlf: false,
            trailing_newline: true,
            rows,
            widths: Vec::new(),
            row: 0,
            col: 0,
            top: 1,
            left: 0,
            edit: None,
            anchor: None,
            whole_rows: false,
            press: None,
            undo: Vec::new(),
            dirty: false,
            confirm_close: false,
            message,
            view: (80, 24),
            book: Some(Book { names: sheet.names, current: sheet.current }),
        };
        grid.measure();
        grid
    }

    /// Keeps the cursor and scroll position from another grid (the same file's previous sheet).
    pub fn keep_place(&mut self, other: &Grid) {
        self.view = other.view;
        self.move_to(other.row as isize, other.col as isize);
    }

    fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(1).max(1)
    }

    fn cell(&self, row: usize, col: usize) -> &str {
        self.rows.get(row).and_then(|r| r.get(col)).map_or("", String::as_str)
    }

    fn measure(&mut self) {
        let cols = self.columns();
        self.widths = vec![1; cols];
        for r in &self.rows {
            for (c, text) in r.iter().enumerate() {
                self.widths[c] = self.widths[c].max(width_of(text).min(MAX_WIDTH));
            }
        }
    }

    /// The file's text as it would be saved.
    pub fn to_text(&self) -> Result<Vec<u8>, String> {
        let terminator = if self.crlf { csv::Terminator::CRLF } else { csv::Terminator::Any(b'\n') };
        let mut writer = csv::WriterBuilder::new()
            .delimiter(self.delimiter)
            .flexible(true)
            .terminator(terminator)
            .quote_style(csv::QuoteStyle::Necessary)
            .from_writer(Vec::new());
        for r in &self.rows {
            writer.write_record(r).map_err(|e| e.to_string())?;
        }
        let mut data = writer.into_inner().map_err(|e| e.to_string())?;
        if !self.trailing_newline {
            let cut = if self.crlf { 2 } else { 1 };
            data.truncate(data.len().saturating_sub(cut));
        }
        Ok(data)
    }

    /// Writes the file under a temporary name next to it, with its permissions, then renames it.
    pub fn save(&mut self) -> Result<(), String> {
        let data = self.to_text()?;
        let name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = self.path.with_file_name(format!(".{name}.tos-save"));
        fs::write(&tmp, &data).map_err(|e| e.to_string())?;
        if let Ok(meta) = fs::metadata(&self.path) {
            let _ = fs::set_permissions(&tmp, meta.permissions());
        }
        fs::rename(&tmp, &self.path).map_err(|e| {
            let _ = fs::remove_file(&tmp);
            e.to_string()
        })?;
        self.dirty = false;
        Ok(())
    }

    // ----- changes -----

    fn record(&mut self, change: Change) {
        self.undo.push(change);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.dirty = true;
    }

    fn set_cell(&mut self, row: usize, col: usize, text: String) {
        let old = self.rows[row].get(col).cloned();
        if old.as_deref() == Some(text.as_str()) || (old.is_none() && text.is_empty()) {
            return;
        }
        let width = width_of(&text).min(MAX_WIDTH);
        let r = &mut self.rows[row];
        if r.len() <= col {
            r.resize(col + 1, String::new());
        }
        r[col] = text;
        let cols = self.columns();
        self.widths.resize(cols, 1);
        self.widths[col] = self.widths[col].max(width);
        self.record(Change::Cell { row, col, old });
    }

    fn insert_row(&mut self, at: usize) {
        self.rows.insert(at, vec![String::new(); self.columns()]);
        self.record(Change::InsertRow(at));
    }

    fn delete_row(&mut self, at: usize) {
        let old = self.rows.remove(at);
        self.record(Change::DeleteRow(at, old));
    }

    fn insert_col(&mut self, at: usize) {
        for r in &mut self.rows {
            if r.len() >= at {
                r.insert(at, String::new());
            }
        }
        self.widths.insert(at.min(self.widths.len()), 1);
        self.record(Change::InsertCol(at));
    }

    fn delete_col(&mut self, at: usize) {
        let old = self.rows.iter_mut().map(|r| (at < r.len()).then(|| r.remove(at))).collect();
        if at < self.widths.len() {
            self.widths.remove(at);
        }
        self.record(Change::DeleteCol(at, old));
    }

    fn undo(&mut self) {
        let Some(change) = self.undo.pop() else {
            self.message = Some("nothing to undo".into());
            return;
        };
        match change {
            Change::Cell { row, col, old } => {
                match old {
                    Some(text) => self.rows[row][col] = text,
                    None => self.rows[row].truncate(col),
                }
                (self.row, self.col) = (row, col);
            }
            Change::InsertRow(at) => {
                self.rows.remove(at);
                self.row = at.min(self.rows.len() - 1);
            }
            Change::DeleteRow(at, old) => {
                self.rows.insert(at, old);
                self.row = at;
            }
            Change::InsertCol(at) => {
                for r in &mut self.rows {
                    if at < r.len() {
                        r.remove(at);
                    }
                }
                self.col = at.saturating_sub(1);
            }
            Change::DeleteCol(at, old) => {
                for (r, cell) in self.rows.iter_mut().zip(old) {
                    if let Some(text) = cell {
                        r.insert(at.min(r.len()), text);
                    }
                }
                self.col = at;
            }
        }
        self.measure();
        self.dirty = true;
        self.clamp();
    }

    // ----- moving -----

    fn clamp(&mut self) {
        self.row = self.row.min(self.rows.len() - 1);
        self.col = self.col.min(self.columns() - 1);
        self.scroll_into_view();
    }

    /// Data rows that fit under the header and its rule.
    fn data_rows(&self) -> usize {
        usize::from(self.view.1.saturating_sub(2)).max(1)
    }

    fn scroll_into_view(&mut self) {
        let rows = self.data_rows();
        if self.row >= 1 {
            if self.row < self.top {
                self.top = self.row;
            } else if self.row >= self.top + rows {
                self.top = self.row + 1 - rows;
            }
        }
        self.top = self.top.max(1);
        if self.col < self.left {
            self.left = self.col;
        }
        let avail = usize::from(self.view.0).saturating_sub(self.gutter() + 1);
        while self.left < self.col && self.span(self.left, self.col) > avail {
            self.left += 1;
        }
    }

    /// Screen width of columns `from..=to`, each followed by a separator.
    fn span(&self, from: usize, to: usize) -> usize {
        (from..=to).map(|c| self.widths.get(c).copied().unwrap_or(1) + 3).sum()
    }

    fn gutter(&self) -> usize {
        self.rows.len().to_string().len() + 1
    }

    fn move_to(&mut self, row: isize, col: isize) {
        self.row = row.clamp(0, self.rows.len() as isize - 1) as usize;
        self.col = col.clamp(0, self.columns() as isize - 1) as usize;
        self.scroll_into_view();
    }

    // ----- keys -----

    pub fn handle_key(&mut self, key: &KeyEvent) -> Outcome {
        self.message = None;
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if self.confirm_close {
            self.confirm_close = false;
            return match key.code {
                KeyCode::Char('y' | 'Y') => match self.save() {
                    Ok(()) => Outcome::Close,
                    Err(e) => {
                        self.message = Some(format!("can't save: {e}"));
                        Outcome::Stay
                    }
                },
                KeyCode::Char('n' | 'N') => Outcome::Close,
                _ => Outcome::Stay,
            };
        }
        let (save, close) = (is_save(key), is_close(key));
        if self.edit.is_some() {
            if !save && !close {
                self.edit_key(key);
                return Outcome::Stay;
            }
            // Saving or closing keeps the cell being edited first.
            self.keep_edit();
        }
        if save {
            self.message = Some(if self.book.is_some() {
                self.read_only_message()
            } else {
                match self.save() {
                    Ok(()) => format!("saved {}", self.path.display()),
                    Err(e) => format!("can't save: {e}"),
                }
            });
            return Outcome::Stay;
        }
        if close {
            return self.close();
        }
        if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            match key.code {
                KeyCode::Char('v') => return self.toggle_range(false),
                KeyCode::Char('V') => return self.toggle_range(true),
                KeyCode::Char('y') => return self.copy(),
                KeyCode::Esc if self.anchor.is_some() => {
                    self.anchor = None;
                    return Outcome::Stay;
                }
                KeyCode::Char('o') if self.dirty => {
                    self.message = Some("unsaved changes: Ctrl+s saves them first".into());
                    return Outcome::Stay;
                }
                KeyCode::Char('o') => return Outcome::OpenApp(self.path.clone()),
                // Changing cells ends a selection.
                KeyCode::Char('e' | 'a' | 'A' | 'd' | 'D' | 'u') | KeyCode::Enter => self.anchor = None,
                _ => {}
            }
        }
        let (row, col) = (self.row as isize, self.col as isize);
        let page = self.data_rows() as isize;
        if let Some(book) = &self.book {
            let last = book.names.len().saturating_sub(1);
            match key.code {
                KeyCode::Char(']') if book.current < last => return Outcome::Sheet(book.current + 1),
                KeyCode::Char('[') if book.current > 0 => return Outcome::Sheet(book.current - 1),
                // Past the first or last sheet nothing happens; the title says which sheet this is.
                KeyCode::Char('[' | ']') => return Outcome::Stay,
                KeyCode::Char('e' | 'a' | 'A' | 'd' | 'D' | 'u') | KeyCode::Enter => {}
                _ => return self.move_key(key, row, col, page, alt),
            }
            self.message = Some(self.read_only_message());
            return Outcome::Stay;
        }
        match key.code {
            _ if alt || key.modifiers.contains(KeyModifiers::CONTROL) => {
                return self.move_key(key, row, col, page, alt);
            }
            KeyCode::Char('e') | KeyCode::Enter => {
                let text = self.cell(self.row, self.col).to_owned();
                let cursor = text.chars().count();
                self.edit = Some(Edit { text, cursor });
            }
            KeyCode::Char('a') => {
                let at = self.row + 1;
                self.insert_row(at);
                self.move_to(at as isize, col);
            }
            KeyCode::Char('A') => {
                let at = self.col + 1;
                self.insert_col(at);
                // Name the new column right away.
                self.move_to(0, at as isize);
                self.edit = Some(Edit { text: String::new(), cursor: 0 });
            }
            KeyCode::Char('d') => {
                if self.row == 0 {
                    self.message = Some("the header row stays; D deletes a column".into());
                } else {
                    self.delete_row(self.row);
                    self.clamp();
                }
            }
            KeyCode::Char('D') => {
                if self.columns() == 1 {
                    self.message = Some("the last column stays".into());
                } else {
                    self.delete_col(self.col);
                    self.clamp();
                }
            }
            KeyCode::Char('u') => self.undo(),
            _ => return self.move_key(key, row, col, page, alt),
        }
        Outcome::Stay
    }

    /// Keys that move around or close, which work read-only too.
    fn move_key(&mut self, key: &KeyEvent, row: isize, col: isize, page: isize, alt: bool) -> Outcome {
        match key.code {
            KeyCode::Char('q') if key.modifiers.is_empty() => return self.close(),
            _ if alt || key.modifiers.contains(KeyModifiers::CONTROL) => {}
            KeyCode::Up | KeyCode::Char('k') => self.move_to(row - 1, col),
            KeyCode::Down | KeyCode::Char('j') => self.move_to(row + 1, col),
            KeyCode::Left | KeyCode::Char('h') => self.move_to(row, col - 1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab => self.move_to(row, col + 1),
            KeyCode::BackTab => self.move_to(row, col - 1),
            KeyCode::PageUp => self.move_to(row - page, col),
            KeyCode::PageDown => self.move_to(row + page, col),
            KeyCode::Char('g') => self.move_to(0, col),
            KeyCode::Char('G') => self.move_to(isize::MAX / 2, col),
            KeyCode::Char('0') | KeyCode::Home => self.move_to(row, 0),
            KeyCode::Char('$') | KeyCode::End => self.move_to(row, isize::MAX / 2),
            _ => {}
        }
        Outcome::Stay
    }

    // ----- selecting and copying -----

    /// `v` (or `V`, whole rows) starts a range at the cursor; the same key again drops it, the other switches.
    fn toggle_range(&mut self, whole_rows: bool) -> Outcome {
        match self.anchor {
            Some(_) if self.whole_rows == whole_rows => self.anchor = None,
            Some(_) => self.whole_rows = whole_rows,
            None => {
                self.anchor = Some((self.row, self.col));
                self.whole_rows = whole_rows;
            }
        }
        Outcome::Stay
    }

    /// The selected rows and columns, inclusive: the range, or just the cursor's cell.
    fn range(&self) -> (usize, usize, usize, usize) {
        let Some((row, col)) = self.anchor else { return (self.row, self.row, self.col, self.col) };
        let (r0, r1) = (row.min(self.row), row.max(self.row));
        if self.whole_rows {
            return (r0, r1, 0, self.columns() - 1);
        }
        (r0, r1, col.min(self.col), col.max(self.col))
    }

    fn selected(&self, row: usize, col: usize) -> bool {
        let (r0, r1, c0, c1) = self.range();
        self.anchor.is_some() && (r0..=r1).contains(&row) && (c0..=c1).contains(&col)
    }

    /// The range as spreadsheet apps put cells on the clipboard: a tab between cells, a newline between rows,
    /// quotes around a cell holding a tab, newline or `"`. No newline at the end, so pasting into a shell runs
    /// nothing. Whole rows keep each row's own length.
    fn range_text(&self) -> String {
        let (r0, r1, c0, c1) = self.range();
        let whole = self.anchor.is_some() && self.whole_rows;
        let mut text = String::new();
        for r in r0..=r1 {
            if r > r0 {
                text.push('\n');
            }
            let end = if whole { self.rows[r].len() } else { c1 + 1 };
            for c in c0..end {
                if c > c0 {
                    text.push('\t');
                }
                text.push_str(&quote(self.cell(r, c)));
            }
        }
        text
    }

    /// `y`, or letting go after a drag: the range, or the cursor's cell, to the clipboard.
    fn copy(&mut self) -> Outcome {
        let text = self.range_text();
        if text.len() > MAX_COPY {
            self.message = Some(format!("too big to copy ({:.1} MB)", text.len() as f64 / f64::from(1 << 20)));
            return Outcome::Stay;
        }
        let (r0, r1, c0, c1) = self.range();
        let count = |n: usize, what: &str| if n == 1 { format!("1 {what}") } else { format!("{n} {what}s") };
        let what = match (r1 - r0 + 1, c1 - c0 + 1) {
            (rows, _) if self.anchor.is_some() && self.whole_rows => count(rows, "row"),
            (1, 1) => "1 cell".to_owned(),
            (rows, cols) => format!("{} × {}", count(rows, "row"), count(cols, "column")),
        };
        self.anchor = None;
        Outcome::Copy { text, what }
    }

    /// Closes, or with unsaved changes asks whether to save first.
    fn close(&mut self) -> Outcome {
        if !self.dirty {
            return Outcome::Close;
        }
        self.confirm_close = true;
        Outcome::Stay
    }

    fn read_only_message(&self) -> String {
        let ext = self.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        format!("read-only: tos can't save .{ext}; o opens it in its default app")
    }

    /// Ends editing a cell, keeping what was typed.
    fn keep_edit(&mut self) {
        if let Some(edit) = self.edit.take() {
            self.set_cell(self.row, self.col, edit.text);
        }
    }

    fn edit_key(&mut self, key: &KeyEvent) {
        let Some(edit) = self.edit.as_mut() else { return };
        let len = edit.text.chars().count();
        let byte = |text: &str, i: usize| text.char_indices().nth(i).map_or(text.len(), |(b, _)| b);
        match key.code {
            KeyCode::Enter | KeyCode::Tab => {
                self.keep_edit();
                if key.code == KeyCode::Tab {
                    self.move_to(self.row as isize, self.col as isize + 1);
                }
            }
            KeyCode::Esc => self.edit = None,
            KeyCode::Left => edit.cursor = edit.cursor.saturating_sub(1),
            KeyCode::Right => edit.cursor = (edit.cursor + 1).min(len),
            KeyCode::Home => edit.cursor = 0,
            KeyCode::End => edit.cursor = len,
            KeyCode::Backspace if edit.cursor > 0 => {
                let at = byte(&edit.text, edit.cursor - 1);
                edit.text.remove(at);
                edit.cursor -= 1;
            }
            KeyCode::Delete if edit.cursor < len => {
                let at = byte(&edit.text, edit.cursor);
                edit.text.remove(at);
            }
            KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                let at = byte(&edit.text, edit.cursor);
                edit.text.insert(at, c);
                edit.cursor += 1;
            }
            _ => {}
        }
    }

    /// The cell at screen cell (`x`, `y`) of the body: None on the rule under the header, or the row numbers.
    fn cell_at(&self, x: u16, y: u16, body: Rect) -> Option<(usize, usize)> {
        let row = match y.checked_sub(body.y)? {
            0 => 0,
            1 => return None,
            y => self.top + usize::from(y) - 2,
        };
        let mut left = usize::from(x.checked_sub(body.x)?).checked_sub(self.gutter())?;
        let mut col = self.left;
        loop {
            let w = self.widths.get(col).copied().unwrap_or(1) + 3;
            if left < w || col + 1 >= self.columns() {
                return Some((row, col));
            }
            left -= w;
            col += 1;
        }
    }

    /// A click picks a cell; a drag selects a range, which is copied when the button comes up; the wheel scrolls.
    pub fn handle_mouse(&mut self, ev: &MouseEvent, body: Rect) -> Outcome {
        let (row, col) = (self.row as isize, self.col as isize);
        match ev.kind {
            MouseEventKind::ScrollDown => self.move_to(row + 3, col),
            MouseEventKind::ScrollUp => self.move_to(row - 3, col),
            MouseEventKind::Down(MouseButton::Left) if self.edit.is_none() => {
                self.anchor = None;
                self.press = None;
                if let Some((r, c)) = self.cell_at(ev.column, ev.row, body) {
                    self.move_to(r as isize, c as isize);
                    self.press = Some((self.row, self.col));
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(press) = self.press else { return Outcome::Stay };
                let Some((mut r, c)) = self.cell_at(ev.column, ev.row, body) else { return Outcome::Stay };
                // On the top or bottom data row, reach one further, which scrolls.
                let y = ev.row - body.y;
                if y == 2 && r > 1 {
                    r -= 1;
                } else if y >= 2 && y + 1 >= body.height {
                    r += 1;
                }
                self.move_to(r as isize, c as isize);
                if self.anchor.is_none() && (self.row, self.col) != press {
                    self.anchor = Some(press);
                    self.whole_rows = false;
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // The button is up whatever happens; a drag that selected something copies it.
                let pressed = self.press.take().is_some();
                if pressed && self.anchor.is_some() {
                    return self.copy();
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    // ----- drawing -----

    pub fn hint(&self) -> &'static str {
        if self.confirm_close {
            "unsaved changes: y save and close · n close without saving · Esc stay"
        } else if self.edit.is_some() {
            "editing a cell · Enter keep · Tab keep and move right · Esc cancel"
        } else if self.anchor.is_some() {
            "y copy · Esc cancel"
        } else if self.book.is_some() {
            "v select · y copy · o app · [ ] sheet · g/G top/bottom · q close"
        } else {
            "e edit · v select · y copy · a/A add row/column · d/D delete · u undo · Ctrl+s save · o app · q close"
        }
    }

    /// The frame title's right-hand side.
    pub fn info(&self) -> String {
        let mark = if self.dirty { " ● unsaved" } else { "" };
        let place = format!("row {}/{} · col {}{mark}", self.row, self.rows.len() - 1, self.col + 1);
        match &self.book {
            Some(b) => {
                let name = b.names.get(b.current).map_or("", String::as_str);
                format!("sheet {}/{} {name} · {place} · read-only", b.current + 1, b.names.len())
            }
            None => place,
        }
    }

    /// Called when the preview box changes size.
    pub fn resize(&mut self, width: u16, height: u16) {
        if self.view != (width, height) {
            self.view = (width, height);
            self.scroll_into_view();
        }
    }

    pub fn render(&self, buf: &mut Buffer, body: Rect, focused: bool) {
        let gutter = self.gutter();
        let dim = Style::default().fg(Color::DarkGray);
        let header = Style::default().add_modifier(Modifier::BOLD);
        let selected = if focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default().bg(Color::DarkGray)
        };
        let in_range = Style::default().bg(Color::Blue).fg(Color::White);
        let draw_row = |buf: &mut Buffer, y: u16, row: usize, base: Style| {
            let label = if row == 0 { String::new() } else { row.to_string() };
            put_str(buf, body.x, y, &format!("{label:>w$} ", w = gutter - 1), dim, body.width);
            let mut x = body.x + gutter as u16;
            for c in self.left..self.columns() {
                if x >= body.right() {
                    break;
                }
                let w = self.widths[c];
                let is_cell = row == self.row && c == self.col;
                let text = match (&self.edit, is_cell) {
                    (Some(edit), true) => {
                        // Show the end of long text, with a bar for the cursor.
                        let mut shown: String = edit.text.chars().take(edit.cursor).collect();
                        shown.push('▏');
                        shown.extend(edit.text.chars().skip(edit.cursor));
                        let skip = shown.chars().count().saturating_sub(w.max(1));
                        shown.chars().skip(skip).collect()
                    }
                    _ => {
                        let t = self.cell(row, c);
                        if width_of(t) > w {
                            let mut s: String = t.chars().take(w.saturating_sub(1)).collect();
                            s.push('…');
                            s
                        } else {
                            t.to_owned()
                        }
                    }
                };
                let style = if is_cell {
                    selected.patch(base)
                } else if self.selected(row, c) {
                    in_range.patch(base)
                } else {
                    base
                };
                let room = body.right() - x;
                fill(buf, Rect::new(x, y, (w as u16 + 2).min(room), 1), " ", style);
                put_str(buf, x + 1, y, &text, style, room.saturating_sub(1));
                x += (w + 2) as u16;
                if x < body.right() {
                    put_str(buf, x, y, "│", dim, 1);
                }
                x += 1;
            }
        };
        draw_row(buf, body.y, 0, header);
        if body.height > 1 {
            let rule: String = "─".repeat(usize::from(body.width));
            put_str(buf, body.x, body.y + 1, &rule, dim, body.width);
        }
        for i in 0..self.data_rows() {
            let row = self.top + i;
            let y = body.y + 2 + i as u16;
            if row >= self.rows.len() || y >= body.bottom() {
                break;
            }
            draw_row(buf, y, row, Style::default());
        }
        if let Some(msg) = &self.message {
            let y = body.bottom().saturating_sub(1);
            fill(buf, Rect::new(body.x, y, body.width, 1), " ", Style::default());
            put_str(buf, body.x + 1, y, msg, Style::default().fg(Color::Yellow), body.width.saturating_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_keys(g: &mut Grid, text: &str) {
        for c in text.chars() {
            g.handle_key(&key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn edits_save_the_same_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.csv");
        // CRLF, a quoted field, a ragged row, no newline at the end.
        fs::write(&path, "name,qty\r\n\"a, b\",3\r\nshort\r\nc,4").unwrap();
        let mut g = Grid::open(&path, b',').unwrap();
        // Move to the "3" and change it to 30.
        g.handle_key(&key(KeyCode::Down));
        g.handle_key(&key(KeyCode::Right));
        g.handle_key(&key(KeyCode::Char('e')));
        type_keys(&mut g, "0");
        g.handle_key(&key(KeyCode::Enter));
        assert!(g.dirty);
        g.save().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "name,qty\r\n\"a, b\",30\r\nshort\r\nc,4");
    }

    #[test]
    fn rows_columns_and_undo() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.tsv");
        fs::write(&path, "a\tb\n1\t2\n").unwrap();
        let mut g = Grid::open(&path, b'\t').unwrap();
        g.handle_key(&key(KeyCode::Char('j')));
        g.handle_key(&key(KeyCode::Char('a')));
        type_keys(&mut g, "e9");
        g.handle_key(&key(KeyCode::Enter));
        assert_eq!(String::from_utf8(g.to_text().unwrap()).unwrap(), "a\tb\n1\t2\n9\t\n");
        // A new column is named straight away.
        g.handle_key(&key(KeyCode::Char('A')));
        type_keys(&mut g, "c");
        g.handle_key(&key(KeyCode::Enter));
        assert_eq!(String::from_utf8(g.to_text().unwrap()).unwrap(), "a\tc\tb\n1\t\t2\n9\t\t\n");
        // Undo the name, the column, the 9 and the row.
        for _ in 0..4 {
            g.handle_key(&key(KeyCode::Char('u')));
        }
        assert_eq!(String::from_utf8(g.to_text().unwrap()).unwrap(), "a\tb\n1\t2\n");
        // q with no changes left closes at once... but undo counts as a change, so it asks.
        assert!(matches!(g.handle_key(&key(KeyCode::Char('q'))), Outcome::Stay));
        assert!(matches!(g.handle_key(&key(KeyCode::Char('n'))), Outcome::Close));
    }

    fn copied(o: Outcome) -> (String, String) {
        match o {
            Outcome::Copy { text, what } => (text, what),
            _ => panic!("nothing was copied"),
        }
    }

    fn csv(text: &str) -> (tempfile::TempDir, Grid) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.csv");
        fs::write(&path, text).unwrap();
        let g = Grid::open(&path, b',').unwrap();
        (dir, g)
    }

    #[test]
    fn y_copies_a_cell_or_a_range_as_tab_separated_text() {
        let (_dir, mut g) = csv("name,qty,note\napple,3,\"crisp, red\"\npear,10,\"say \"\"hi\"\"\"\nfig\n");
        // One cell: the cursor's.
        assert_eq!(copied(g.handle_key(&key(KeyCode::Char('y')))), ("name".into(), "1 cell".into()));
        // v, then moving, stretches a range; y copies it and ends it.
        g.handle_key(&key(KeyCode::Char('j')));
        g.handle_key(&key(KeyCode::Char('l')));
        g.handle_key(&key(KeyCode::Char('v')));
        g.handle_key(&key(KeyCode::Char('j')));
        g.handle_key(&key(KeyCode::Char('l')));
        assert_eq!(g.hint(), "y copy · Esc cancel");
        let (text, what) = copied(g.handle_key(&key(KeyCode::Char('y'))));
        // A comma needs no quotes; a quote does, doubled. No newline at the end.
        assert_eq!(text, "3\tcrisp, red\n10\t\"say \"\"hi\"\"\"");
        assert_eq!(what, "2 rows × 2 columns");
        assert!(g.anchor.is_none());
        // A short row's missing cells copy as empty ones: fig has no note.
        g.handle_key(&key(KeyCode::Char('v')));
        g.handle_key(&key(KeyCode::Char('j')));
        assert_eq!(copied(g.handle_key(&key(KeyCode::Char('y')))).0, "\"say \"\"hi\"\"\"\n");
        // Esc drops a range.
        g.handle_key(&key(KeyCode::Char('v')));
        g.handle_key(&key(KeyCode::Esc));
        assert!(g.anchor.is_none());
    }

    #[test]
    fn whole_rows_keep_their_own_length() {
        let (_dir, mut g) = csv("a,b,c\n1,2,3\nshort\n");
        g.handle_key(&key(KeyCode::Char('j')));
        g.handle_key(&key(KeyCode::Char('V')));
        g.handle_key(&key(KeyCode::Char('j')));
        assert_eq!(copied(g.handle_key(&key(KeyCode::Char('y')))), ("1\t2\t3\nshort".into(), "2 rows".into()));
    }

    #[test]
    fn a_drag_selects_and_copies_on_release() {
        let (_dir, mut g) = csv("name,qty\napple,3\npear,10\n");
        let body = Rect::new(0, 0, 40, 10);
        g.resize(body.width, body.height);
        let at = |kind, x, y| MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE };
        // Columns start after the 2-column row numbers: "name " is 5 wide plus 3, so qty starts at x 10.
        let left = MouseButton::Left;
        assert!(matches!(g.handle_mouse(&at(MouseEventKind::Down(left), 4, 2), body), Outcome::Stay));
        // A click alone copies nothing.
        assert!(matches!(g.handle_mouse(&at(MouseEventKind::Up(left), 4, 2), body), Outcome::Stay));
        assert_eq!((g.row, g.col), (1, 0));
        g.handle_mouse(&at(MouseEventKind::Down(left), 4, 2), body);
        g.handle_mouse(&at(MouseEventKind::Drag(left), 11, 2), body);
        g.handle_mouse(&at(MouseEventKind::Drag(left), 11, 3), body);
        let (text, what) = copied(g.handle_mouse(&at(MouseEventKind::Up(left), 11, 3), body));
        assert_eq!((text.as_str(), what.as_str()), ("apple\t3\npear\t10", "2 rows × 2 columns"));
    }

    #[test]
    fn o_opens_the_file_unless_there_are_unsaved_changes() {
        let (_dir, mut g) = csv("a\n1\n");
        assert!(matches!(g.handle_key(&key(KeyCode::Char('o'))), Outcome::OpenApp(p) if p == g.path));
        g.handle_key(&key(KeyCode::Char('e')));
        type_keys(&mut g, "2");
        g.handle_key(&key(KeyCode::Enter));
        assert!(matches!(g.handle_key(&key(KeyCode::Char('o'))), Outcome::Stay));
        assert_eq!(g.message.as_deref(), Some("unsaved changes: Ctrl+s saves them first"));
    }

    fn with(code: char, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(code), modifiers)
    }

    #[test]
    fn ctrl_s_saves_and_ctrl_e_closes() {
        let (dir, mut g) = csv("a,b\n1,2\n");
        let path = dir.path().join("t.csv");
        // Ctrl+s while a cell is being edited keeps the cell, then saves.
        g.handle_key(&key(KeyCode::Char('j')));
        g.handle_key(&key(KeyCode::Char('e')));
        type_keys(&mut g, "5");
        assert!(matches!(g.handle_key(&with('s', KeyModifiers::CONTROL)), Outcome::Stay));
        assert!(g.edit.is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), "a,b\n15,2\n");
        assert!(g.message.as_deref().unwrap().starts_with("saved "));
        // Saved, so Ctrl+e closes at once.
        assert!(matches!(g.handle_key(&with('e', KeyModifiers::CONTROL)), Outcome::Close));
    }

    #[test]
    fn ctrl_e_with_unsaved_changes_asks_and_alt_keys_still_work() {
        let (dir, mut g) = csv("a\n1\n");
        let path = dir.path().join("t.csv");
        g.handle_key(&key(KeyCode::Char('j')));
        g.handle_key(&key(KeyCode::Char('e')));
        type_keys(&mut g, "2");
        // Ctrl+e while editing keeps the cell too, and then asks, as there are changes.
        assert!(matches!(g.handle_key(&with('e', KeyModifiers::CONTROL)), Outcome::Stay));
        assert!(g.confirm_close);
        assert!(matches!(g.handle_key(&key(KeyCode::Esc)), Outcome::Stay));
        // Alt+s saves and Alt+q closes, as before.
        g.handle_key(&with('s', KeyModifiers::ALT));
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\n12\n");
        assert!(matches!(g.handle_key(&with('q', KeyModifiers::ALT)), Outcome::Close));
    }

    #[test]
    fn a_spreadsheet_says_read_only_on_ctrl_s_and_closes_on_ctrl_e() {
        let sheet = finder_core::sheet::Sheet {
            names: vec!["One".into()],
            current: 0,
            rows: vec![vec!["x".into()]],
            truncated: false,
        };
        let mut g = Grid::from_sheet(Path::new("/tmp/book.xlsx"), sheet);
        assert!(matches!(g.handle_key(&with('s', KeyModifiers::CONTROL)), Outcome::Stay));
        assert!(g.message.as_deref().unwrap().starts_with("read-only: tos can't save .xlsx"));
        assert!(matches!(g.handle_key(&with('e', KeyModifiers::CONTROL)), Outcome::Close));
    }

    #[test]
    fn past_the_first_or_last_sheet_nothing_happens() {
        let sheet = finder_core::sheet::Sheet {
            names: vec!["One".into(), "Two".into()],
            current: 0,
            rows: vec![vec!["x".into()]],
            truncated: false,
        };
        let mut g = Grid::from_sheet(Path::new("/tmp/book.xlsx"), sheet);
        assert!(matches!(g.handle_key(&key(KeyCode::Char('['))), Outcome::Stay));
        assert_eq!(g.message, None);
        assert!(matches!(g.handle_key(&key(KeyCode::Char(']'))), Outcome::Sheet(1)));
        // Read-only, but selecting and copying work.
        g.handle_key(&key(KeyCode::Char('e')));
        assert!(g.message.as_deref().unwrap().starts_with("read-only: tos can't save .xlsx"));
        assert_eq!(copied(g.handle_key(&key(KeyCode::Char('y')))).0, "x");
    }

    #[test]
    fn non_utf8_and_huge_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.csv");
        fs::write(&path, [0xff, 0xfe, b',', b'a']).unwrap();
        assert!(Grid::open(&path, b',').err().unwrap().contains("not UTF-8"));
    }
}
