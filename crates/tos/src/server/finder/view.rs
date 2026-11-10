//! Turns preview data into styled lines for the terminal: Markdown, code, CSV tables, folders and images.

use finder_core::{Entry, Kind, Preview, human_size};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use image::RgbImage;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, LinkType, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::highlight::Highlighter;

/// Highlight at most this many lines per file; the rest are shown plain.
const MAX_HIGHLIGHT_LINES: usize = 5000;

pub enum Body {
    Lines(Vec<Line<'static>>),
    /// A picture, with an id that's new for every preview (so encodings can be cached by it).
    Image(Arc<RgbImage>, u64),
}

static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);

/// A new picture id.
pub fn next_image_id() -> u64 {
    NEXT_IMAGE.fetch_add(1, Ordering::Relaxed)
}

pub struct View {
    /// Short description for the preview header, for example "markdown · 1.2 KB".
    pub info: String,
    pub body: Body,
    /// Where the links are in `body`'s lines.
    pub links: Vec<Link>,
}

/// A link, or the part of one on one line: columns `start..end` of rendered line `line`, and where it points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub url: String,
}

impl View {
    fn lines(info: String, lines: Vec<Line<'static>>) -> View {
        View { info, body: Body::Lines(lines), links: Vec::new() }
    }
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Builds the view for `preview` at `width` columns.
pub fn build(preview: Preview, kind: &Kind, size: Option<u64>, width: u16) -> View {
    let width = usize::from(width.max(10));
    let size_text = size.map(human_size).unwrap_or_default();
    let info = |extra: &str| {
        [kind.label(), extra, &size_text].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ")
    };
    match preview {
        Preview::Dir(entries) => View::lines(format!("folder · {} items", entries.len()), dir_lines(&entries)),
        Preview::Markdown(src) | Preview::Html(src) => {
            let (lines, links) = markdown(&src, width);
            View { info: info(""), body: Body::Lines(lines), links }
        }
        Preview::Csv { rows, truncated } => {
            let mut lines = table(&rows, width, true);
            if truncated {
                lines.push(Line::styled(format!("… only the first {} rows are shown", rows.len()), dim()));
            }
            View::lines(info(&format!("{} rows", rows.len())), lines)
        }
        Preview::Image { thumbnail, width: w, height: h } => {
            let body = Body::Image(Arc::new(thumbnail), next_image_id());
            View { info: info(&format!("{w}×{h}")), body, links: Vec::new() }
        }
        Preview::Pdf { info: doc, pages, truncated } => {
            let (mut lines, _) = super::pdf::text_lines(1, &pages, width);
            if truncated {
                lines.push(Line::styled("… the preview stops here (file too large)", dim()));
            }
            View::lines(info(&format!("{} pages", doc.pages)), lines)
        }
        Preview::Text { text, syntax, truncated } => {
            let mut lines = code_lines(&text, syntax.as_deref());
            if truncated {
                lines.push(Line::styled("… the preview stops here (file too large)", dim()));
            }
            View::lines(info(""), lines)
        }
        Preview::Binary { size } => {
            View::lines(info(""), vec![Line::styled(format!("binary file, {}", human_size(size)), dim())])
        }
        Preview::Error(e) => {
            View::lines(info(""), vec![Line::styled(format!("can't preview: {e}"), Style::default().fg(Color::Red))])
        }
    }
}

fn dir_lines(entries: &[Entry]) -> Vec<Line<'static>> {
    if entries.is_empty() {
        return vec![Line::styled("(empty folder)", dim())];
    }
    entries
        .iter()
        .map(|e| {
            if e.is_dir {
                Line::from(vec![Span::styled(
                    format!("{}/", e.name),
                    Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD),
                )])
            } else {
                Line::from(vec![Span::raw(e.name.clone()), Span::styled(format!("  {}", human_size(e.size)), dim())])
            }
        })
        .collect()
}

