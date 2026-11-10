//! The file list's menu, opened by a right-click or `m`: rename, copy name, copy path, delete. Also the rename
//! field and the "delete for good?" question that the menu leads to.
//! Plan: docs/plans/2026-11-07-finder-menu.md.

use std::io;
use std::path::Path;
use std::process::{Command, Stdio};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::super::layout::contains;
use super::super::render::{draw_frame, fill, put_str};
use super::{Action, Finder};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Rename,
    CopyName,
    CopyPath,
    Delete,
}

/// The menu's items, their labels and their letters.
const ITEMS: [(Item, &str, char); 4] = [
    (Item::Rename, "Rename", 'r'),
    (Item::CopyName, "Copy name", 'y'),
    (Item::CopyPath, "Copy path", 'p'),
    (Item::Delete, "Delete", 'd'),
];

/// The menu box: " Copy name y " inside a frame.
const MENU_WIDTH: u16 = 15;
const MENU_HEIGHT: u16 = ITEMS.len() as u16 + 2;

/// The question box is at least this wide, spilling over the preview if the list is narrower.
const CONFIRM_WIDTH: u16 = 30;

/// What's open over the file list. Entries are named rather than indexed, since the folder can change meanwhile.
pub enum Popup {
    /// The menu for the entry `name`, in its box at `rect` (screen cells), with item `hover` highlighted.
    Menu { name: String, rect: Rect, hover: usize },
    /// The entry `name`'s row is a text field holding its new name.
    Rename { name: String, edit: LineEdit },
    /// Moving `name` to the trash failed for `reason`; deleting it for good waits for `y`.
    Confirm { name: String, reason: String, is_dir: bool, rect: Rect },
}

/// A one-line text field: its text and the cursor's position, in characters.
pub struct LineEdit {
    pub text: String,
    pub cursor: usize,
}

impl LineEdit {
    fn new(text: &str) -> Self {
        LineEdit { text: text.to_owned(), cursor: text.chars().count() }
    }

    fn byte(&self, i: usize) -> usize {
        self.text.char_indices().nth(i).map_or(self.text.len(), |(b, _)| b)
    }

    /// Edits the text. Returns false for keys it doesn't use.
    fn key(&mut self, key: &KeyEvent) -> bool {
        let len = self.text.chars().count();
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(len),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = len,
            KeyCode::Backspace if self.cursor > 0 => {
                let at = self.byte(self.cursor - 1);
                self.text.remove(at);
                self.cursor -= 1;
            }
            KeyCode::Delete if self.cursor < len => {
                let at = self.byte(self.cursor);
                self.text.remove(at);
            }
            KeyCode::Char('u') if ctrl => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                let at = self.byte(self.cursor);
                self.text.insert(at, c);
                self.cursor += 1;
            }
            _ => return false,
        }
        true
    }

    /// The text with a bar at the cursor, cut at the start to fit `width` columns (but never past the cursor).
    pub fn shown(&self, width: usize) -> String {
        let mut s: String = self.text.chars().take(self.cursor).collect();
        s.push('▏');
        s.extend(self.text.chars().skip(self.cursor));
        // Keep the cursor in view.
        let skip = s.chars().count().saturating_sub(width.max(1)).min(self.cursor);
        s.chars().skip(skip).take(width.max(1)).collect()
    }
}

/// A box of `size` next to the list row at `row`, starting at column `x`: below the row if it fits in `list`,
/// else above it, else at the list's top. It's moved left to fit the list, but never left of it.
fn place(x: u16, row: u16, size: (u16, u16), list: Rect) -> Rect {
    let (w, h) = size;
    let x = x.min(list.right().saturating_sub(w)).max(list.x);
    let y = if row + 1 + h <= list.bottom() {
        row + 1
    } else if row >= list.y + h {
        row - h
    } else {
        list.y
    };
    Rect::new(x, y, w, h)
}

/// The command that moves a file to the trash: `$TB_TRASH` (the path is added at the end), or the system's
/// (`gio trash --` on Linux, `trash` on macOS).
pub fn trash_command() -> Vec<String> {
    match std::env::var("TB_TRASH") {
        Ok(c) if !c.trim().is_empty() => c.split_whitespace().map(str::to_owned).collect(),
        _ => crate::os::default_trash_command(),
    }
}

