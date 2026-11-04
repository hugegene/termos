//! Drawing: terminal cells into a `ratatui` buffer, the chrome around panes, and the bytes sent to clients.

use std::io::Write;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::search::Match;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor};
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

use super::layout::Separator;
use super::pane::TermPane;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
}

fn named(n: NamedColor) -> Color {
    use NamedColor::*;
    let i = n as usize;
    if i < 16 {
        return Color::Indexed(i as u8);
    }
    match n {
        DimBlack => Color::Indexed(0),
        DimRed => Color::Indexed(1),
        DimGreen => Color::Indexed(2),
        DimYellow => Color::Indexed(3),
        DimBlue => Color::Indexed(4),
        DimMagenta => Color::Indexed(5),
        DimCyan => Color::Indexed(6),
        DimWhite => Color::Indexed(7),
        _ => Color::Reset,
    }
}

pub fn color(c: AnsiColor) -> Color {
    match c {
        AnsiColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(i) => Color::Indexed(i),
        AnsiColor::Named(n) => named(n),
    }
}

pub fn cell_style(cell: &Cell) -> Style {
    let mut style = Style::default().fg(color(cell.fg)).bg(color(cell.bg));
    let f = cell.flags;
    let mut m = Modifier::empty();
    if f.contains(Flags::BOLD) {
        m |= Modifier::BOLD;
    }
    if f.contains(Flags::DIM) {
        m |= Modifier::DIM;
    }
    if f.contains(Flags::ITALIC) {
        m |= Modifier::ITALIC;
    }
    if f.intersects(Flags::ALL_UNDERLINES) {
        m |= Modifier::UNDERLINED;
    }
    if f.contains(Flags::INVERSE) {
        m |= Modifier::REVERSED;
    }
    if f.contains(Flags::HIDDEN) {
        m |= Modifier::HIDDEN;
    }
    if f.contains(Flags::STRIKEOUT) {
        m |= Modifier::CROSSED_OUT;
    }
    style.add_modifier = m;
    style
}

/// Extra things to draw over a terminal pane.
#[derive(Default)]
pub struct TermDecor<'a> {
    /// Show the program's cursor (only for the focused pane).
    pub show_cursor: bool,
    /// The current search match, drawn brightest.
    pub search_match: Option<&'a Match>,
    /// Other search matches on screen.
    pub other_matches: &'a [Match],
    /// Show the scroll position over the pane's top-right corner (when no frame shows it).
    pub scroll_overlay: bool,
}

/// Draws a terminal pane into `rect`. Returns the cursor position if it should be shown.
pub fn draw_term(buf: &mut Buffer, rect: Rect, pane: &TermPane, decor: &TermDecor) -> Option<Cursor> {
    if rect.width == 0 || rect.height == 0 {
        return None;
    }
    let term = pane.term.lock();
    let content = term.renderable_content();
    let offset = content.display_offset as i32;
    let selection = content.selection;
    let in_match = |p: Point| decor.search_match.is_some_and(|m| m.contains(&p));
    let in_other = |p: Point| decor.other_matches.iter().any(|m| m.contains(&p));
    for indexed in content.display_iter {
        let line = indexed.point.line.0 + offset;
        let col = indexed.point.column.0;
        if line < 0 || line >= i32::from(rect.height) || col >= usize::from(rect.width) {
            continue;
        }
        let cell = indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let (x, y) = (rect.x + col as u16, rect.y + line as u16);
        let Some(target) = buf.cell_mut((x, y)) else { continue };
        let mut style = cell_style(cell);
        if selection.is_some_and(|s| s.contains(indexed.point)) {
            style.add_modifier.toggle(Modifier::REVERSED);
        }
        if in_match(indexed.point) {
            style = style.fg(Color::Black).bg(Color::Yellow).remove_modifier(Modifier::REVERSED);
        } else if in_other(indexed.point) {
            style = style.fg(Color::Black).bg(Color::Cyan).remove_modifier(Modifier::REVERSED);
        }
        let wide_at_edge = cell.flags.contains(Flags::WIDE_CHAR) && col + 1 >= usize::from(rect.width);
        if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) || wide_at_edge || cell.c == '\0' {
            target.set_symbol(" ");
        } else if let Some(zw) = cell.zerowidth() {
            let mut s = String::from(cell.c);
            s.extend(zw);
            target.set_symbol(&s);
        } else {
            target.set_char(cell.c);
        }
        target.set_style(style);
    }

    // Scrollback position indicator.
    if offset > 0 && decor.scroll_overlay {
        let label = format!(" {}/{} ", offset, term.grid().history_size());
        let x = rect.x + rect.width.saturating_sub(label.len() as u16);
        put_str(buf, x, rect.y, &label, Style::default().fg(Color::Black).bg(Color::Yellow), rect.width);
    }

    if !decor.show_cursor || content.cursor.shape == CursorShape::Hidden {
        return None;
    }
    let line = content.cursor.point.line.0 + offset;
    let col = content.cursor.point.column.0;
    (line >= 0 && line < i32::from(rect.height) && col < usize::from(rect.width)).then(|| Cursor {
        x: rect.x + col as u16,
        y: rect.y + line as u16,
        shape: content.cursor.shape,
    })
}

