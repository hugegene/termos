//! The finder's PDF preview ([plan](../../../../../docs/plans/2026-11-05-pdf-preview.md)).
//!
//! Text is the default: `pdftotext` output, loaded 100 pages at a time as you scroll. `t` switches to the page
//! view: the page rendered by `pdftoppm` at the pane's width × zoom, scrolled by cutting the visible part out
//! of it, so scrolling never waits for poppler.

use std::path::PathBuf;
use std::sync::Arc;

use finder_core::pdf::PdfInfo;
use image::RgbImage;
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// How big the page is drawn in the page view.
#[derive(serde::Serialize, serde::Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Zoom {
    /// The whole page in the pane.
    FitPage,
    /// As wide as the pane.
    #[default]
    FitWidth,
    /// This many percent of the pane's width.
    Percent(u16),
}

const STEPS: [Zoom; 6] =
    [Zoom::FitPage, Zoom::FitWidth, Zoom::Percent(150), Zoom::Percent(200), Zoom::Percent(300), Zoom::Percent(400)];

impl Zoom {
    pub fn zoom_in(self) -> Zoom {
        self.step(1)
    }

    pub fn zoom_out(self) -> Zoom {
        self.step(-1)
    }

    fn step(self, by: isize) -> Zoom {
        let i = STEPS.iter().position(|s| *s == self).unwrap_or(1) as isize;
        STEPS[(i + by).clamp(0, STEPS.len() as isize - 1) as usize]
    }

    pub fn label(self) -> String {
        match self {
            Zoom::FitPage => "fit page".into(),
            Zoom::FitWidth => "fit width".into(),
            Zoom::Percent(p) => format!("{p}%"),
        }
    }

    /// The width to render a page of this `aspect` (width ÷ height) at, for a `viewport` in pixels. The
    /// worker then holds it under the pixel cap.
    pub fn width(self, viewport: (u32, u32), aspect: f64) -> u32 {
        let (vw, vh) = viewport;
        match self {
            Zoom::FitPage => vw.min((f64::from(vh) * aspect) as u32),
            Zoom::FitWidth => vw,
            Zoom::Percent(p) => vw * u32::from(p) / 100,
        }
    }
}

/// What the finder asks its worker for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PdfJob {
    /// The text of pages `first` to `last`.
    Text { first: u32, last: u32 },
    /// Page `page`, rendered for `zoom` in a viewport of this many pixels.
    Page { page: u32, zoom: Zoom, viewport: (u32, u32) },
}

/// What the worker sends back for a job.
pub enum PdfResult {
    Text {
        pages: Vec<String>,
        truncated: bool,
    },
    Page {
        image: Arc<RgbImage>,
    },
    /// The page is slow, and is being drawn again without anti-aliasing.
    Slow,
    Error(String),
}

/// A rendered page, and what it was rendered for.
pub struct PageImage {
    pub page: u32,
    pub zoom: Zoom,
    pub viewport: (u32, u32),
    pub img: Arc<RgbImage>,
    /// A picture id that's new for each page image (encodings are cached by it).
    pub id: u64,
}

/// How a chunk of text joined what was loaded.
#[derive(Debug, PartialEq, Eq)]
pub enum Merge {
    Appended,
    /// Added above the loaded text: this many lines came in above the old first line.
    Prepended(usize),
    /// Not next to what was loaded, so it replaced it.
    Replaced,
}

