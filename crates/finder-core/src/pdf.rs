//! PDF previews through poppler's command-line tools (`pdfinfo`, `pdftotext`, `pdftoppm`, package
//! poppler-utils), run as separate programs. See `docs/plans/2026-11-05-pdf-preview.md` and the findings it
//! links for why, and for the timings behind the limits.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use image::RgbImage;

use crate::Limits;
use crate::tool::{self, ToolError};

/// What `pdfinfo` says about a document.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfInfo {
    pub pages: u32,
    pub title: Option<String>,
    /// Each page's size in points as it's shown, that is after its rotation.
    pub sizes: Vec<(f64, f64)>,
}

impl PdfInfo {
    /// Page `page`'s width ÷ height (1-based). Unknown pages count as A4 portrait.
    pub fn aspect(&self, page: u32) -> f64 {
        let (w, h) = self.sizes.get(page.saturating_sub(1) as usize).copied().unwrap_or((595.0, 842.0));
        if w > 0.0 && h > 0.0 { w / h } else { 595.0 / 842.0 }
    }
}

/// A rendered page, and whether it was drawn with anti-aliasing.
#[derive(Debug)]
pub struct Rendered {
    pub image: RgbImage,
    pub smooth: bool,
}

/// Time limit for `pdfinfo` and `pdftotext`, which are quick even on large files (finding: under 1 s for 1004
/// pages of text).
const TOOL_TIMEOUT: Duration = Duration::from_secs(20);

fn missing(tool: &str) -> String {
    format!("PDF preview needs {tool} (package poppler-utils)")
}

/// poppler's message for a password-protected file is "Command Line Error: Incorrect password".
fn explain(e: ToolError) -> ToolError {
    match e {
        ToolError::Failed(msg) if msg.contains("Incorrect password") => {
            ToolError::Failed("password-protected PDF".into())
        }
        other => other,
    }
}

/// Page count, title and page sizes.
pub fn info(path: &Path, cancel: &dyn Fn() -> bool) -> Result<PdfInfo, ToolError> {
    let mut cmd = Command::new("pdfinfo");
    // A last page past the end lists every page's size: +0.5–6 ms (docs/findings/2026-11-05-pdf-scroll-step.md).
    cmd.args(["-f", "1", "-l", "100000000"]).arg(path);
    let out = tool::run(cmd, 4 << 20, Some(TOOL_TIMEOUT), cancel, &missing("pdfinfo")).map_err(explain)?;
    Ok(parse_info(&String::from_utf8_lossy(&out.stdout)))
}

fn parse_info(text: &str) -> PdfInfo {
    let mut info = PdfInfo { pages: 0, title: None, sizes: Vec::new() };
    let mut rotations: Vec<u32> = Vec::new();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("Pages:") {
            info.pages = v.trim().parse().unwrap_or(0);
        } else if let Some(v) = line.strip_prefix("Title:") {
            let v = v.trim();
            info.title = (!v.is_empty()).then(|| v.to_owned());
        } else if let Some(rest) = line.strip_prefix("Page ") {
            // "Page    1 size:  595.276 x 841.89 pts (A4)" and "Page    1 rot:   90"
            let mut words = rest.split_whitespace();
            let (Some(n), Some(what)) = (words.next().and_then(|n| n.parse::<usize>().ok()), words.next()) else {
                continue;
            };
            let slot = n.saturating_sub(1);
            match what {
                "size:" => {
                    let w = words.next().and_then(|v| v.parse::<f64>().ok());
                    let h = words.nth(1).and_then(|v| v.parse::<f64>().ok());
                    if let (Some(w), Some(h)) = (w, h) {
                        if info.sizes.len() <= slot {
                            info.sizes.resize(slot + 1, (595.0, 842.0));
                        }
                        info.sizes[slot] = (w, h);
                    }
                }
                "rot:" => {
                    if rotations.len() <= slot {
                        rotations.resize(slot + 1, 0);
                    }
                    rotations[slot] = words.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                }
                _ => {}
            }
        }
    }
    for (size, rot) in info.sizes.iter_mut().zip(rotations) {
        if rot % 180 == 90 {
            *size = (size.1, size.0);
        }
    }
    info
}

