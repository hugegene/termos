//! File listing, file types and preview data for the finder.
//!
//! This crate knows nothing about terminals. It returns plain data, so the terminal finder and (later) the
//! phone API can both draw it their own way.

use std::cmp::Ordering;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub mod link;
pub mod office;
pub mod pdf;
pub mod sheet;
pub mod tool;
pub mod video;

/// One row of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// What kind of file this is, decided from the name and, if needed, the first bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Dir,
    Markdown,
    Html,
    Csv {
        delimiter: u8,
    },
    Image,
    Pdf,
    /// A video, known by its extension (`video::EXTENSIONS`).
    Video,
    /// A spreadsheet (`sheet::EXTENSIONS`); holds its extension, for the label.
    Sheet(&'static str),
    /// A Word document, shown as Markdown.
    Docx,
    /// A PowerPoint presentation, shown as Markdown.
    Pptx,
    /// An old binary Word or PowerPoint file (`doc`, `ppt`), which isn't read; holds its extension.
    OldOffice(&'static str),
    /// Text or source code. `syntax` is a file extension or name a highlighter can use.
    Text {
        syntax: Option<String>,
    },
    Binary,
}

impl Kind {
    /// A short label for headers, for example "markdown".
    pub fn label(&self) -> &'static str {
        match self {
            Kind::Dir => "folder",
            Kind::Markdown => "markdown",
            Kind::Html => "html",
            Kind::Csv { delimiter: b'\t' } => "tsv",
            Kind::Csv { .. } => "csv",
            Kind::Image => "image",
            Kind::Pdf => "pdf",
            Kind::Video => "video",
            Kind::Sheet(ext) | Kind::OldOffice(ext) => ext,
            Kind::Docx => "docx",
            Kind::Pptx => "pptx",
            Kind::Text { .. } => "text",
            Kind::Binary => "binary",
        }
    }

    /// Whether a text editor can open this kind of file.
    pub fn is_editable(&self) -> bool {
        matches!(self, Kind::Markdown | Kind::Html | Kind::Csv { .. } | Kind::Text { .. })
    }
}

/// Preview content for one file or folder.
#[derive(Debug, Clone)]
pub enum Preview {
    Dir(Vec<Entry>),
    Markdown(String),
    /// HTML converted to Markdown, so it can be shown the same way.
    Html(String),
    Csv {
        rows: Vec<Vec<String>>,
        truncated: bool,
    },
    Image {
        thumbnail: image::RgbImage,
        width: u32,
        height: u32,
    },
    /// A PDF's text, one string per page, from page 1 up to `Limits::pdf_text_pages` pages. `truncated` says
    /// the text stopped at `Limits::max_bytes`.
    Pdf {
        info: pdf::PdfInfo,
        pages: Vec<String>,
        truncated: bool,
    },
    /// A video's facts, and a frame from 10% of the way in: `None` while it's still being read (the finder reads
    /// the facts first, then the frame), or the reason there's none.
    Video {
        info: video::VideoInfo,
        frame: Option<Result<image::RgbImage, String>>,
    },
    /// One sheet of a spreadsheet, as text, up to `Limits::max_rows` rows.
    Sheet(sheet::Sheet),
    /// A Word or PowerPoint file as Markdown.
    Office(office::Document),
    Text {
        text: String,
        syntax: Option<String>,
        truncated: bool,
    },
    Binary {
        size: u64,
    },
    Error(String),
}

/// Limits that keep previews fast on large files.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Bytes read from a text file.
    pub max_bytes: usize,
    /// Rows read from a CSV file.
    pub max_rows: usize,
    /// Longest side of an image thumbnail, in pixels.
    pub max_image_side: u32,
    /// PDF pages whose text is read at a time.
    pub pdf_text_pages: u32,
    /// Most pixels in a rendered PDF page (24 MP is 72 MB; an A4 page at 4000 pixels wide fits).
    pub max_pixels: u32,
    /// A PDF page still rendering after this is drawn again without anti-aliasing.
    pub pdf_slow: Duration,
    /// ... and given this long before the preview gives up on it.
    pub pdf_give_up: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: 2 << 20,
            max_rows: 2000,
            max_image_side: 1600,
            pdf_text_pages: 100,
            max_pixels: 24_000_000,
            pdf_slow: Duration::from_secs(2),
            pdf_give_up: Duration::from_secs(10),
        }
    }
}

