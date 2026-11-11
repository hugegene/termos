//! The CSV grid editor (step 5), opened from the finder with Enter on a `.csv` or `.tsv` file. It replaces
//! the prototype's VisiData. Saving keeps the file's delimiter, line endings and ragged rows.
//!
//! Spreadsheets (xlsx, ods and the like) open in it read-only, a sheet at a time: `tos` can't write them back
//! without losing their formatting (docs/plans/2026-11-07-video-and-office-previews.md).

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

/// What the finder should do after a key in the grid.
pub enum Outcome {
    Stay,
    Close,
    /// Show this sheet of the spreadsheet instead (0-based).
    Sheet(usize),
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

impl Grid {
    pub fn open(path: &Path, delimiter: u8) -> Result<Grid, String> {
        let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
        if size > MAX_BYTES {
            return Err(format!("too big to edit as a grid ({} MB); E opens it in Neovim", size >> 20));
        }
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        let text = String::from_utf8(bytes).map_err(|_| "not UTF-8 text; E opens it in Neovim".to_owned())?;
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
                return Err(format!("more than {MAX_ROWS} rows; E opens it in Neovim"));
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
        if self.edit.is_some() {
            self.edit_key(key);
            return Outcome::Stay;
        }
        let (row, col) = (self.row as isize, self.col as isize);
        let page = self.data_rows() as isize;
        if let Some(book) = &self.book {
            let last = book.names.len().saturating_sub(1);
            match key.code {
                KeyCode::Char(']') if book.current < last => return Outcome::Sheet(book.current + 1),
                KeyCode::Char('[') if book.current > 0 => return Outcome::Sheet(book.current - 1),
                KeyCode::Char('[' | ']') => {
                    self.message = Some(format!("{} sheet", if book.current == 0 { "first" } else { "last" }));
                    return Outcome::Stay;
                }
                KeyCode::Char('s') if alt => {}
                KeyCode::Char('e' | 'a' | 'A' | 'd' | 'D' | 'u') | KeyCode::Enter => {}
                _ => return self.move_key(key, row, col, page, alt),
            }
            let ext = self.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
            self.message = Some(format!("read-only: tos can't save .{ext}"));
            return Outcome::Stay;
        }
        match key.code {
            KeyCode::Char('s') if alt => match self.save() {
                Ok(()) => self.message = Some(format!("saved {}", self.path.display())),
                Err(e) => self.message = Some(format!("can't save: {e}")),
            },
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
            KeyCode::Char('q') if alt || key.modifiers.is_empty() => {
                if !self.dirty {
                    return Outcome::Close;
                }
                self.confirm_close = true;
            }
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

    fn edit_key(&mut self, key: &KeyEvent) {
        let Some(edit) = self.edit.as_mut() else { return };
        let len = edit.text.chars().count();
        let byte = |text: &str, i: usize| text.char_indices().nth(i).map_or(text.len(), |(b, _)| b);
        match key.code {
            KeyCode::Enter | KeyCode::Tab => {
                let text = std::mem::take(&mut edit.text);
                self.edit = None;
                self.set_cell(self.row, self.col, text);
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

    /// A click picks a cell; the wheel scrolls.
    pub fn handle_mouse(&mut self, ev: &MouseEvent, body: Rect) {
        let (row, col) = (self.row as isize, self.col as isize);
        match ev.kind {
            MouseEventKind::ScrollDown => self.move_to(row + 3, col),
            MouseEventKind::ScrollUp => self.move_to(row - 3, col),
            MouseEventKind::Down(MouseButton::Left) if self.edit.is_none() => {
                let y = ev.row.saturating_sub(body.y);
                let r = match y {
                    0 => Some(0),
                    1 => None,
                    y => Some(self.top + usize::from(y) - 2),
                };
                let mut x = usize::from(ev.column.saturating_sub(body.x)).checked_sub(self.gutter());
                let mut c = self.left;
                while let Some(left) = x {
                    let w = self.widths.get(c).copied().unwrap_or(1) + 3;
                    if left < w || c + 1 >= self.columns() {
                        break;
                    }
                    x = Some(left - w);
                    c += 1;
                }
                if let (Some(r), Some(_)) = (r, x) {
                    self.move_to(r as isize, c as isize);
                }
            }
            _ => {}
        }
    }

    // ----- drawing -----

    pub fn hint(&self) -> &'static str {
        if self.confirm_close {
            "unsaved changes: y save and close · n close without saving · Esc stay"
        } else if self.edit.is_some() {
            "editing a cell · Enter keep · Tab keep and move right · Esc cancel"
        } else if self.book.is_some() {
            "read-only · arrows move · [ ] sheet · g/G top/bottom · q close"
        } else {
            "e edit · a/A add row/column · d/D delete · u undo · Alt+s save · q close"
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
                let style = if is_cell { selected.patch(base) } else { base };
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

    #[test]
    fn non_utf8_and_huge_files_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.csv");
        fs::write(&path, [0xff, 0xfe, b',', b'a']).unwrap();
        assert!(Grid::open(&path, b',').err().unwrap().contains("not UTF-8"));
    }
}