/// Source code or plain text with line numbers and syntax colours.
fn code_lines(text: &str, syntax: Option<&str>) -> Vec<Line<'static>> {
    let highlighted = Highlighter::get().highlight(text, syntax, MAX_HIGHLIGHT_LINES);
    let digits = highlighted.len().max(1).to_string().len();
    highlighted
        .into_iter()
        .enumerate()
        .map(|(i, pieces)| {
            let mut spans = vec![Span::styled(format!("{:>digits$} │ ", i + 1), dim())];
            spans.extend(pieces.into_iter().map(|(s, t)| Span::styled(t, s)));
            Line::from(spans)
        })
        .collect()
}

fn truncate(s: &str, width: usize) -> String {
    if s.width() <= width {
        return format!("{s}{}", " ".repeat(width - s.width()));
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw + 1 > width {
            break;
        }
        out.push(c);
        w += cw;
    }
    let trimmed = out.trim_end().len();
    w -= out.len() - trimmed;
    out.truncate(trimmed);
    out.push('…');
    w += 1;
    out.push_str(&" ".repeat(width.saturating_sub(w)));
    out
}

/// A cell's text on one line.
fn clean(s: &str) -> String {
    s.replace(['\n', '\r', '\t'], " ")
}

/// Column gap in tables: " │ ".
const TABLE_SEP: usize = 3;

/// A table: columns sized to their content, shrunk to fit `width`, first row bold if `header`.
pub fn table(rows: &[Vec<String>], width: usize, header: bool) -> Vec<Line<'static>> {
    draw_table(rows, &column_widths(rows, width), header)
}

/// Table columns sized to their content (up to 40 columns each), shrunk to fit `width`.
fn column_widths(rows: &[Vec<String>], width: usize) -> Vec<usize> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return Vec::new();
    }
    let mut widths = vec![1usize; cols];
    for row in rows.iter().take(500) {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(clean(cell).width().min(40));
        }
    }
    while widths.iter().sum::<usize>() + TABLE_SEP * (cols - 1) > width {
        let (i, w) = widths.iter().enumerate().max_by_key(|(_, w)| **w).map(|(i, w)| (i, *w)).unwrap();
        if w <= 3 {
            break;
        }
        widths[i] -= 1;
    }
    widths
}

/// Draws a table with columns `widths` wide; a rule follows the first row if it's a `header`.
fn draw_table(rows: &[Vec<String>], widths: &[usize], header: bool) -> Vec<Line<'static>> {
    if widths.is_empty() {
        return vec![Line::styled("(empty)", dim())];
    }
    let mut lines = Vec::new();
    for (r, row) in rows.iter().enumerate() {
        let bold = header && r == 0;
        let mut spans = Vec::new();
        for (c, w) in widths.iter().enumerate() {
            if c > 0 {
                spans.push(Span::styled(" │ ", dim()));
            }
            let text = truncate(&clean(row.get(c).map(String::as_str).unwrap_or("")), *w);
            let style = if bold { Style::default().add_modifier(Modifier::BOLD) } else { Style::default() };
            spans.push(Span::styled(text, style));
        }
        lines.push(Line::from(spans));
        if bold {
            let rule = widths.iter().map(|w| "─".repeat(*w)).collect::<Vec<_>>().join("─┼─");
            lines.push(Line::styled(rule, dim()));
        }
    }
    lines
}

/// Markdown rendered for the terminal: wrapped paragraphs, headings, lists, quotes, code and tables. Also says
/// where the links landed.
pub fn markdown(src: &str, width: usize) -> (Vec<Line<'static>>, Vec<Link>) {
    let mut md = Md::new(width);
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES;
    for event in Parser::new_ext(src, options) {
        md.event(event);
    }
    md.flush();
    while md.lines.last().is_some_and(|l| l.width() == 0) {
        md.lines.pop();
    }
    (md.lines, md.links)
}

/// Where a link or image points; email autolinks get their `mailto:`.
fn link_url(link_type: LinkType, dest: &CowStr) -> String {
    if link_type == LinkType::Email { format!("mailto:{dest}") } else { dest.to_string() }
}