/// Lists a folder: folders first, then files, each sorted by name (case-insensitive, numbers in order).
pub fn list_dir(dir: &Path, show_hidden: bool) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for item in fs::read_dir(dir)? {
        let Ok(item) = item else { continue };
        let name = item.file_name().to_string_lossy().into_owned();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let path = item.path();
        let is_symlink = item.file_type().map(|t| t.is_symlink()).unwrap_or(false);
        // Follow symlinks so a link to a folder behaves like a folder.
        let meta = fs::metadata(&path).or_else(|_| item.metadata());
        let (is_dir, size, modified) = match meta {
            Ok(m) => (m.is_dir(), m.len(), m.modified().ok()),
            Err(_) => (false, 0, None),
        };
        entries.push(Entry { name, path, is_dir, is_symlink, size, modified });
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| natural_cmp(&a.name, &b.name)));
    Ok(entries)
}

/// Compares names case-insensitively, with runs of digits compared as numbers ("file2" < "file10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return a.cmp(b),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na: String = std::iter::from_fn(|| ai.next_if(|c| c.is_ascii_digit())).collect();
                let nb: String = std::iter::from_fn(|| bi.next_if(|c| c.is_ascii_digit())).collect();
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                ai.next();
                bi.next();
            }
        }
    }
}

const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Decides the kind of a path from its name, then from its first bytes.
pub fn detect(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Dir;
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "md" | "markdown" | "mdown" | "mkd" => return Kind::Markdown,
        "html" | "htm" | "xhtml" => return Kind::Html,
        "csv" => return Kind::Csv { delimiter: b',' },
        "tsv" => return Kind::Csv { delimiter: b'\t' },
        e if IMAGE_EXTS.contains(&e) => return Kind::Image,
        "pdf" => return Kind::Pdf,
        e if video::EXTENSIONS.contains(&e) => return Kind::Video,
        "docx" => return Kind::Docx,
        "pptx" => return Kind::Pptx,
        "doc" => return Kind::OldOffice("doc"),
        "ppt" => return Kind::OldOffice("ppt"),
        e => {
            if let Some(ext) = sheet::EXTENSIONS.iter().find(|x| **x == e) {
                return Kind::Sheet(ext);
            }
        }
    }
    let head = read_head(path);
    if head.starts_with(b"%PDF-") {
        return Kind::Pdf;
    }
    if looks_binary(&head) {
        return Kind::Binary;
    }
    let syntax = if ext.is_empty() { path.file_name().map(|n| n.to_string_lossy().into_owned()) } else { Some(ext) };
    Kind::Text { syntax }
}

/// The first 8 KB of a file (empty if it can't be read).
fn read_head(path: &Path) -> Vec<u8> {
    let mut head = Vec::with_capacity(8192);
    if let Ok(f) = fs::File::open(path) {
        let _ = f.take(8192).read_to_end(&mut head);
    }
    head
}

/// True if `head` contains a NUL byte or isn't valid UTF-8 (allowing a cut-off last character).
fn looks_binary(head: &[u8]) -> bool {
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(_) => false,
        Err(e) => e.error_len().is_some(),
    }
}

/// Loads preview data for `path`. Never panics; problems come back as `Preview::Error`.
pub fn load_preview(path: &Path, kind: &Kind, limits: Limits) -> Preview {
    match load(path, kind, limits) {
        Ok(p) => p,
        Err(e) => Preview::Error(e.to_string()),
    }
}