/// One PDF being previewed.
pub struct PdfView {
    pub path: PathBuf,
    pub info: Option<Arc<PdfInfo>>,
    pub error: Option<String>,
    /// The first loaded page of text; `texts[0]` is its text.
    first: u32,
    texts: Vec<String>,
    /// The loaded text as lines, wrapped at `width`.
    pub lines: Vec<Line<'static>>,
    /// The line each loaded page starts on.
    starts: Vec<usize>,
    width: usize,
    /// The first chunk of text was empty (scanned pages), so the page view is shown.
    pub no_text: bool,
    /// The page shown in the page view (1-based), and the view's top-left corner on it, in pixels. `top` can
    /// be `u32::MAX` for "the bottom", until the page's height is known.
    pub page: u32,
    pub top: u32,
    pub left: u32,
    pub image: Option<PageImage>,
    /// The page being rendered was too slow and is being drawn without anti-aliasing.
    pub slow: bool,
    /// The last job asked for, so it isn't asked for twice.
    pub sent: Option<PdfJob>,
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

impl PdfView {
    pub fn new(path: PathBuf, width: usize) -> Self {
        PdfView {
            path,
            info: None,
            error: None,
            first: 1,
            texts: Vec::new(),
            lines: Vec::new(),
            starts: Vec::new(),
            width,
            no_text: false,
            page: 1,
            top: 0,
            left: 0,
            image: None,
            slow: false,
            sent: None,
        }
    }

    pub fn pages(&self) -> u32 {
        self.info.as_ref().map_or(0, |i| i.pages)
    }

    fn aspect(&self, page: u32) -> f64 {
        self.info.as_ref().map_or(595.0 / 842.0, |i| i.aspect(page))
    }

    /// What's missing for the current view, if anything. `scroll` and `rows` are the text view's position and
    /// height; `chunk` is how many pages of text to ask for at a time.
    pub fn wants(
        &self,
        pages_view: bool,
        zoom: Zoom,
        viewport: (u32, u32),
        scroll: usize,
        rows: usize,
        chunk: u32,
    ) -> Option<PdfJob> {
        if self.error.is_some() {
            return None;
        }
        if pages_view || self.no_text {
            if viewport.0 < 16 || viewport.1 < 16 {
                return None;
            }
            let have =
                self.image.as_ref().is_some_and(|i| i.page == self.page && i.zoom == zoom && i.viewport == viewport);
            return (!have).then_some(PdfJob::Page { page: self.page, zoom, viewport });
        }
        if self.texts.is_empty() {
            return Some(PdfJob::Text { first: self.first, last: self.first + chunk - 1 });
        }
        let next = self.first + self.texts.len() as u32;
        if next <= self.pages() && scroll + rows * 2 >= self.lines.len() {
            return Some(PdfJob::Text { first: next, last: next + chunk - 1 });
        }
        if self.first > 1 && scroll < rows {
            return Some(PdfJob::Text { first: self.first.saturating_sub(chunk).max(1), last: self.first - 1 });
        }
        None
    }

    /// Adds the text of pages `first`, `first + 1`, … . A chunk cut short at the byte limit loses its last
    /// page, which is read again with the next chunk.
    pub fn add_text(&mut self, first: u32, mut pages: Vec<String>, truncated: bool) -> Merge {
        if truncated && pages.len() > 1 {
            pages.pop();
        }
        if first == 1 && self.texts.is_empty() && !pages.is_empty() && pages.iter().all(|p| p.trim().is_empty()) {
            self.no_text = true;
        }
        let next = self.first + self.texts.len() as u32;
        let merge = if !self.texts.is_empty() && first == next {
            self.texts.extend(pages);
            Merge::Appended
        } else if !self.texts.is_empty() && first + pages.len() as u32 == self.first && !pages.is_empty() {
            let old_first = self.first;
            pages.append(&mut self.texts);
            self.texts = pages;
            self.first = first;
            self.rebuild();
            return Merge::Prepended(self.starts[(old_first - first) as usize]);
        } else {
            self.first = first;
            self.texts = pages;
            Merge::Replaced
        };
        self.rebuild();
        merge
    }

    /// Forgets the text and starts it again at `page`, for `t` on a page that isn't loaded.
    pub fn restart_text(&mut self, page: u32) {
        self.first = page.max(1);
        self.texts.clear();
        self.rebuild();
    }

    /// Wraps the text again at a new width.
    pub fn relayout(&mut self, width: usize) {
        if width != self.width {
            self.width = width;
            self.rebuild();
        }
    }

    fn rebuild(&mut self) {
        (self.lines, self.starts) = text_lines(self.first, &self.texts, self.width);
    }