/// The text of pages `first` to `last` (1-based), one string per page, reading at most `max_bytes`. The flag
/// says whether reading stopped at that limit, in which case the last page may be cut short.
pub fn text(
    path: &Path,
    first: u32,
    last: u32,
    max_bytes: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<(Vec<String>, bool), ToolError> {
    let mut cmd = Command::new("pdftotext");
    cmd.args(["-layout", "-enc", "UTF-8", "-f", &first.to_string(), "-l", &last.to_string()]).arg(path).arg("-");
    let out = tool::run(cmd, max_bytes, Some(TOOL_TIMEOUT), cancel, &missing("pdftotext")).map_err(explain)?;
    Ok((split_pages(&String::from_utf8_lossy(&out.stdout)), out.truncated))
}

/// `pdftotext` ends every page with a form feed.
fn split_pages(text: &str) -> Vec<String> {
    let mut pages: Vec<String> = text.split('\u{c}').map(str::to_owned).collect();
    if pages.len() > 1 && pages.last().is_some_and(|p| p.is_empty()) {
        pages.pop();
    }
    pages
}

/// The width to render page `page` at when `want` pixels are wanted, kept under `limits.max_pixels`.
pub fn render_width(info: &PdfInfo, page: u32, want: u32, limits: &Limits) -> u32 {
    let max = (f64::from(limits.max_pixels) * info.aspect(page)).sqrt() as u32;
    want.min(max).max(16)
}

/// Renders page `page` (1-based) `width` pixels wide.
///
/// With `smooth`, it first tries with anti-aliasing for `limits.pdf_slow`; a page that takes longer is
/// rendered again without it (after calling `on_slow`), given `limits.pdf_give_up`. The finding measured
/// normal pages at 14–120 ms and a 200,000-line page at 16 s with anti-aliasing and 4.5 s without.
pub fn page(
    path: &Path,
    page: u32,
    width: u32,
    smooth: bool,
    limits: &Limits,
    cancel: &dyn Fn() -> bool,
    on_slow: &dyn Fn(),
) -> Result<Rendered, ToolError> {
    if smooth {
        match render(path, page, width, true, limits, limits.pdf_slow, cancel) {
            Err(ToolError::TimedOut) => on_slow(),
            other => return other.map(|image| Rendered { image, smooth: true }),
        }
    }
    render(path, page, width, false, limits, limits.pdf_give_up, cancel).map(|image| Rendered { image, smooth: false })
}

fn render(
    path: &Path,
    page: u32,
    width: u32,
    smooth: bool,
    limits: &Limits,
    timeout: Duration,
    cancel: &dyn Fn() -> bool,
) -> Result<RgbImage, ToolError> {
    let n = page.to_string();
    let mut cmd = Command::new("pdftoppm");
    cmd.args(["-f", &n, "-l", &n, "-scale-to-x", &width.to_string(), "-scale-to-y", "-1"]);
    if !smooth {
        cmd.args(["-aaVector", "no"]);
    }
    // Raw PPM on stdout: PNG encoding cost 100 ms to 2.1 s in the finding.
    cmd.arg(path);
    // Room for a page twice the pixel cap, in case the page sizes were wrong.
    let max_out = limits.max_pixels as usize * 3 * 2 + 4096;
    let out = tool::run(cmd, max_out, Some(timeout), cancel, &missing("pdftoppm")).map_err(explain)?;
    if out.truncated {
        return Err(ToolError::Failed("page too large to show".into()));
    }
    if out.stdout.is_empty() {
        return Err(ToolError::Failed(format!("no page {page}")));
    }
    let img = image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Pnm)
        .map_err(|e| ToolError::Failed(format!("can't read the rendered page: {e}")))?;
    Ok(img.into_rgb8())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
    }

    /// Tests that run poppler skip, saying so, when it isn't installed.
    fn have_poppler() -> bool {
        let ok = ["pdfinfo", "pdftotext", "pdftoppm"].iter().all(|t| Command::new(t).arg("-v").output().is_ok());
        if !ok {
            eprintln!("skipped: poppler-utils isn't installed");
        }
        ok
    }

    const NEVER: &dyn Fn() -> bool = &|| false;

    #[test]
    fn parses_info() {
        let text = "Title:           A title\nPages:           3\nPage    1 size:  595.276 x 841.89 pts (A4)\n\
                    Page    1 rot:   0\nPage    2 size:  612 x 792 pts (letter)\nPage    2 rot:   90\n";
        let info = parse_info(text);
        assert_eq!(info.pages, 3);
        assert_eq!(info.title.as_deref(), Some("A title"));
        assert_eq!(info.sizes, vec![(595.276, 841.89), (792.0, 612.0)]);
        assert!((info.aspect(3) - 595.0 / 842.0).abs() < 1e-9, "unknown pages count as A4");
        assert_eq!(parse_info("Title:  \nPages: 1\n").title, None);
    }

    #[test]
    fn splits_pages_at_form_feeds() {
        assert_eq!(split_pages("one\n\u{c}two\n\u{c}"), ["one\n", "two\n"]);
        assert_eq!(split_pages(""), [""]);
    }

    #[test]
    fn render_width_stays_under_the_pixel_cap() {
        let info = PdfInfo { pages: 2, title: None, sizes: vec![(595.0, 842.0), (100.0, 10_000.0)] };
        let limits = Limits::default();
        assert_eq!(render_width(&info, 1, 1000, &limits), 1000);
        let w = render_width(&info, 1, 8000, &limits);
        let h = f64::from(w) / info.aspect(1);
        assert!(f64::from(w) * h <= f64::from(limits.max_pixels) && w > 4000, "{w}");
        // A very tall page is held to a narrow render.
        assert!(render_width(&info, 2, 1000, &limits) < 500);
    }

    #[test]
    fn reads_a_document() {
        if !have_poppler() {
            return;
        }
        let path = data("two-pages.pdf");
        let info = info(&path, NEVER).unwrap();
        assert_eq!(info.pages, 2);
        assert_eq!(info.title.as_deref(), Some("Fixture document"));
        assert_eq!(info.sizes, vec![(595.0, 842.0); 2]);

        let (pages, truncated) = text(&path, 1, 100, 1 << 20, NEVER).unwrap();
        assert_eq!(pages.len(), 2);
        assert!(pages[0].contains("Page one says alpha") && pages[1].contains("Page two says beta"), "{pages:?}");
        assert!(!truncated);
        let (pages, _) = text(&path, 2, 2, 1 << 20, NEVER).unwrap();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].contains("beta"));
        let (_, truncated) = text(&path, 1, 2, 10, NEVER).unwrap();
        assert!(truncated);

        let limits = Limits::default();
        let r = page(&path, 2, 300, true, &limits, NEVER, &|| {}).unwrap();
        assert!(r.smooth);
        assert_eq!(r.image.width(), 300);
        assert_eq!(r.image.height(), (300.0 * 842.0 / 595.0_f64).round() as u32);
        // Page two has a red line across its middle.
        let mid = r.image.get_pixel(150, r.image.height() * 442 / 842);
        assert!(mid[0] > 200 && mid[1] < 60 && mid[2] < 60, "{mid:?}");
    }

    #[test]
    fn slow_pages_are_drawn_again_without_smoothing() {
        if !have_poppler() {
            return;
        }
        // Nothing renders in 0 ms, so the first try always runs out of time.
        let limits = Limits { pdf_slow: Duration::ZERO, ..Limits::default() };
        let told = std::cell::Cell::new(false);
        let r = page(&data("two-pages.pdf"), 1, 200, true, &limits, NEVER, &|| told.set(true)).unwrap();
        assert!(!r.smooth && told.get());
        assert_eq!(r.image.width(), 200);
        let limits = Limits { pdf_slow: Duration::ZERO, pdf_give_up: Duration::ZERO, ..Limits::default() };
        assert!(matches!(page(&data("two-pages.pdf"), 1, 200, true, &limits, NEVER, &|| {}), Err(ToolError::TimedOut)));
    }

    #[test]
    fn scanned_pages_have_no_text() {
        if !have_poppler() {
            return;
        }
        let (pages, _) = text(&data("scanned.pdf"), 1, 100, 1 << 20, NEVER).unwrap();
        assert!(pages.iter().all(|p| p.trim().is_empty()), "{pages:?}");
        assert!(page(&data("scanned.pdf"), 1, 100, true, &Limits::default(), NEVER, &|| {}).is_ok());
    }

    #[test]
    fn broken_and_locked_files() {
        if !have_poppler() {
            return;
        }
        match info(&data("truncated.pdf"), NEVER) {
            Err(ToolError::Failed(msg)) => assert!(msg.contains("trailer"), "{msg}"),
            other => panic!("{other:?}"),
        }
        match info(&data("locked.pdf"), NEVER) {
            Err(ToolError::Failed(msg)) => assert_eq!(msg, "password-protected PDF"),
            other => panic!("{other:?}"),
        }
        match page(&data("locked.pdf"), 1, 100, true, &Limits::default(), NEVER, &|| {}) {
            Err(ToolError::Failed(msg)) => assert_eq!(msg, "password-protected PDF"),
            other => panic!("{other:?}"),
        }
    }
}
