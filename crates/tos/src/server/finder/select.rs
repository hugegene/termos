//! Text selected in the finder's preview: with the mouse, from one character to another, or with `v`/`V`, whole
//! lines. Plan: docs/plans/2026-11-21-preview-select-copy.md.
//!
//! Points are (line, column): a line of the preview's rendered lines, and a display column of its text, counted
//! from the first column after the gutter (a code file's line numbers), which is never selected. Column
//! `usize::MAX` is the end of the line.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

/// The most a selection copies: OSC 52 with more is often dropped.
pub const MAX_COPY: usize = 1 << 20;

pub type Point = (usize, usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Making {
    /// The mouse button is down: drags move the end.
    Mouse,
    /// `v` or `V`: keys move the end.
    Keys,
    /// Made with the mouse and copied; it stays until Esc, a click or another file.
    Done,
}

#[derive(Debug, Clone)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
    /// Whole lines, rather than from one character to another.
    pub lines: bool,
    pub making: Making,
    /// The mouse has moved since the press. Until it does, nothing is selected (a click is just a click).
    pub moved: bool,
}

impl Selection {
    /// The mouse button went down at `at`.
    pub fn press(at: Point) -> Self {
        Selection { anchor: at, head: at, lines: false, making: Making::Mouse, moved: false }
    }

    /// `v` or `V`: whole lines, starting with `line`.
    pub fn keys(line: usize) -> Self {
        Selection { anchor: (line, 0), head: (line, 0), lines: true, making: Making::Keys, moved: true }
    }

    /// The mouse moved to `at` with the button down.
    pub fn drag(&mut self, at: Point) {
        self.head = at;
        self.moved |= at != self.anchor;
    }

    /// Something is selected (not just a press that hasn't moved).
    pub fn shown(&self) -> bool {
        self.moved
    }

    /// Lines `n` further down (or up, if negative), within `count` lines.
    pub fn extend(&mut self, n: isize, count: usize) {
        self.head.0 = self.head.0.saturating_add_signed(n).min(count.saturating_sub(1));
    }

    /// Where it starts and ends, in order.
    fn ends(&self) -> (Point, Point) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    /// The columns selected on `line`, inclusive (the end may be `usize::MAX`), or none.
    fn columns(&self, line: usize) -> Option<(usize, usize)> {
        let (start, end) = self.ends();
        if !self.moved || line < start.0 || line > end.0 {
            return None;
        }
        if self.lines {
            return Some((0, usize::MAX));
        }
        let from = if line == start.0 { start.1 } else { 0 };
        let to = if line == end.0 { end.1 } else { usize::MAX };
        Some((from, to))
    }

    /// The selected text: a newline between lines and none at the end, trailing spaces trimmed. Lines in the
    /// middle are whole, even where they run past the pane's edge.
    pub fn text(&self, lines: &[Line], gutter: usize) -> String {
        let (start, end) = self.ends();
        let mut out = Vec::new();
        for (i, line) in lines.iter().enumerate().skip(start.0).take(end.0 + 1 - start.0) {
            let Some((from, to)) = self.columns(i) else { continue };
            let text = plain(line);
            out.push(cut(&text, gutter.saturating_add(from), gutter.saturating_add(to)).trim_end().to_owned());
        }
        out.join("\n")
    }

    /// How much is selected, for the status bar: "1 line", "3 lines" or "12 characters".
    pub fn describe(&self, text: &str) -> String {
        let (start, end) = self.ends();
        match end.0 - start.0 + 1 {
            1 if !self.lines => format!("{} characters", text.chars().count()),
            1 => "1 line".to_owned(),
            n => format!("{n} lines"),
        }
    }