    /// The page at line `scroll` of the text view.
    pub fn page_at(&self, scroll: usize) -> u32 {
        self.first + self.starts.partition_point(|&s| s <= scroll).saturating_sub(1) as u32
    }

    /// The line where `page` starts in the text view, if it's loaded.
    pub fn start_of(&self, page: u32) -> Option<usize> {
        page.checked_sub(self.first).and_then(|i| self.starts.get(i as usize)).copied()
    }

    /// The line of the next (`forward`) or previous page start from `scroll`, among the loaded pages.
    pub fn next_start(&self, scroll: usize, forward: bool) -> Option<usize> {
        if forward {
            self.starts.iter().copied().find(|&s| s > scroll)
        } else {
            self.starts.iter().copied().rev().find(|&s| s < scroll)
        }
    }

    /// The current page image, if it's for the page being shown.
    fn current(&self) -> Option<&PageImage> {
        self.image.as_ref().filter(|i| i.page == self.page)
    }

    pub fn go_to(&mut self, page: u32) {
        let page = page.clamp(1, self.pages().max(1));
        if page != self.page {
            self.page = page;
            self.top = 0;
            self.slow = false;
            self.error = None;
        }
    }

    /// Scrolls the page view by `dy` pixels. Past the end of a page it moves to the next or previous one; at
    /// fit page, where there's nothing to scroll, it turns the page.
    pub fn scroll(&mut self, dy: i64, zoom: Zoom, viewport_h: u32) {
        if dy == 0 {
            return;
        }
        if zoom == Zoom::FitPage {
            self.go_to(if dy > 0 { self.page + 1 } else { self.page.saturating_sub(1) });
            return;
        }
        let max_top = self.current().filter(|i| i.zoom == zoom).map(|i| i.img.height().saturating_sub(viewport_h));
        if let Some(m) = max_top {
            self.top = self.top.min(m);
        }
        let step = dy.unsigned_abs().min(u64::from(u32::MAX)) as u32;
        if dy > 0 {
            match max_top {
                Some(m) if self.top >= m => {
                    if self.page < self.pages() {
                        self.go_to(self.page + 1);
                    }
                }
                Some(m) => self.top = self.top.saturating_add(step).min(m),
                None => self.top = self.top.saturating_add(step),
            }
        } else if self.top == 0 {
            if self.page > 1 {
                self.go_to(self.page - 1);
                self.top = u32::MAX;
            }
        } else {
            self.top = self.top.saturating_sub(step);
        }
    }

    /// Pans sideways by `dx` pixels. Returns false if the page isn't wider than the viewport.
    pub fn pan(&mut self, dx: i64, viewport_w: u32) -> bool {
        let Some(img) = self.current() else { return false };
        let max_left = img.img.width().saturating_sub(viewport_w);
        if img.zoom == Zoom::FitPage || max_left == 0 {
            return false;
        }
        let step = dx.unsigned_abs().min(u64::from(u32::MAX)) as u32;
        self.left = if dx > 0 { self.left.saturating_add(step) } else { self.left.saturating_sub(step) }.min(max_left);
        true
    }

    /// Keeps the middle of the view in place when the zoom changes from `old` to `new`.
    pub fn rezoom(&mut self, old: Zoom, new: Zoom, viewport: (u32, u32)) {
        let (vw, vh) = viewport;
        let aspect = self.aspect(self.page);
        let old_w = f64::from(old.width(viewport, aspect).max(1));
        let new_w = f64::from(new.width(viewport, aspect).max(1));
        let ratio = new_w / old_w;
        let mid_y = f64::from(self.top.min(u32::MAX / 2)) + f64::from(vh) / 2.0;
        let mid_x = f64::from(self.left) + f64::from(vw) / 2.0;
        self.top = (mid_y * ratio - f64::from(vh) / 2.0).max(0.0) as u32;
        self.left = (mid_x * ratio - f64::from(vw) / 2.0).max(0.0) as u32;
        self.error = None;
    }