fn load(path: &Path, kind: &Kind, limits: Limits) -> io::Result<Preview> {
    Ok(match kind {
        Kind::Dir => Preview::Dir(list_dir(path, false)?),
        Kind::Markdown => Preview::Markdown(read_text(path, limits.max_bytes)?.0),
        Kind::Html => {
            let (html, _) = read_text(path, limits.max_bytes)?;
            Preview::Html(htmd::convert(&html).map_err(|e| io::Error::other(e.to_string()))?)
        }
        Kind::Csv { delimiter } => {
            let mut reader = csv::ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .delimiter(*delimiter)
                .from_path(path)
                .map_err(io::Error::other)?;
            let mut rows = Vec::new();
            let mut truncated = false;
            for record in reader.records() {
                if rows.len() >= limits.max_rows {
                    truncated = true;
                    break;
                }
                let record = record.map_err(io::Error::other)?;
                rows.push(record.iter().map(str::to_owned).collect());
            }
            Preview::Csv { rows, truncated }
        }
        Kind::Image => {
            let img = image::open(path).map_err(io::Error::other)?;
            let (width, height) = (img.width(), img.height());
            let side = limits.max_image_side;
            let thumb = if width > side || height > side { img.thumbnail(side, side) } else { img };
            Preview::Image { thumbnail: thumb.to_rgb8(), width, height }
        }
        Kind::Text { syntax } => {
            let (text, truncated) = read_text(path, limits.max_bytes)?;
            Preview::Text { text, syntax: syntax.clone(), truncated }
        }
        Kind::Pdf => {
            let never = || false;
            let info = pdf::info(path, &never).map_err(io::Error::other)?;
            let (pages, truncated) =
                pdf::text(path, 1, limits.pdf_text_pages, limits.max_bytes, &never).map_err(io::Error::other)?;
            Preview::Pdf { info, pages, truncated }
        }
        Kind::Video => {
            let never = || false;
            let info = video::info(path, &never).map_err(io::Error::other)?;
            let frame = video::frame(path, &info, &limits, &never).map_err(|e| e.to_string());
            Preview::Video { info, frame: Some(frame) }
        }
        Kind::Sheet(_) => Preview::Sheet(sheet::read(path, 0, limits.max_rows).map_err(io::Error::other)?),
        Kind::Docx => Preview::Office(office::docx(path, limits.max_bytes).map_err(io::Error::other)?),
        Kind::Pptx => Preview::Office(office::pptx(path, limits.max_bytes).map_err(io::Error::other)?),
        Kind::OldOffice(ext) => {
            return Err(io::Error::other(format!(".{ext} isn't supported, only .{ext}x")));
        }
        Kind::Binary => Preview::Binary { size: fs::metadata(path)?.len() },
    })
}

/// Reads up to `max` bytes as text, replacing invalid UTF-8. Returns whether the file was cut short.
fn read_text(path: &Path, max: usize) -> io::Result<(String, bool)> {
    let file = fs::File::open(path)?;
    let mut buf = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() > max;
    buf.truncate(max);
    Ok((String::from_utf8_lossy(&buf).into_owned(), truncated))
}

/// Formats a byte count for people, for example "1.5 KB".
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

/// Keeps entries whose names contain the letters of `query` in order (case-insensitive).
/// Returns the indexes of matching entries, best matches (shortest span, earliest start) first.
pub fn filter(entries: &[Entry], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..entries.len()).collect();
    }
    let q: Vec<char> = query.to_lowercase().chars().collect();
    let mut scored: Vec<(usize, usize, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| subsequence_span(&e.name.to_lowercase(), &q).map(|(s, l)| (l, s, i)))
        .collect();
    scored.sort();
    scored.into_iter().map(|(_, _, i)| i).collect()
}