    /// Reverses the selected cells of the lines on screen: `lines[top..]` drawn from `body.y`, each starting
    /// at column `x` (after the padding). An empty selected line shows one reversed cell.
    pub fn draw(&self, buf: &mut Buffer, lines: &[Line], top: usize, gutter: usize, x: u16, body: Rect) {
        let reversed = Style::default().add_modifier(Modifier::REVERSED);
        for row in 0..body.height {
            let line = top + usize::from(row);
            let Some(l) = lines.get(line) else { break };
            let Some((from, to)) = self.columns(line) else { continue };
            // An empty line counts as one column wide, so it shows when it's selected.
            let width = l.width().saturating_sub(gutter).max(1);
            if from >= width {
                continue;
            }
            let last = to.min(width - 1);
            let left = usize::from(x) + gutter + from;
            let right = (usize::from(x) + gutter + last + 1).min(usize::from(body.right()));
            if left >= right {
                continue;
            }
            buf.set_style(Rect::new(left as u16, body.y + row, (right - left) as u16, 1), reversed);
        }
    }
}

/// A line's text without its styles.
fn plain(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

/// The characters of `text` that cover display columns `from..=to`.
fn cut(text: &str, from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if col > to {
            break;
        }
        if col + w.max(1) > from {
            out.push(c);
        }
        col += w;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Span;

    fn lines(text: &[&str]) -> Vec<Line<'static>> {
        text.iter().map(|t| Line::from(t.to_string())).collect()
    }

    fn select(anchor: Point, head: Point) -> Selection {
        let mut s = Selection::press(anchor);
        s.drag(head);
        s
    }

    #[test]
    fn one_line_from_one_character_to_another() {
        let l = lines(&["one two three"]);
        assert_eq!(select((0, 4), (0, 12)).text(&l, 0), "two three");
        // Backwards is the same.
        assert_eq!(select((0, 12), (0, 4)).text(&l, 0), "two three");
        assert_eq!(select((0, 4), (0, 12)).describe("two three"), "9 characters");
    }

    #[test]
    fn middle_lines_are_whole_and_the_end_of_the_line_is_max() {
        let l = lines(&["first line", "a very long middle line that runs past the pane", "last   "]);
        let s = select((0, 6), (2, usize::MAX));
        assert_eq!(s.text(&l, 0), "line\na very long middle line that runs past the pane\nlast");
        assert_eq!(s.describe(""), "3 lines");
    }

    #[test]
    fn a_code_files_line_numbers_are_left_out() {
        let l = vec![
            Line::from(vec![Span::raw(" 9 │ "), Span::raw("fn main() {")]),
            Line::from(vec![Span::raw("10 │ "), Span::raw("    run();")]),
        ];
        assert_eq!(Selection::keys(0).text(&l, 5), "fn main() {");
        let mut s = Selection::keys(0);
        s.extend(5, l.len());
        assert_eq!(s.head.0, 1);
        assert_eq!(s.text(&l, 5), "fn main() {\n    run();");
        // A press in the gutter starts at the text.
        assert_eq!(select((1, 0), (1, 6)).text(&l, 5), "    run");
    }

    #[test]
    fn wide_characters_are_whole() {
        let l = lines(&["日本語 text"]);
        // Column 3 is the second half of 本.
        assert_eq!(select((0, 3), (0, 4)).text(&l, 0), "本語");
    }

    #[test]
    fn a_press_alone_selects_nothing() {
        let l = lines(&["abc"]);
        let s = Selection::press((0, 1));
        assert!(!s.shown());
        assert_eq!(s.text(&l, 0), "");
    }

    #[test]
    fn the_highlight_covers_the_selected_text_on_screen() {
        let l = lines(&["zero", "one two", "three"]);
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        // Line 0 is scrolled off; lines 1 and 2 are on rows 0 and 1, text from column 1.
        select((1, 4), (2, 1)).draw(&mut buf, &l, 1, 0, 1, area);
        let rev = |x, y| buf[(x, y)].modifier.contains(Modifier::REVERSED);
        assert!(!rev(4, 0) && rev(5, 0) && rev(7, 0) && !rev(8, 0));
        assert!(rev(1, 1) && rev(2, 1) && !rev(3, 1));
    }
}