/// A link in a table: its row and column, and its byte range in the cell's (trimmed) text.
struct CellLink {
    row: usize,
    col: usize,
    start: usize,
    end: usize,
    url: String,
}

struct Indent {
    first: String,
    rest: String,
    used: bool,
    style: Style,
}

struct Md {
    width: usize,
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    cur_w: usize,
    prefix_w: usize,
    started: bool,
    styles: Vec<Style>,
    indents: Vec<Indent>,
    lists: Vec<Option<u64>>,
    code: Option<(Option<String>, String)>,
    table: Option<Vec<Vec<String>>>,
    cell: Option<String>,
    row: Vec<String>,
    /// Links (and images) being placed, outermost first. Text placed meanwhile belongs to the outermost.
    link_stack: Vec<String>,
    links: Vec<Link>,
    /// Links open in the table cell being read: where they start in the cell, and where they point.
    cell_open: Vec<(usize, String)>,
    /// Links in the table cell being read: byte range in the cell, and where they point.
    cell_links: Vec<(usize, usize, String)>,
    table_links: Vec<CellLink>,
}

impl Md {
    fn new(width: usize) -> Self {
        Md {
            width,
            lines: Vec::new(),
            cur: Vec::new(),
            cur_w: 0,
            prefix_w: 0,
            started: false,
            styles: Vec::new(),
            indents: Vec::new(),
            lists: Vec::new(),
            code: None,
            table: None,
            cell: None,
            row: Vec::new(),
            link_stack: Vec::new(),
            links: Vec::new(),
            cell_open: Vec::new(),
            cell_links: Vec::new(),
            table_links: Vec::new(),
        }
    }

    fn style(&self) -> Style {
        self.styles.iter().fold(Style::default(), |acc, s| acc.patch(*s))
    }

    fn push_style(&mut self, s: Style) {
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        self.styles.pop();
    }

    fn start_line(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.cur_w = 0;
        for ind in &mut self.indents {
            let text = if ind.used { ind.rest.clone() } else { ind.first.clone() };
            ind.used = true;
            self.cur_w += text.width();
            self.cur.push(Span::styled(text, ind.style));
        }
        self.prefix_w = self.cur_w;
    }