/// Start and length of the shortest-from-first-match span where `q` appears in order in `name`.
fn subsequence_span(name: &str, q: &[char]) -> Option<(usize, usize)> {
    let chars: Vec<char> = name.chars().collect();
    let mut best: Option<(usize, usize)> = None;
    for start in 0..chars.len() {
        if chars[start] != q[0] {
            continue;
        }
        let mut qi = 1;
        let mut end = start;
        for (i, c) in chars.iter().enumerate().skip(start + 1) {
            if qi == q.len() {
                break;
            }
            if *c == q[qi] {
                qi += 1;
                end = i;
            }
        }
        if qi == q.len() {
            let span = (start, end - start + 1);
            if best.is_none_or(|b| span.1 < b.1) {
                best = Some(span);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str) -> Entry {
        Entry { name: name.into(), path: name.into(), is_dir: false, is_symlink: false, size: 0, modified: None }
    }

    #[test]
    fn natural_order() {
        let mut names = vec!["file10", "File2", "file1", "a"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["a", "file1", "File2", "file10"]);
    }

    #[test]
    fn listing_puts_folders_first_and_hides_dotfiles() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("b.txt"), "b").unwrap();
        fs::write(dir.path().join(".hidden"), "h").unwrap();
        fs::create_dir(dir.path().join("zdir")).unwrap();
        let names: Vec<_> = list_dir(dir.path(), false).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["zdir", "b.txt"]);
        assert_eq!(list_dir(dir.path(), true).unwrap().len(), 3);
    }

    #[test]
    fn detects_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let p = |n: &str| dir.path().join(n);
        fs::write(p("a.md"), "# hi").unwrap();
        fs::write(p("b.rs"), "fn main() {}").unwrap();
        fs::write(p("c.bin"), [0u8, 1, 2]).unwrap();
        fs::write(p("Makefile"), "all:").unwrap();
        fs::write(p("doc.PDF"), "not even a pdf").unwrap();
        fs::write(p("download"), b"%PDF-1.7\n\x00\x01binary").unwrap();
        assert_eq!(detect(&p("a.md")), Kind::Markdown);
        assert_eq!(detect(&p("b.rs")), Kind::Text { syntax: Some("rs".into()) });
        assert_eq!(detect(&p("c.bin")), Kind::Binary);
        assert_eq!(detect(&p("Makefile")), Kind::Text { syntax: Some("Makefile".into()) });
        assert_eq!(detect(dir.path()), Kind::Dir);
        assert_eq!(detect(&p("doc.PDF")), Kind::Pdf);
        assert_eq!(detect(&p("download")), Kind::Pdf);
        assert!(!Kind::Pdf.is_editable());
        fs::write(p("clip.MOV"), "").unwrap();
        fs::write(p("types.ts"), "export {}").unwrap();
        assert_eq!(detect(&p("clip.MOV")), Kind::Video);
        assert_eq!(detect(&p("types.ts")), Kind::Text { syntax: Some("ts".into()) });
        assert!(!Kind::Video.is_editable());
        for (name, kind) in [
            ("a.XLSX", Kind::Sheet("xlsx")),
            ("a.ods", Kind::Sheet("ods")),
            ("a.docx", Kind::Docx),
            ("a.pptx", Kind::Pptx),
            ("a.doc", Kind::OldOffice("doc")),
        ] {
            fs::write(p(name), "").unwrap();
            assert_eq!(detect(&p(name)), kind, "{name}");
            assert!(!kind.is_editable());
        }
        assert_eq!(Kind::Sheet("xlsm").label(), "xlsm");
    }

    #[test]
    fn csv_and_html_previews() {
        let dir = tempfile::tempdir().unwrap();
        let csv_path = dir.path().join("t.csv");
        fs::write(&csv_path, "a,b\n1,2,3\n").unwrap();
        match load_preview(&csv_path, &detect(&csv_path), Limits::default()) {
            Preview::Csv { rows, truncated } => {
                assert_eq!(rows, vec![vec!["a", "b"], vec!["1", "2", "3"]]);
                assert!(!truncated);
            }
            other => panic!("{other:?}"),
        }
        let html_path = dir.path().join("p.html");
        fs::write(&html_path, "<h1>Title</h1><p>Hello <b>you</b></p>").unwrap();
        match load_preview(&html_path, &detect(&html_path), Limits::default()) {
            Preview::Html(md) => assert!(md.contains("# Title") && md.contains("**you**"), "{md}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn text_is_truncated_at_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.txt");
        fs::write(&p, "x".repeat(100)).unwrap();
        let limits = Limits { max_bytes: 10, ..Limits::default() };
        match load_preview(&p, &detect(&p), limits) {
            Preview::Text { text, truncated, .. } => assert!(text.len() == 10 && truncated),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn filter_ranks_tight_matches_first() {
        let entries = [entry("notes.md"), entry("n_o_t_e_s.txt"), entry("readme.md")];
        assert_eq!(filter(&entries, "notes"), vec![0, 1]);
        assert_eq!(filter(&entries, "md"), vec![0, 2]);
        assert_eq!(filter(&entries, ""), vec![0, 1, 2]);
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
    }
}