/// Why the trash program can't run because it's missing. A missing default also says how to pick another.
fn not_installed(program: &str, is_default: bool) -> String {
    if is_default {
        format!("'{program}' isn't installed; $TB_TRASH can name another trash command")
    } else {
        format!("'{program}' isn't installed")
    }
}

/// Moves `path` to the trash with `command`. The error is a short reason, such as gio's "Trashing on system
/// internal mounts is not supported".
fn trash(command: &[String], path: &Path) -> Result<(), String> {
    let Some(program) = command.first() else { return Err("no trash command".into()) };
    let out = Command::new(program)
        .args(&command[1..])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    match out {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            // gio says "gio: file:///…/x: Trashing on system internal mounts is not supported"; keep the end.
            let err = String::from_utf8_lossy(&out.stderr);
            let line = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("");
            let reason = line.rsplit(": ").next().unwrap_or("").trim();
            Err(if reason.is_empty() { format!("'{}' failed ({})", program, out.status) } else { reason.to_owned() })
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(not_installed(program, command == crate::os::default_trash_command().as_slice()))
        }
        Err(e) => Err(format!("can't run '{}': {e}", program)),
    }
}

/// Renames `from` to `to` without ever replacing an existing file.
fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        // Some filesystems don't support the flag; check first instead.
        Err(rustix::io::Errno::INVAL) => {
            if to.symlink_metadata().is_ok() {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            std::fs::rename(from, to)
        }
        r => r.map_err(io::Error::from),
    }
}

/// Greedy word wrap to `width` columns; words longer than a line are cut.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let mut word: String = word.to_owned();
        loop {
            let (have, need) = (line.chars().count(), word.chars().count());
            let room = if have == 0 { width } else { width.saturating_sub(have + 1) };
            if need <= room {
                if have > 0 {
                    line.push(' ');
                }
                line.push_str(&word);
                break;
            }
            if have > 0 {
                lines.push(std::mem::take(&mut line));
                continue;
            }
            lines.push(word.chars().take(width).collect());
            word = word.chars().skip(width).collect();
        }
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

impl Finder {
    /// The screen row of visible entry `index`, if it's on screen.
    fn row_of(&self, index: usize) -> Option<u16> {
        let rows = self.entries_rect(self.list_rect);
        let offset = index.checked_sub(self.list_top)?;
        (offset < usize::from(rows.height)).then(|| rows.y + offset as u16)
    }

    /// Selects visible entry `index` and opens the menu for it, at column `x`.
    pub(super) fn open_menu(&mut self, index: usize, x: u16) {
        self.select(index);
        let (Some(row), Some(e)) = (self.row_of(self.selected), self.current()) else { return };
        let name = e.name.clone();
        let rect = place(x, row, (MENU_WIDTH, MENU_HEIGHT), self.list_rect);
        self.popup = Some(Popup::Menu { name, rect, hover: 0 });
    }

    /// `m`: the menu for the selected entry.
    pub(super) fn open_menu_at_selection(&mut self) {
        let x = self.entries_rect(self.list_rect).x + 2;
        self.open_menu(self.selected, x);
    }

    pub fn close_popup(&mut self) {
        self.popup = None;
    }

    /// The box drawn over the panes (the menu or the question), in screen cells.
    pub fn popup_rect(&self) -> Option<Rect> {
        match self.popup.as_ref()? {
            Popup::Menu { rect, .. } | Popup::Confirm { rect, .. } => Some(*rect),
            Popup::Rename { .. } => None,
        }
    }

    /// Whether the box covers part of the preview, where a real image can't be drawn over.
    pub fn popup_covers_preview(&self) -> bool {
        self.popup_rect().is_some_and(|r| r.intersects(self.preview_rect))
    }