/// Draws a pane frame: a box with `title` at the left of the top border and `right` at its right end.
pub fn draw_frame(buf: &mut Buffer, rect: Rect, title: &str, right: &str, style: Style) {
    let rect = rect.intersection(buf.area);
    if rect.width < 2 || rect.height < 2 {
        return;
    }
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.right() - 1, rect.bottom() - 1);
    let line = style.remove_modifier(Modifier::BOLD);
    let mut set = |x: u16, y: u16, sym: &str| {
        if let Some(c) = buf.cell_mut((x, y)) {
            c.set_symbol(sym).set_style(line);
        }
    };
    for x in x0 + 1..x1 {
        set(x, y0, "─");
        set(x, y1, "─");
    }
    for y in y0 + 1..y1 {
        set(x0, y, "│");
        set(x1, y, "│");
    }
    set(x0, y0, "┌");
    set(x1, y0, "┐");
    set(x0, y1, "└");
    set(x1, y1, "┘");
    // "┌ title ──── right ┐": both labels sit between the corners with a space either side.
    let room = rect.width.saturating_sub(2);
    let right = format!(" {right} ");
    let right_w = right.chars().count() as u16;
    let right_w = if right.trim().is_empty() || right_w + 8 > room { 0 } else { right_w };
    if right_w > 0 {
        put_str(buf, x1 - right_w, y0, &right, line, right_w);
    }
    if !title.is_empty() {
        let max = room.saturating_sub(right_w + 1);
        if max >= 3 {
            let title = format!(" {title} ");
            let w = put_str(buf, x0 + 1, y0, &title, style, max);
            // A cut-off title ends in "…" instead of mid-word.
            if usize::from(w) < title.chars().count() && w > 1 {
                put_str(buf, x0 + w, y0, "…", style, 1);
            }
        }
    }
}

/// Writes `s` at (x, y), clipped to `max` columns. Returns the width written.
pub fn put_str(buf: &mut Buffer, x: u16, y: u16, s: &str, style: Style, max: u16) -> u16 {
    if y >= buf.area.height || x >= buf.area.width {
        return 0;
    }
    let max = max.min(buf.area.width - x);
    let (end, _) = buf.set_stringn(x, y, s, usize::from(max), style);
    end - x
}

pub fn fill(buf: &mut Buffer, rect: Rect, symbol: &str, style: Style) {
    let rect = rect.intersection(buf.area);
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            if let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(symbol).set_style(style);
            }
        }
    }
}

/// Draws separator lines; parts next to the focused pane are highlighted. T-junctions get joined.
pub fn draw_separators(buf: &mut Buffer, seps: &[Separator], focused: Option<Rect>) {
    let normal = Style::default().fg(Color::DarkGray);
    let active = Style::default().fg(Color::Green);
    let touches = |x: u16, y: u16, vertical: bool| {
        focused.is_some_and(|f| {
            let inside = |x: u16, y: u16| x >= f.x && x < f.x + f.width && y >= f.y && y < f.y + f.height;
            if vertical {
                inside(x.wrapping_sub(1), y) || inside(x + 1, y)
            } else {
                inside(x, y.wrapping_sub(1)) || inside(x, y + 1)
            }
        })
    };
    for s in seps {
        for y in s.rect.top()..s.rect.bottom() {
            for x in s.rect.left()..s.rect.right() {
                let style = if touches(x, y, s.vertical) { active } else { normal };
                if let Some(c) = buf.cell_mut((x, y)) {
                    c.set_symbol(if s.vertical { "│" } else { "─" }).set_style(style);
                }
            }
        }
    }
    // Join a separator's ends to the perpendicular line it meets.
    for s in seps {
        let r = s.rect;
        let ends: [(u16, u16, &str); 2] = if s.vertical {
            [(r.x, r.y.wrapping_sub(1), "┬"), (r.x, r.y + r.height, "┴")]
        } else {
            [(r.x.wrapping_sub(1), r.y, "├"), (r.x + r.width, r.y, "┤")]
        };
        for (x, y, joint) in ends {
            let meets = seps.iter().any(|o| o.vertical != s.vertical && super::layout::contains(o.rect, x, y));
            if meets && let Some(c) = buf.cell_mut((x, y)) {
                c.set_symbol(joint);
            }
        }
    }
}

/// The DECSCUSR sequence for a cursor shape. Block uses the terminal's default shape.
fn cursor_shape_seq(shape: CursorShape) -> &'static [u8] {
    match shape {
        CursorShape::Underline => b"\x1b[4 q",
        CursorShape::Beam => b"\x1b[6 q",
        _ => b"\x1b[0 q",
    }
}

/// Terminal output that turns `prev` into `next` on the client's screen. `clear` redraws everything.
/// `overlay` (images) goes after the text and before the cursor is placed. Returns nothing if the screen and
/// cursor are already right.
pub fn encode_frame(
    prev: &Buffer,
    next: &Buffer,
    prev_cursor: Option<Cursor>,
    cursor: Option<Cursor>,
    clear: bool,
    overlay: &[u8],
) -> Vec<u8> {
    let updates = prev.diff(next);
    if !clear && updates.is_empty() && prev_cursor == cursor && overlay.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<u8> = b"\x1b[?2026h".to_vec();
    if clear {
        out.extend(b"\x1b[0m\x1b[H\x1b[2J");
    }
    {
        let mut be = CrosstermBackend::new(&mut out);
        let _ = be.hide_cursor();
        let _ = be.draw(updates.into_iter());
        let _ = be.write_all(overlay);
        if let Some(c) = cursor {
            if prev_cursor.map(|p| p.shape) != Some(c.shape) || clear {
                let _ = be.write_all(cursor_shape_seq(c.shape));
            }
            let _ = be.set_cursor_position(Position { x: c.x, y: c.y });
            let _ = be.show_cursor();
        }
        let _ = Backend::flush(&mut be);
    }
    out.extend(b"\x1b[?2026l");
    out
}