    fn flush(&mut self) {
        if self.started {
            self.lines.push(Line::from(std::mem::take(&mut self.cur)));
            self.started = false;
            self.cur_w = 0;
        }
    }

    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| l.width() > 0) {
            self.lines.push(Line::default());
        }
    }

    fn push_word(&mut self, word: &str, style: Style) {
        self.start_line();
        let mut word = word.to_owned();
        let avail = self.width.saturating_sub(self.prefix_w).max(1);
        if self.cur_w + word.trim_end().width() > self.width && self.cur_w > self.prefix_w {
            self.flush();
            self.start_line();
            word = word.trim_start().to_owned();
        }
        // A single word longer than the line is split at the edge.
        while word.width() > avail && self.cur_w == self.prefix_w {
            let mut head = String::new();
            let mut w = 0;
            for c in word.chars() {
                let cw = c.width().unwrap_or(0);
                if w + cw > avail {
                    break;
                }
                head.push(c);
                w += cw;
            }
            word = word[head.len()..].to_owned();
            self.put(head, style);
            self.flush();
            self.start_line();
        }
        if !word.is_empty() {
            self.put(word, style);
        }
    }

    /// Adds `text` to the line being built, noting it as part of the link being placed, if any.
    fn put(&mut self, text: String, style: Style) {
        let width = text.width();
        if let Some(url) = self.link_stack.first()
            && width > 0
        {
            let (line, start) = (self.lines.len(), self.cur_w);
            match self.links.last_mut() {
                Some(l) if l.line == line && l.end == start && l.url == *url => l.end += width,
                _ => self.links.push(Link { line, start, end: start + width, url: url.clone() }),
            }
        }
        self.cur_w += width;
        self.cur.push(Span::styled(text, style));
    }

    fn push_text(&mut self, text: &str, style: Style) {
        for piece in text.split_inclusive(' ') {
            self.push_word(piece, style);
        }
    }

    fn heading_style(level: HeadingLevel) -> Style {
        let bold = Style::default().add_modifier(Modifier::BOLD);
        match level {
            HeadingLevel::H1 => bold.fg(Color::Magenta).add_modifier(Modifier::UNDERLINED),
            HeadingLevel::H2 => bold.fg(Color::Cyan),
            HeadingLevel::H3 => bold.fg(Color::Blue),
            _ => bold,
        }
    }

    fn event(&mut self, event: Event) {
        if let Some(cell) = &mut self.cell {
            match event {
                Event::Text(t) | Event::Code(t) | Event::Html(t) | Event::InlineHtml(t) => cell.push_str(&t),
                Event::Start(Tag::Link { link_type, dest_url, .. } | Tag::Image { link_type, dest_url, .. }) => {
                    self.cell_open.push((cell.len(), link_url(link_type, &dest_url)));
                }
                Event::End(TagEnd::Link | TagEnd::Image) => {
                    if let Some((start, url)) = self.cell_open.pop()
                        && self.cell_open.is_empty()
                    {
                        self.cell_links.push((start, cell.len(), url));
                    }
                }
                Event::End(TagEnd::TableCell) => self.end(TagEnd::TableCell),
                _ => {}
            }
            return;
        }
        if let Some((_, buf)) = &mut self.code {
            match event {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => self.end_code_block(),
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let style = self.style();
                self.push_text(&t, style);
            }
            Event::Code(t) => {
                let style = self.style().fg(Color::Yellow);
                self.push_text(&t, style);
            }
            Event::Html(t) | Event::InlineHtml(t) => {
                let style = self.style().patch(dim());
                self.push_text(t.trim_end_matches('\n'), style);
            }
            Event::SoftBreak => {
                let style = self.style();
                self.push_word(" ", style);
            }
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.lines.push(Line::styled("─".repeat(self.width.min(80)), dim()));
                self.blank();
            }
            Event::TaskListMarker(done) => {
                let style = self.style().fg(Color::Yellow);
                self.push_word(if done { "[x] " } else { "[ ] " }, style);
            }
            Event::FootnoteReference(label) => {
                let style = self.style().patch(dim());
                self.push_word(&format!("[^{label}]"), style);
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.blank();
                self.push_style(Self::heading_style(level));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.indents.push(Indent { first: "│ ".into(), rest: "│ ".into(), used: false, style: dim() });
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split([' ', ',']).next().filter(|l| !l.is_empty()).map(str::to_owned)
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len().max(1);
                let bullet = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let b = format!("{n}. ");
                        *n += 1;
                        b
                    }
                    _ => ["• ", "◦ ", "▪ "][(depth - 1) % 3].to_owned(),
                };
                let rest = " ".repeat(bullet.width());
                self.indents.push(Indent {
                    first: bullet,
                    rest,
                    used: false,
                    style: Style::default().fg(Color::Yellow),
                });
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { link_type, dest_url, .. } => {
                self.link_stack.push(link_url(link_type, &dest_url));
                self.push_style(Style::default().fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { link_type, dest_url, .. } => {
                self.link_stack.push(link_url(link_type, &dest_url));
                let style = self.style().patch(dim());
                self.push_word("[image: ", style);
                self.push_style(dim());
            }
            Tag::Table(_) => {
                self.flush();
                self.table = Some(Vec::new());
            }
            Tag::TableHead | Tag::TableRow => self.row.clear(),
            Tag::TableCell => self.cell = Some(String::new()),
            Tag::FootnoteDefinition(label) => {
                self.flush();
                let style = dim();
                self.push_word(&format!("[^{label}]: "), style);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Heading(_) => {
                self.pop_style();
                self.blank();
            }
            TagEnd::Paragraph => {
                self.flush();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.indents.pop();
                self.blank();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Item => {
                self.flush();
                self.indents.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                self.link_stack.pop();
            }
            TagEnd::Image => {
                self.pop_style();
                let style = self.style().patch(dim());
                self.push_word("]", style);
                self.link_stack.pop();
            }
            TagEnd::TableCell => {
                let cell = self.cell.take().unwrap_or_default();
                let text = cell.trim();
                let lead = cell.len() - cell.trim_start().len();
                // The row is added to the table when it ends.
                let (row, col) = (self.table.as_ref().map_or(0, Vec::len), self.row.len());
                for (start, end, url) in std::mem::take(&mut self.cell_links) {
                    let clip = |i: usize| i.saturating_sub(lead).min(text.len());
                    let (start, end) = (clip(start), clip(end));
                    if start < end {
                        self.table_links.push(CellLink { row, col, start, end, url });
                    }
                }
                self.cell_open.clear();
                self.row.push(text.to_owned());
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                let row = std::mem::take(&mut self.row);
                if let Some(t) = &mut self.table {
                    t.push(row);
                }
            }
            TagEnd::Table => {
                let rows = self.table.take().unwrap_or_default();
                let avail = self.width.saturating_sub(self.indent_width());
                let widths = column_widths(&rows, avail);
                let first = self.lines.len();
                // Where each table line starts, after the indents of quotes and lists.
                let mut starts = Vec::new();
                for line in draw_table(&rows, &widths, true) {
                    self.start_line();
                    starts.push(self.cur_w);
                    self.cur.extend(line.spans);
                    self.flush();
                }
                for l in std::mem::take(&mut self.table_links) {
                    // A rule follows the header row.
                    let index = l.row + usize::from(l.row > 0);
                    let (Some(x), Some(cell), Some(w)) =
                        (starts.get(index), rows.get(l.row).and_then(|r| r.get(l.col)), widths.get(l.col))
                    else {
                        continue;
                    };
                    let x = x + widths[..l.col].iter().sum::<usize>() + TABLE_SEP * l.col;
                    // A cut cell ends in "…" at its last column.
                    let start = clean(&cell[..l.start]).width().min(*w);
                    let end = clean(&cell[..l.end]).width().min(*w);
                    if start < end {
                        self.links.push(Link { line: first + index, start: x + start, end: x + end, url: l.url });
                    }
                }
                self.blank();
            }
            TagEnd::FootnoteDefinition => self.flush(),
            _ => {}
        }
    }

    fn indent_width(&self) -> usize {
        self.indents.iter().map(|i| i.rest.width()).sum()
    }

    fn end_code_block(&mut self) {
        let Some((lang, text)) = self.code.take() else { return };
        for pieces in Highlighter::get().highlight(&text, lang.as_deref(), MAX_HIGHLIGHT_LINES) {
            self.start_line();
            self.cur.push(Span::raw("  "));
            self.cur.extend(pieces.into_iter().map(|(s, t)| Span::styled(t, s)));
            self.flush();
        }
        if self.lists.is_empty() {
            self.blank();
        }
    }
}

/// Colours for an image drawn with half-block characters: each cell is (top pixel, bottom pixel).
/// The image keeps its proportions and fits inside `cols` × `rows` cells.
pub fn image_cells(img: &RgbImage, cols: u16, rows: u16) -> Vec<Vec<(Color, Color)>> {
    let (iw, ih) = (img.width().max(1) as f32, img.height().max(1) as f32);
    let scale = (f32::from(cols) / iw).min(f32::from(rows) * 2.0 / ih);
    let w = ((iw * scale) as u32).max(1);
    let h = ((ih * scale) as u32).max(2);
    let small = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
    let px = |x: u32, y: u32| {
        if y < small.height() {
            let p = small.get_pixel(x, y);
            Color::Rgb(p[0], p[1], p[2])
        } else {
            Color::Reset
        }
    };
    (0..h.div_ceil(2)).map(|cy| (0..w).map(|x| (px(x, cy * 2), px(x, cy * 2 + 1))).collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>().trim_end().to_owned())
            .collect()
    }

    #[test]
    fn markdown_wraps_and_formats() {
        let md =
            "# Title\n\nOne two three four five six.\n\n- alpha\n- beta\n  - nested\n\n> quoted\n\n```\ncode\n```\n";
        let out = text(&markdown(md, 16).0);
        assert_eq!(
            out,
            [
                "Title",
                "",
                "One two three",
                "four five six.",
                "",
                "• alpha",
                "• beta",
                "  ◦ nested",
                "",
                "│ quoted",
                "",
                "  code",
            ]
        );
    }

    #[test]
    fn markdown_tables() {
        let md = "| a | bb |\n|---|---|\n| 1 | 2 |\n";
        let out = text(&markdown(md, 40).0);
        assert_eq!(out, ["a │ bb", "──┼───", "1 │ 2"]);
    }

    /// Each link as (line, the text under it, url).
    fn links(md: &str, width: usize) -> Vec<(usize, String, String)> {
        let (lines, links) = markdown(md, width);
        let text = text(&lines);
        links
            .into_iter()
            .map(|l| {
                let under: String = text[l.line].chars().skip(l.start).take(l.end - l.start).collect();
                (l.line, under, l.url)
            })
            .collect()
    }

    #[test]
    fn links_are_where_they_are_drawn() {
        let found = links("# T\n\nSee [the bee](b.md) and <https://x.org>.\n\n- [**nested** one](n.md)\n", 80);
        assert_eq!(
            found,
            [
                (2, "the bee".into(), "b.md".into()),
                (2, "https://x.org".into(), "https://x.org".into()),
                (4, "nested one".into(), "n.md".into()),
            ]
        );
        assert_eq!(links("<a@b.c>", 80), [(0, "a@b.c".into(), "mailto:a@b.c".into())]);
    }

    #[test]
    fn wrapped_links_have_a_piece_per_line() {
        let (lines, _) = markdown("one two [three four five](x.md)", 12);
        assert_eq!(text(&lines), ["one two", "three four", "five"]);
        let found = links("one two [three four five](x.md)", 12);
        assert_eq!(found, [(1, "three four".into(), "x.md".into()), (2, "five".into(), "x.md".into())]);
    }

    #[test]
    fn images_and_badges() {
        let found = links("![a cat](cat.png) [![badge](b.svg)](https://ci)", 80);
        assert_eq!(
            found,
            [(0, "[image: a cat]".into(), "cat.png".into()), (0, "[image: badge]".into(), "https://ci".into())]
        );
    }

    #[test]
    fn links_in_tables() {
        let md =
            "> | cmd | about |\n> |---|---|\n> | [`tos ls`](ls.md) | lists |\n> | x [a long name](long.md) | y |\n";
        let found = links(md, 18);
        // In a quote, after "│ ". The second link is cut with the cell, at its "…".
        assert_eq!(text(&markdown(md, 18).0)[3], "│ x a lon… │ y");
        assert_eq!(found, [(2, "tos ls".into(), "ls.md".into()), (3, "a lon…".into(), "long.md".into())]);
    }

    #[test]
    fn long_words_are_split() {
        let out = text(&markdown("abcdefghijklmnop", 10).0);
        assert_eq!(out, ["abcdefghij", "klmnop"]);
    }

    #[test]
    fn tables_shrink_to_fit() {
        let rows = vec![vec!["name".to_owned(), "a very long description".to_owned()]];
        let out = text(&table(&rows, 20, false));
        assert_eq!(out, ["name │ a very long…"]);
    }

    #[test]
    fn images_keep_their_shape() {
        let img = RgbImage::from_pixel(100, 50, image::Rgb([255, 0, 0]));
        let cells = image_cells(&img, 40, 40);
        assert_eq!(cells[0].len(), 40);
        assert_eq!(cells.len(), 10);
    }
}