    pub(super) fn popup_hint(&self) -> Option<&'static str> {
        Some(match self.popup.as_ref()? {
            Popup::Menu { .. } => "menu · j/k move · Enter or a letter runs · Esc closes",
            Popup::Rename { .. } => "rename · type the new name · Enter renames · Esc cancels",
            Popup::Confirm { .. } => "y deletes for good · any other key keeps it",
        })
    }

    /// The new name being typed for the entry `name`, if it's being renamed.
    pub(super) fn renaming(&self, name: &str) -> Option<&LineEdit> {
        match &self.popup {
            Some(Popup::Rename { name: n, edit }) if n == name => Some(edit),
            _ => None,
        }
    }

    /// A key while something is open. None if nothing is open; otherwise the key is used up.
    pub(super) fn popup_key(&mut self, key: &KeyEvent) -> Option<Action> {
        let popup = self.popup.take()?;
        let ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
        Some(match popup {
            Popup::Menu { name, rect, hover } => {
                let last = ITEMS.len() - 1;
                let hover = match key.code {
                    KeyCode::Char('j') | KeyCode::Down => (hover + 1).min(last),
                    KeyCode::Char('k') | KeyCode::Up => hover.saturating_sub(1),
                    KeyCode::Enter => return Some(self.run(ITEMS[hover].0, name)),
                    KeyCode::Esc | KeyCode::Char('q' | 'm') => return Some(Action::None),
                    _ if ctrl_c => return Some(Action::None),
                    KeyCode::Char(c) => match ITEMS.iter().find(|i| i.2 == c) {
                        Some(i) => return Some(self.run(i.0, name)),
                        None => hover,
                    },
                    _ => hover,
                };
                self.popup = Some(Popup::Menu { name, rect, hover });
                Action::None
            }
            Popup::Rename { name, mut edit } => match key.code {
                KeyCode::Enter => self.finish_rename(&name, &edit.text),
                KeyCode::Esc => Action::None,
                _ if ctrl_c => Action::None,
                _ => {
                    edit.key(key);
                    self.popup = Some(Popup::Rename { name, edit });
                    Action::None
                }
            },
            Popup::Confirm { name, is_dir, .. } => match key.code {
                KeyCode::Char('y') if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                    self.delete_for_good(&name, is_dir)
                }
                _ => Action::Flash(format!("kept '{name}'")),
            },
        })
    }

    /// A mouse event while something is open. None if nothing is open; otherwise the event is used up.
    pub(super) fn popup_mouse(&mut self, ev: &MouseEvent) -> Option<Action> {
        let popup = self.popup.take()?;
        let (x, y) = (ev.column, ev.row);
        let down = matches!(ev.kind, MouseEventKind::Down(_));
        let wheel = matches!(ev.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown);
        match popup {
            Popup::Menu { name, rect, hover } => {
                let inside = contains(rect, x, y);
                let item = (inside && y > rect.y && y + 1 < rect.bottom()).then(|| usize::from(y - rect.y - 1));
                match (ev.kind, item) {
                    (MouseEventKind::Down(MouseButton::Left), Some(i)) => return Some(self.run(ITEMS[i].0, name)),
                    // A right-click on another entry moves the menu there.
                    (MouseEventKind::Down(MouseButton::Right), _) if !inside => {
                        if let Some(index) = self.entry_at(x, y) {
                            self.open_menu(index, x);
                        }
                        return Some(Action::None);
                    }
                    // A click outside, or the wheel, closes the menu and does nothing else.
                    _ if (down && !inside) || wheel => return Some(Action::None),
                    _ => {}
                }
                let hover = item.unwrap_or(hover);
                self.popup = Some(Popup::Menu { name, rect, hover });
                Some(Action::None)
            }
            // A click anywhere cancels a rename, or keeps the file the question is about.
            Popup::Confirm { name, .. } if down => Some(Action::Flash(format!("kept '{name}'"))),
            Popup::Rename { .. } if down => Some(Action::None),
            p => {
                self.popup = Some(p);
                Some(Action::None)
            }
        }
    }

    /// The visible entry at screen cell (`x`, `y`) of the list.
    pub(super) fn entry_at(&self, x: u16, y: u16) -> Option<usize> {
        let rows = self.entries_rect(self.list_rect);
        if !contains(rows, x, y) {
            return None;
        }
        let index = self.list_top + usize::from(y - rows.y);
        (index < self.visible.len()).then_some(index)
    }

    fn run(&mut self, item: Item, name: String) -> Action {
        match item {
            Item::Rename => {
                let edit = LineEdit::new(&name);
                self.popup = Some(Popup::Rename { name, edit });
                Action::None
            }
            Item::CopyName => Action::Copy(name),
            Item::CopyPath => Action::Copy(self.dir.join(&name).display().to_string()),
            Item::Delete => self.delete(&name),
        }
    }

    fn finish_rename(&mut self, name: &str, new: &str) -> Action {
        if new.is_empty() || new == name {
            return Action::None;
        }
        if new.contains(['/', '\0']) {
            return Action::Flash("a name can't contain '/'".into());
        }
        if new == "." || new == ".." {
            return Action::Flash(format!("'{new}' can't be a name"));
        }
        match rename_no_replace(&self.dir.join(name), &self.dir.join(new)) {
            Ok(()) => {
                self.reload();
                self.refilter(Some(new));
                Action::Flash(format!("renamed '{name}' to '{new}'"))
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Action::Flash(format!("'{new}' already exists")),
            Err(e) => Action::Flash(format!("can't rename '{name}': {e}")),
        }
    }

    /// Reloads the folder after an entry went away, keeping the selection on the same row.
    fn reload_at(&mut self, index: usize) {
        self.reload();
        self.select(index.min(self.visible.len().saturating_sub(1)));
    }

    fn delete(&mut self, name: &str) -> Action {
        let path = self.dir.join(name);
        let index = self.selected;
        let Ok(meta) = path.symlink_metadata() else {
            self.reload_at(index);
            return Action::Flash(format!("'{name}' is gone"));
        };
        match trash(&self.trash, &path) {
            Ok(()) => {
                self.reload_at(index);
                Action::Flash(format!("moved '{name}' to the trash"))
            }
            Err(reason) => {
                let row = self.visible.iter().position(|i| self.entries[*i].name == name).and_then(|i| self.row_of(i));
                let row = row.unwrap_or(self.list_rect.y);
                let is_dir = meta.is_dir();
                let width = self.list_rect.width.max(CONFIRM_WIDTH);
                let height = confirm_lines(name, &reason, is_dir, width).len() as u16 + 2;
                let rect = place(self.list_rect.x, row, (width, height), self.list_rect);
                self.popup = Some(Popup::Confirm { name: name.to_owned(), reason, is_dir, rect });
                Action::None
            }
        }
    }

    fn delete_for_good(&mut self, name: &str, was_dir: bool) -> Action {
        let path = self.dir.join(name);
        let index = self.selected;
        // A symlink is removed itself: its own metadata is never a folder's.
        let result = match path.symlink_metadata() {
            Ok(m) if m.is_dir() != was_dir => return Action::Flash(format!("'{name}' changed meanwhile; kept it")),
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
            Err(e) => Err(e),
        };
        self.reload_at(index);
        match result {
            Ok(()) => Action::Flash(format!("deleted '{name}'")),
            Err(e) => Action::Flash(format!("can't delete '{name}': {e}")),
        }
    }

    /// Draws the menu or the question over the panes. The rename field is drawn in the list's row instead.
    pub fn render_popup(&self, buf: &mut Buffer) {
        let Some(popup) = &self.popup else { return };
        match popup {
            Popup::Menu { rect, hover, .. } => {
                let rect = rect.intersection(buf.area);
                fill(buf, rect, " ", Style::default());
                draw_frame(buf, rect, "", "", Style::default());
                for (i, (item, label, letter)) in ITEMS.iter().enumerate() {
                    let y = rect.y + 1 + i as u16;
                    if y + 1 >= rect.bottom() {
                        break;
                    }
                    let mut style =
                        if *item == Item::Delete { Style::default().fg(Color::Red) } else { Style::default() };
                    if i == *hover {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    let inside = rect.width.saturating_sub(2);
                    let text = format!(" {label:<w$}{letter} ", w = usize::from(inside.saturating_sub(3)));
                    put_str(buf, rect.x + 1, y, &text, style, inside);
                }
            }
            Popup::Confirm { name, reason, is_dir, rect } => {
                let rect = rect.intersection(buf.area);
                let red = Style::default().fg(Color::Red);
                fill(buf, rect, " ", Style::default());
                draw_frame(buf, rect, "delete", "", red.add_modifier(Modifier::BOLD));
                let lines = confirm_lines(name, reason, *is_dir, rect.width);
                for (i, line) in lines.iter().enumerate() {
                    let y = rect.y + 1 + i as u16;
                    if y + 1 >= rect.bottom() {
                        break;
                    }
                    let style = if i + 1 == lines.len() { red.add_modifier(Modifier::BOLD) } else { Style::default() };
                    put_str(buf, rect.x + 2, y, line, style, rect.width.saturating_sub(4));
                }
            }
            Popup::Rename { .. } => {}
        }
    }
}

/// The question's lines, wrapped for a box `width` wide.
fn confirm_lines(name: &str, reason: &str, is_dir: bool, width: u16) -> Vec<String> {
    let inside = usize::from(width.saturating_sub(4));
    let what = if is_dir { "the folder and everything in it" } else { "it" };
    let mut lines = wrap(&format!("Can't move '{name}' to the trash: {reason}."), inside);
    lines.extend(wrap(&format!("Delete {what} for good? y / n"), inside));
    lines
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::super::buffer_text;
    use super::*;

    /// A finder on a folder holding `a`, `b` and `c`, its list 30 × 12 with frames (entries from row 1).
    fn setup() -> (tempfile::TempDir, Finder, mpsc::Receiver<crate::server::Ev>) {
        let dir = tempfile::tempdir().unwrap();
        for n in ["a", "b", "c"] {
            std::fs::write(dir.path().join(n), n).unwrap();
        }
        let (tx, rx) = mpsc::channel();
        let f = Finder::new(1, dir.path().to_path_buf(), Rect::new(0, 0, 30, 12), true, tx);
        (dir, f, rx)
    }

    fn press(f: &mut Finder, code: KeyCode) -> Action {
        f.handle_key(&KeyEvent::new(code, KeyModifiers::NONE), false)
    }

    fn typed(f: &mut Finder, text: &str) {
        for c in text.chars() {
            press(f, KeyCode::Char(c));
        }
    }

    fn mouse(f: &mut Finder, kind: MouseEventKind, x: u16, y: u16) -> Action {
        f.handle_mouse(&MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }, false)
    }

    /// The list with whatever is open over it, as text.
    fn screen(f: &Finder) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, 50, 14));
        f.render_list(&mut buf, f.list_rect, true, Color::Green);
        f.render_popup(&mut buf);
        buffer_text(&buf)
    }

    /// The screen's words in order, without box lines, so wrapped sentences read as one.
    fn words(f: &Finder) -> String {
        let text = screen(f).replace(['│', '┌', '┐', '└', '┘', '─'], " ");
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn selected(f: &Finder) -> String {
        f.current().map(|e| e.name.clone()).unwrap_or_default()
    }

    fn flash(a: Action) -> String {
        match a {
            Action::Flash(s) => s,
            _ => String::new(),
        }
    }

    const RIGHT: MouseEventKind = MouseEventKind::Down(MouseButton::Right);
    const LEFT: MouseEventKind = MouseEventKind::Down(MouseButton::Left);

    #[test]
    fn a_right_click_selects_and_opens_the_menu() {
        let (_dir, mut f, _rx) = setup();
        mouse(&mut f, RIGHT, 4, 2);
        assert_eq!(selected(&f), "b");
        let text = screen(&f);
        assert!(text.contains("│ Rename    r │") && text.contains("│ Delete    d │"), "{text}");
        assert_eq!(f.popup_rect(), Some(Rect::new(4, 3, MENU_WIDTH, MENU_HEIGHT)));
        assert!(f.hint(false).starts_with("menu"));

        // Esc closes it; so does a click outside, which selects nothing.
        press(&mut f, KeyCode::Esc);
        assert!(f.popup.is_none());
        mouse(&mut f, RIGHT, 4, 1);
        mouse(&mut f, LEFT, 25, 3);
        assert!(f.popup.is_none());
        assert_eq!(selected(&f), "a");

        // A right-click on empty space opens nothing.
        mouse(&mut f, RIGHT, 4, 6);
        assert!(f.popup.is_none());
    }

    #[test]
    fn the_pointer_highlights_and_a_click_runs() {
        let (dir, mut f, _rx) = setup();
        mouse(&mut f, RIGHT, 4, 1);
        // The menu is at rows 2–7; "Copy path" is its third item, on row 5.
        mouse(&mut f, MouseEventKind::Moved, 6, 5);
        assert!(matches!(&f.popup, Some(Popup::Menu { hover: 2, .. })));
        let copied = mouse(&mut f, LEFT, 6, 5);
        assert!(matches!(&copied, Action::Copy(p) if *p == dir.path().join("a").display().to_string()));
        assert!(f.popup.is_none());
    }

    #[test]
    fn keys_open_and_run_the_menu() {
        let (_dir, mut f, _rx) = setup();
        press(&mut f, KeyCode::Char('j'));
        press(&mut f, KeyCode::Char('m'));
        assert!(matches!(&f.popup, Some(Popup::Menu { name, .. }) if name == "b"));
        press(&mut f, KeyCode::Char('j'));
        assert!(matches!(press(&mut f, KeyCode::Enter), Action::Copy(n) if n == "b"));
        press(&mut f, KeyCode::Char('m'));
        assert!(matches!(press(&mut f, KeyCode::Char('y')), Action::Copy(n) if n == "b"));
        // Letters that aren't items do nothing, and the list's keys wait.
        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('x'));
        press(&mut f, KeyCode::Char('q'));
        assert!(f.popup.is_none());
        assert_eq!(selected(&f), "b");
    }

    #[test]
    fn rename_in_the_row() {
        let (dir, mut f, _rx) = setup();
        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('r'));
        assert!(screen(&f).contains(" a▏"), "{}", screen(&f));
        typed(&mut f, "-new.txt");
        let done = flash(press(&mut f, KeyCode::Enter));
        assert_eq!(done, "renamed 'a' to 'a-new.txt'");
        assert!(dir.path().join("a-new.txt").exists() && !dir.path().join("a").exists());
        assert_eq!(selected(&f), "a-new.txt");

        // Taken names, slashes and ".." are refused, and nothing changes.
        for (name, why) in
            [("b", "'b' already exists"), ("x/y", "a name can't contain '/'"), ("..", "'..' can't be a name")]
        {
            press(&mut f, KeyCode::Char('m'));
            press(&mut f, KeyCode::Char('r'));
            f.handle_key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL), false);
            typed(&mut f, name);
            assert_eq!(flash(press(&mut f, KeyCode::Enter)), why);
        }
        assert_eq!(std::fs::read_to_string(dir.path().join("b")).unwrap(), "b");
        assert!(dir.path().join("a-new.txt").exists());

        // Esc cancels.
        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('r'));
        typed(&mut f, "zzz");
        press(&mut f, KeyCode::Esc);
        assert!(f.popup.is_none() && dir.path().join("a-new.txt").exists());
    }

    #[test]
    fn delete_moves_to_the_trash() {
        let (dir, mut f, _rx) = setup();
        let trash = tempfile::tempdir().unwrap();
        // The path is added last, so in the script $0 is the trash folder and $1 the file. (macOS's mv has no -t.)
        f.trash = vec!["sh".into(), "-c".into(), r#"mv -- "$1" "$0""#.into(), trash.path().display().to_string()];
        press(&mut f, KeyCode::Char('j'));
        press(&mut f, KeyCode::Char('m'));
        assert_eq!(flash(press(&mut f, KeyCode::Char('d'))), "moved 'b' to the trash");
        assert!(trash.path().join("b").exists() && !dir.path().join("b").exists());
        // The selection stays on the same row: the next file.
        assert_eq!(selected(&f), "c");
    }

    #[test]
    fn where_there_is_no_trash_only_y_deletes() {
        let (dir, mut f, _rx) = setup();
        f.trash = vec!["sh".into(), "-c".into(), "echo 'gio: file:///x: Trashing is not supported' >&2; exit 1".into()];
        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('d'));
        let text = words(&f);
        assert!(
            text.contains("Can't move 'a' to the trash: Trashing is not supported. Delete it for good? y / n"),
            "{text}"
        );
        assert_eq!(flash(press(&mut f, KeyCode::Char('n'))), "kept 'a'");
        assert!(dir.path().join("a").exists());

        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('d'));
        assert_eq!(flash(press(&mut f, KeyCode::Char('y'))), "deleted 'a'");
        assert!(!dir.path().join("a").exists());
        assert_eq!(selected(&f), "b");
    }

    #[test]
    fn a_missing_default_trash_says_how_to_pick_another() {
        assert_eq!(not_installed("my-trash", false), "'my-trash' isn't installed");
        assert_eq!(not_installed("trash", true), "'trash' isn't installed; $TB_TRASH can name another trash command");
    }

    #[test]
    fn folders_go_with_their_contents_and_links_without_their_targets() {
        let (dir, mut f, _rx) = setup();
        f.trash = vec!["/nonexistent/tb-trash".into()];
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), "k").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        std::fs::create_dir_all(dir.path().join("dir/sub")).unwrap();
        std::fs::write(dir.path().join("dir/sub/x"), "x").unwrap();
        f.reload();

        f.refilter(Some("dir"));
        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('d'));
        let text = words(&f);
        assert!(text.contains("'/nonexistent/tb-trash' isn't installed."), "{text}");
        assert!(text.contains("Delete the folder and everything in it for good? y / n"), "{text}");
        press(&mut f, KeyCode::Char('y'));
        assert!(!dir.path().join("dir").exists());

        f.refilter(Some("link"));
        assert_eq!(selected(&f), "link");
        press(&mut f, KeyCode::Char('m'));
        press(&mut f, KeyCode::Char('d'));
        assert!(words(&f).contains("Delete it for good?"), "{}", screen(&f));
        press(&mut f, KeyCode::Char('y'));
        assert!(dir.path().join("link").symlink_metadata().is_err());
        assert!(outside.path().join("keep").exists());
    }

    #[test]
    fn boxes_go_below_the_row_and_stay_in_the_list() {
        let list = Rect::new(0, 1, 30, 20);
        assert_eq!(place(5, 3, (15, 6), list), Rect::new(5, 4, 15, 6));
        // Too far right: moved left to the list's edge.
        assert_eq!(place(25, 3, (15, 6), list), Rect::new(15, 4, 15, 6));
        // No room below: above the row.
        assert_eq!(place(5, 18, (15, 6), list), Rect::new(5, 12, 15, 6));
        // A list narrower than the box: it starts at the list and spills right.
        assert_eq!(place(5, 3, (15, 6), Rect::new(0, 1, 10, 20)), Rect::new(0, 4, 15, 6));
        // Room neither below nor above: the list's top.
        assert_eq!(place(0, 4, (15, 6), Rect::new(0, 1, 30, 8)), Rect::new(0, 1, 15, 6));
    }

    #[test]
    fn wrap_breaks_at_spaces_and_cuts_long_words() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 4), [""]);
    }

    #[test]
    fn the_field_edits_and_shows_its_end() {
        let mut e = LineEdit::new("notes.md");
        let k = |c| KeyEvent::new(c, KeyModifiers::NONE);
        for _ in 0..3 {
            e.key(&k(KeyCode::Left));
        }
        e.key(&k(KeyCode::Char('2')));
        assert_eq!(e.text, "notes2.md");
        e.key(&k(KeyCode::Backspace));
        e.key(&k(KeyCode::Delete));
        assert_eq!(e.text, "notesmd");
        assert_eq!(e.shown(4), "s▏md");
        e.key(&k(KeyCode::Home));
        assert_eq!(e.shown(4), "▏not");
        e.key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!((e.text.as_str(), e.cursor), ("", 0));
    }
}