    /// The part of the current page image to show, as (x, y, width, height), or `None` for all of it.
    pub fn crop(&self, viewport: (u32, u32)) -> Option<(u32, u32, u32, u32)> {
        let img = self.image.as_ref()?;
        if img.zoom == Zoom::FitPage {
            return None;
        }
        let (iw, ih) = (img.img.width(), img.img.height());
        let (w, h) = (viewport.0.min(iw), viewport.1.min(ih));
        if (w, h) == (iw, ih) {
            return None;
        }
        // A stale image (still showing the last page) starts at its top.
        let top = if img.page == self.page { self.top } else { 0 };
        Some((self.left.min(iw - w), top.min(ih - h), w, h))
    }

    /// The preview header: `pdf · page 3/93 · fit width · 1.2 MB · <title>`.
    pub fn header(&self, pages_view: bool, zoom: Zoom, scroll: usize, size: &str) -> String {
        let pages_view = pages_view || self.no_text;
        let mut parts = vec!["pdf".to_owned()];
        if self.no_text {
            parts.push("no text · showing pages".into());
        }
        if let Some(info) = &self.info {
            let page = if pages_view { self.page } else { self.page_at(scroll) };
            parts.push(format!("page {page}/{}", info.pages));
        }
        if pages_view {
            parts.push(zoom.label());
            let ready = self.image.as_ref().is_some_and(|i| i.page == self.page && i.zoom == zoom);
            if !ready && self.error.is_none() {
                parts.push(if self.slow { "rendering without smoothing…" } else { "rendering…" }.into());
            }
        }
        if !size.is_empty() {
            parts.push(size.to_owned());
        }
        if let Some(title) = self.info.as_ref().and_then(|i| i.title.clone()) {
            parts.push(title);
        }
        parts.join(" · ")
    }
}

/// The text of pages `first`, `first + 1`, … as lines wrapped at `width`, with a dim `── page N ──` line
/// before every page but the first of the document. Also returns the line each page starts on.
pub fn text_lines(first: u32, texts: &[String], width: usize) -> (Vec<Line<'static>>, Vec<usize>) {
    let mut lines = Vec::new();
    let mut starts = Vec::with_capacity(texts.len());
    for (i, text) in texts.iter().enumerate() {
        let n = first + i as u32;
        if n > 1 && !lines.is_empty() {
            lines.push(Line::default());
        }
        starts.push(lines.len());
        if n > 1 {
            lines.push(Line::styled(format!("── page {n} ──"), dim()));
        }
        let mut blank = true;
        for raw in text.lines() {
            let raw = raw.trim_end();
            if raw.is_empty() {
                // Runs of blank lines become one.
                if !blank {
                    lines.push(Line::default());
                }
                blank = true;
                continue;
            }
            blank = false;
            lines.extend(wrap(raw, width).into_iter().map(Line::raw));
        }
        while lines.len() > starts[i] + usize::from(n > 1) && lines.last().is_some_and(|l| l.width() == 0) {
            lines.pop();
        }
    }
    (lines, starts)
}

/// Splits a line to fit `width` columns, at spaces where it can. Continuation lines keep the line's indent.
fn wrap(line: &str, width: usize) -> Vec<String> {
    let width = width.max(4);
    if line.width() <= width {
        return vec![line.to_owned()];
    }
    let indent = (line.len() - line.trim_start().len()).min(width / 2);
    let pad = " ".repeat(indent);
    let mut out = Vec::new();
    let mut rest = line;
    let mut prefix = "";
    while !rest.is_empty() {
        let avail = width - prefix.len();
        if rest.width() <= avail {
            out.push(format!("{prefix}{rest}"));
            break;
        }
        let (mut used, mut cut, mut space) = (0, 0, None);
        for (i, c) in rest.char_indices() {
            let cw = c.width().unwrap_or(0);
            if used + cw > avail {
                break;
            }
            used += cw;
            cut = i + c.len_utf8();
            if c == ' ' && i > 0 {
                space = Some(i);
            }
        }
        if cut == 0 {
            // A character wider than the space left: take it anyway, so the loop moves on.
            cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
        }
        let (head, tail) = match space {
            Some(sp) if !rest[..sp].trim().is_empty() => (&rest[..sp], rest[sp..].trim_start()),
            _ => (&rest[..cut], &rest[cut..]),
        };
        out.push(format!("{prefix}{}", head.trim_end()));
        rest = tail;
        prefix = &pad;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect()
    }

    fn info(pages: u32) -> Arc<PdfInfo> {
        Arc::new(PdfInfo { pages, title: Some("T".into()), sizes: vec![(600.0, 800.0); pages as usize] })
    }

    fn view(pages: u32) -> PdfView {
        let mut v = PdfView::new("x.pdf".into(), 40);
        v.info = Some(info(pages));
        v
    }

    fn image(v: &mut PdfView, zoom: Zoom, w: u32, h: u32) {
        let img = Arc::new(RgbImage::new(w, h));
        v.image = Some(PageImage { page: v.page, zoom, viewport: (100, 100), img, id: 1 });
    }

    #[test]
    fn zoom_steps() {
        assert_eq!(Zoom::FitWidth.zoom_out(), Zoom::FitPage);
        assert_eq!(Zoom::FitPage.zoom_out(), Zoom::FitPage);
        assert_eq!(Zoom::FitWidth.zoom_in(), Zoom::Percent(150));
        assert_eq!(Zoom::Percent(400).zoom_in(), Zoom::Percent(400));
        assert_eq!(Zoom::Percent(200).width((1000, 900), 0.7), 2000);
        assert_eq!(Zoom::FitPage.width((1000, 900), 0.7), 630);
        assert_eq!(Zoom::FitPage.width((300, 900), 0.7), 300);
    }

    #[test]
    fn text_has_page_lines_and_wraps() {
        let texts = vec!["Title\n\n\n\nBody\n\n".to_owned(), "  a long line that wraps here\n".to_owned()];
        let (lines, starts) = text_lines(1, &texts, 16);
        assert_eq!(text(&lines), ["Title", "", "Body", "", "── page 2 ──", "  a long line", "  that wraps", "  here"]);
        assert_eq!(starts, [0, 4]);
        let (lines, starts) = text_lines(5, &texts[1..], 40);
        assert_eq!(text(&lines), ["── page 5 ──", "  a long line that wraps here"]);
        assert_eq!(starts, [0]);
    }

    #[test]
    fn wrap_handles_long_words() {
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("    x y", 4), ["    ", "  x", "  y"].map(str::trim_end));
    }

    #[test]
    fn text_loads_in_chunks_both_ways() {
        let mut v = view(250);
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), 0, 10, 100), Some(PdfJob::Text { first: 1, last: 100 }));
        let pages = |a: u32, b: u32| (a..=b).map(|n| format!("text {n}\n")).collect::<Vec<_>>();
        assert_eq!(v.add_text(1, pages(1, 100), false), Merge::Replaced);
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), 0, 10, 100), None);
        // Near the end, the next chunk.
        let end = v.lines.len() - 5;
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), end, 10, 100), Some(PdfJob::Text { first: 101, last: 200 }));
        assert_eq!(v.add_text(101, pages(101, 200), false), Merge::Appended);
        assert_eq!(v.page_at(v.start_of(150).unwrap()), 150);

        // `t` from page 230 of the page view starts the text there; scrolling up loads what's above.
        v.restart_text(230);
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), 0, 10, 100), Some(PdfJob::Text { first: 230, last: 329 }));
        assert_eq!(v.add_text(230, pages(230, 250), false), Merge::Replaced);
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), 0, 10, 100), Some(PdfJob::Text { first: 130, last: 229 }));
        let Merge::Prepended(n) = v.add_text(130, pages(130, 229), false) else { panic!() };
        assert_eq!(v.page_at(n), 230);
        // All 250 pages are never loaded at once from here: page 1–129 stay unloaded until scrolled to.
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), n, 10, 100), None);
    }

    #[test]
    fn cut_short_chunks_drop_their_last_page() {
        let mut v = view(10);
        v.add_text(1, vec!["a\n".into(), "b\n".into(), "c, cut sh".into()], true);
        assert_eq!(v.page_at(v.lines.len()), 2);
        assert_eq!(v.wants(false, Zoom::FitWidth, (0, 0), 0, 10, 100), Some(PdfJob::Text { first: 3, last: 102 }));
    }

    #[test]
    fn empty_text_means_scanned() {
        let mut v = view(3);
        v.add_text(1, vec!["\n".into(), " \n".into(), String::new()], false);
        assert!(v.no_text);
        assert_eq!(
            v.wants(false, Zoom::FitWidth, (500, 400), 0, 10, 100),
            Some(PdfJob::Page { page: 1, zoom: Zoom::FitWidth, viewport: (500, 400) })
        );
        assert!(v.header(false, Zoom::FitWidth, 0, "1 KB").starts_with("pdf · no text · showing pages · page 1/3"));
    }

    #[test]
    fn scrolling_runs_on_into_the_next_page_and_back() {
        let mut v = view(3);
        image(&mut v, Zoom::FitWidth, 100, 250);
        v.scroll(60, Zoom::FitWidth, 100);
        assert_eq!((v.page, v.top), (1, 60));
        v.scroll(200, Zoom::FitWidth, 100);
        assert_eq!((v.page, v.top), (1, 150), "stops at the bottom first");
        v.scroll(30, Zoom::FitWidth, 100);
        assert_eq!((v.page, v.top), (2, 0), "then moves on");
        v.scroll(-30, Zoom::FitWidth, 100);
        assert_eq!((v.page, v.top), (1, u32::MAX), "back to the bottom of page 1");
        image(&mut v, Zoom::FitWidth, 100, 250);
        assert_eq!(v.crop((100, 100)), Some((0, 150, 100, 100)));
        v.scroll(-50, Zoom::FitWidth, 100);
        assert_eq!(v.top, 100);
        // The last page doesn't run on.
        v.go_to(3);
        image(&mut v, Zoom::FitWidth, 100, 100);
        v.scroll(30, Zoom::FitWidth, 100);
        assert_eq!(v.page, 3);
        // At fit page the scroll keys turn pages.
        v.scroll(-1, Zoom::FitPage, 100);
        assert_eq!(v.page, 2);
    }

    #[test]
    fn panning_and_zoom_keep_the_view_in_place() {
        let mut v = view(1);
        image(&mut v, Zoom::FitWidth, 100, 140);
        assert!(!v.pan(10, 100), "nothing to pan at fit width");
        image(&mut v, Zoom::Percent(200), 200, 280);
        assert!(v.pan(30, 100));
        assert!(v.pan(500, 100));
        assert_eq!(v.left, 100);
        assert_eq!(v.crop((100, 100)), Some((100, 0, 100, 100)));
        // Zooming in keeps the middle of the view: x 150 at 200% is x 300 at 400%.
        v.top = 50;
        v.rezoom(Zoom::Percent(200), Zoom::Percent(400), (100, 100));
        assert_eq!((v.left, v.top), (250, 150));
    }

    #[test]
    fn page_view_asks_for_the_right_render() {
        let mut v = view(2);
        let job = v.wants(true, Zoom::Percent(150), (400, 300), 0, 10, 100);
        assert_eq!(job, Some(PdfJob::Page { page: 1, zoom: Zoom::Percent(150), viewport: (400, 300) }));
        v.image = Some(PageImage {
            page: 1,
            zoom: Zoom::Percent(150),
            viewport: (400, 300),
            img: Arc::new(RgbImage::new(600, 800)),
            id: 1,
        });
        assert_eq!(v.wants(true, Zoom::Percent(150), (400, 300), 0, 10, 100), None);
        assert!(v.header(true, Zoom::Percent(150), 0, "").ends_with("page 1/2 · 150% · T"));
        // A resize needs a new render.
        assert!(v.wants(true, Zoom::Percent(150), (500, 300), 0, 10, 100).is_some());
        v.go_to(2);
        assert!(v.header(true, Zoom::Percent(150), 0, "").contains("rendering…"));
        assert_eq!(
            v.crop((400, 300)),
            Some((0, 0, 400, 300)),
            "the old page shows from its top until the new one comes"
        );
    }
}
