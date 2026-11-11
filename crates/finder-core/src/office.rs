//! Word (`docx`) and PowerPoint (`pptx`) files as Markdown, for the finder's Markdown renderer. Both are zip files
//! of XML, read with the `zip` and `quick-xml` crates calamine already uses.
//!
//! - **docx:** headings (styles "Heading 1"–"Heading 6" and "Title"), paragraphs, bulleted and numbered lists,
//!   tables, and bold and italic. Headers, footers, footnotes, comments, deleted text and pictures are left out;
//!   a picture becomes `[image]`.
//! - **pptx:** each slide in the presentation's order, as `## 3 · Its title`, then its text, a paragraph per
//!   line, with bullets and their levels. Speaker notes aren't read yet.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, escape};
use zip::ZipArchive;
use zip::result::ZipError;

/// At most this much XML is unpacked from one part of a file, which guards against zip bombs. An **estimate**:
/// real documents' XML is far smaller.
const MAX_PART: u64 = 50 << 20;

/// The start of an OLE (compound) file. Office saves a password-protected docx, xlsx or pptx as one of these,
/// holding the encrypted package; old `.doc`, `.xls` and `.ppt` files are OLE files too.
const OLE: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// A document as Markdown.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub markdown: String,
    /// The Markdown was cut at the size limit.
    pub truncated: bool,
    /// How many slides a presentation has.
    pub slides: Option<usize>,
}

/// Fails with "password-protected `what`" if `path` is an encrypted docx, xlsx or pptx. Those are OLE files with
/// an "EncryptionInfo" stream, not zip files. Other OLE files (an old `.xls`, say) pass.
pub fn check_locked(path: &Path, what: &str) -> Result<(), String> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if !matches!(ext.as_str(), "docx" | "pptx" | "xlsx" | "xlsm" | "xlsb") {
        return Ok(());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut head = [0u8; 8];
    if file.read_exact(&mut head).is_err() || head != OLE {
        return Ok(());
    }
    let mut rest = Vec::new();
    file.take(MAX_PART).read_to_end(&mut rest).map_err(|e| e.to_string())?;
    let name: Vec<u8> = "EncryptionInfo".encode_utf16().flat_map(u16::to_le_bytes).collect();
    if rest.windows(name.len()).any(|w| w == name) {
        return Err(format!("password-protected {what}"));
    }
    Err(format!("not a real .{ext} file: an older Office format under a new name?"))
}

/// A Word document as Markdown, cut at `max_bytes`.
pub fn docx(path: &Path, max_bytes: usize) -> Result<Document, String> {
    check_locked(path, "document")?;
    let mut zip = open(path)?;
    let styles = part(&mut zip, "word/styles.xml")?.map(|x| heading_styles(&x)).unwrap_or_default();
    let numbering = part(&mut zip, "word/numbering.xml")?.map(|x| list_kinds(&x)).unwrap_or_default();
    let body = part(&mut zip, "word/document.xml")?.ok_or("no word/document.xml: not a Word document")?;
    let markdown = Docx::new(styles, numbering).run(&body)?;
    Ok(cut(markdown, max_bytes, None))
}

/// A PowerPoint presentation as Markdown, cut at `max_bytes`.
pub fn pptx(path: &Path, max_bytes: usize) -> Result<Document, String> {
    check_locked(path, "presentation")?;
    let mut zip = open(path)?;
    let pres = part(&mut zip, "ppt/presentation.xml")?.ok_or("no ppt/presentation.xml: not a presentation")?;
    let rels = part(&mut zip, "ppt/_rels/presentation.xml.rels")?.map(|x| relationships(&x)).unwrap_or_default();
    let ids = slide_ids(&pres)?;
    let mut out = String::new();
    let mut count = 0;
    for id in &ids {
        let Some(target) = rels.get(id) else { continue };
        let Some(xml) = part(&mut zip, &resolve("ppt", target))? else { continue };
        count += 1;
        slide(&xml, count, &mut out)?;
        if out.len() > max_bytes {
            break;
        }
    }
    let end = out.trim_end().len();
    out.truncate(end);
    out.push('\n');
    Ok(cut(out, max_bytes, Some(ids.len())))
}

fn open(path: &Path) -> Result<ZipArchive<File>, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    ZipArchive::new(file).map_err(|e| match e {
        ZipError::InvalidArchive(_) => "not a zip file: damaged, or not really this kind of file".to_owned(),
        e => e.to_string(),
    })
}

/// One part of the zip as text, or `None` if there's no such part.
fn part(zip: &mut ZipArchive<File>, name: &str) -> Result<Option<String>, String> {
    let file = match zip.by_name(name) {
        Ok(f) => f,
        Err(ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    let mut buf = Vec::new();
    file.take(MAX_PART + 1).read_to_end(&mut buf).map_err(|e: io::Error| format!("{name}: {e}"))?;
    if buf.len() as u64 > MAX_PART {
        return Err(format!("{name} is over {} MB unpacked", MAX_PART >> 20));
    }
    String::from_utf8(buf).map(Some).map_err(|_| format!("{name} isn't UTF-8"))
}

/// Cuts Markdown at `max` bytes, on a line boundary if one is near.
fn cut(mut markdown: String, max: usize, slides: Option<usize>) -> Document {
    let truncated = markdown.len() > max;
    if truncated {
        let mut end = max;
        while !markdown.is_char_boundary(end) {
            end -= 1;
        }
        let end = markdown[..end].rfind('\n').filter(|n| end - n < 4096).map_or(end, |n| n + 1);
        markdown.truncate(end);
    }
    Document { markdown, truncated, slides }
}

/// A part path relative to the folder `base`, as relationship targets are: `slides/slide1.xml` in `ppt` is
/// `ppt/slides/slide1.xml`; `/ppt/x.xml` is from the top; `..` goes up.
fn resolve(base: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { Vec::new() } else { base.split('/').collect() };
    for piece in target.split('/') {
        match piece {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

// ----- reading XML -----

/// An element's name without its prefix: `p` for `w:p`.
fn local(e: &BytesStart) -> String {
    e.local_name().as_ref().to_owned()
}

/// An attribute's value, by its name without the prefix.
fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

/// Whether a switch such as `<w:b/>` or `<w:b w:val="0"/>` is on.
fn on(e: &BytesStart) -> bool {
    !matches!(attr(e, "val").as_deref(), Some("0" | "false" | "off" | "none"))
}

/// Walks an XML text, calling `f` for each start (`Some(true)` with the element), each end (`Some(false)`), and
/// each piece of text (`None`, with the text). Empty elements come as a start and an end.
fn walk(xml: &str, mut f: impl FnMut(Option<(bool, &BytesStart)>, &str)) -> Result<(), String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event().map_err(|e| format!("bad XML: {e}"))? {
            Event::Start(e) => f(Some((true, &e)), ""),
            Event::Empty(e) => {
                f(Some((true, &e)), "");
                f(Some((false, &e)), "");
            }
            Event::End(e) => {
                let start = BytesStart::new(e.name().as_ref().to_owned());
                f(Some((false, &start)), "");
            }
            Event::Text(t) => f(None, &t.xml10_content()),
            Event::CData(t) => f(None, &t),
            Event::GeneralRef(r) => {
                let text = match r.resolve_char_ref() {
                    Ok(Some(c)) => c.to_string(),
                    _ => escape::resolve_predefined_entity(&r).unwrap_or_default().to_owned(),
                };
                f(None, &text);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

/// Relationship ids and their targets, from a `.rels` part.
fn relationships(xml: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let _ = walk(xml, |ev, _| {
        if let Some((true, e)) = ev
            && local(e) == "Relationship"
            && let (Some(id), Some(target)) = (attr(e, "Id"), attr(e, "Target"))
        {
            map.insert(id, target);
        }
    });
    map
}

// ----- Markdown -----

/// Text with bold and italic, built a run at a time.
#[derive(Default)]
struct Inline {
    pieces: Vec<(String, bool, bool)>,
}

impl Inline {
    fn push(&mut self, text: &str, bold: bool, italic: bool) {
        if text.is_empty() {
            return;
        }
        match self.pieces.last_mut() {
            Some((t, b, i)) if *b == bold && *i == italic => t.push_str(text),
            _ => self.pieces.push((text.to_owned(), bold, italic)),
        }
    }

    fn is_blank(&self) -> bool {
        self.pieces.iter().all(|(t, ..)| t.trim().is_empty())
    }

    /// As Markdown: escaped, with `**` and `*` around bold and italic text (spaces kept outside them). Line
    /// breaks become `breaks`.
    fn markdown(&self, breaks: &str) -> String {
        let mut out = String::new();
        for (text, bold, italic) in &self.pieces {
            let core = text.trim();
            let mark = match (bold, italic) {
                _ if core.is_empty() => "",
                (true, true) => "***",
                (true, false) => "**",
                (false, true) => "*",
                (false, false) => "",
            };
            let lead = &text[..text.len() - text.trim_start().len()];
            let trail = &text[text.trim_end().len()..];
            out.push_str(lead);
            out.push_str(mark);
            out.push_str(&escape_md(core));
            out.push_str(mark);
            out.push_str(trail);
        }
        let out = out.trim().to_owned();
        let out = escape_start(&out);
        out.lines().map(str::trim).collect::<Vec<_>>().join(breaks)
    }
}

/// Backslashes before the characters Markdown would act on.
fn escape_md(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '>' | '|' | '#' | '~' | '&') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Keeps a paragraph that starts like a list item or a quote ("- ", "+ ", "1. ") a paragraph.
fn escape_start(text: &str) -> String {
    let digits = text.chars().take_while(char::is_ascii_digit).count();
    if text.starts_with(['-', '+', '=']) {
        return format!("\\{text}");
    }
    if digits > 0 && text[digits..].starts_with(['.', ')']) {
        return format!("{}\\{}", &text[..digits], &text[digits..]);
    }
    text.to_owned()
}

/// A table in Markdown; the first row is its header.
fn table_markdown(rows: &[Vec<String>]) -> String {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return String::new();
    }
    let line = |row: &[String]| {
        let cells: Vec<&str> = (0..cols).map(|c| row.get(c).map_or("", String::as_str)).collect();
        format!("| {} |\n", cells.join(" | "))
    };
    let mut out = line(&rows[0]);
    out.push_str(&format!("|{}\n", "---|".repeat(cols)));
    for row in &rows[1..] {
        out.push_str(&line(row));
    }
    out
}

/// Builds Markdown blocks, keeping lists together and blank lines between blocks.
#[derive(Default)]
struct Blocks {
    out: String,
    /// The widths of the markers of the list being written, one per level ("- " is 2, "1. " is 3).
    list: Vec<usize>,
}

impl Blocks {
    fn end_list(&mut self) {
        if !self.list.is_empty() {
            self.list.clear();
            self.out.push('\n');
        }
    }

    fn paragraph(&mut self, text: &str) {
        self.end_list();
        self.out.push_str(text);
        self.out.push_str("\n\n");
    }

    fn heading(&mut self, level: usize, text: &str) {
        self.paragraph(&format!("{} {text}", "#".repeat(level.clamp(1, 6))));
    }

    /// A list item at `level` (0 is the outermost), numbered or bulleted.
    fn item(&mut self, level: usize, numbered: bool, text: &str) {
        let level = level.min(8);
        // A nested item is indented to where its parent's text starts.
        while self.list.len() < level {
            self.list.push(2);
        }
        let indent: usize = self.list[..level].iter().sum();
        let marker = if numbered { "1. " } else { "- " };
        self.list.truncate(level);
        self.list.push(marker.len());
        let pad = " ".repeat(indent + marker.len());
        self.out.push_str(&" ".repeat(indent));
        self.out.push_str(marker);
        self.out.push_str(&text.replace('\n', &format!("\n{pad}")));
        self.out.push('\n');
    }

    fn table(&mut self, rows: &[Vec<String>]) {
        self.end_list();
        let t = table_markdown(rows);
        if !t.is_empty() {
            self.out.push_str(&t);
            self.out.push('\n');
        }
    }

    fn finish(mut self) -> String {
        self.end_list();
        let trimmed = self.out.trim_end().len();
        self.out.truncate(trimmed);
        self.out.push('\n');
        self.out
    }
}

// ----- docx -----

/// Heading levels by style id, from `styles.xml`: styles named "heading 1" to "heading 6" (Word keeps English
/// names even in other languages), and "Title" as level 1.
fn heading_styles(xml: &str) -> HashMap<String, usize> {
    let mut map = HashMap::new();
    let mut id: Option<String> = None;
    let _ = walk(xml, |ev, _| match ev {
        Some((true, e)) if local(e) == "style" => id = attr(e, "styleId"),
        Some((true, e)) if local(e) == "name" => {
            if let (Some(id), Some(name)) = (&id, attr(e, "val")) {
                let name = name.to_lowercase();
                let level = match name.strip_prefix("heading ") {
                    Some(n) => n.trim().parse().ok().filter(|n| (1..=6).contains(n)),
                    None => (name == "title").then_some(1),
                };
                if let Some(level) = level {
                    map.insert(id.clone(), level);
                }
            }
        }
        _ => {}
    });
    map
}

/// Whether each list (by `numId`) and level is numbered (`Some(true)`), bulleted (`Some(false)`) or has no
/// marker at all (`None`), from `numbering.xml`.
fn list_kinds(xml: &str) -> HashMap<(String, usize), Option<bool>> {
    let mut abstracts: HashMap<String, HashMap<usize, Option<bool>>> = HashMap::new();
    let mut lists: Vec<(String, String)> = Vec::new();
    let (mut abs, mut level, mut num): (Option<String>, usize, Option<String>) = (None, 0, None);
    let _ = walk(xml, |ev, _| match ev {
        Some((true, e)) => match local(e).as_str() {
            "abstractNum" => abs = attr(e, "abstractNumId"),
            "lvl" => level = attr(e, "ilvl").and_then(|l| l.parse().ok()).unwrap_or(0),
            "numFmt" => {
                if let (Some(a), Some(fmt)) = (&abs, attr(e, "val")) {
                    let numbered = match fmt.as_str() {
                        "none" => None,
                        "bullet" => Some(false),
                        _ => Some(true),
                    };
                    abstracts.entry(a.clone()).or_default().insert(level, numbered);
                }
            }
            "num" => num = attr(e, "numId"),
            "abstractNumId" => {
                if let (Some(n), Some(a)) = (&num, attr(e, "val")) {
                    lists.push((n.clone(), a));
                }
            }
            _ => {}
        },
        Some((false, e)) if matches!(local(e).as_str(), "abstractNum") => abs = None,
        Some((false, e)) if matches!(local(e).as_str(), "num") => num = None,
        _ => {}
    });
    let mut kinds = HashMap::new();
    for (num, abs) in lists {
        for (level, numbered) in abstracts.get(&abs).into_iter().flatten() {
            kinds.insert((num.clone(), *level), *numbered);
        }
    }
    kinds
}

/// Reads `document.xml` into Markdown.
struct Docx {
    styles: HashMap<String, usize>,
    lists: HashMap<(String, usize), Option<bool>>,
    blocks: Blocks,
    /// Elements whose contents are left out, open now (deleted text, a picture's insides, alternatives).
    skip: usize,
    in_ppr: bool,
    in_rpr: bool,
    in_text: bool,
    bold: bool,
    italic: bool,
    para: Inline,
    style: Option<String>,
    num: (Option<String>, usize),
    /// Tables being read, innermost last: their rows of cells.
    tables: Vec<Vec<Vec<String>>>,
}

impl Docx {
    fn new(styles: HashMap<String, usize>, lists: HashMap<(String, usize), Option<bool>>) -> Docx {
        Docx {
            styles,
            lists,
            blocks: Blocks::default(),
            skip: 0,
            in_ppr: false,
            in_rpr: false,
            in_text: false,
            bold: false,
            italic: false,
            para: Inline::default(),
            style: None,
            num: (None, 0),
            tables: Vec::new(),
        }
    }

    fn run(mut self, xml: &str) -> Result<String, String> {
        walk(xml, |ev, text| match ev {
            Some((true, e)) => self.start(e),
            Some((false, e)) => self.end(e),
            None if self.in_text && self.skip == 0 => self.para.push(text, self.bold, self.italic),
            None => {}
        })?;
        Ok(self.blocks.finish())
    }

    fn start(&mut self, e: &BytesStart) {
        let name = local(e);
        if self.skip > 0 {
            if matches!(name.as_str(), "del" | "drawing" | "pict" | "object" | "Fallback") {
                self.skip += 1;
            }
            return;
        }
        match name.as_str() {
            "del" | "Fallback" => self.skip = 1,
            "drawing" | "pict" | "object" => {
                self.para.push(" [image] ", false, false);
                self.skip = 1;
            }
            "p" => {
                self.para = Inline::default();
                self.style = None;
                self.num = (None, 0);
            }
            "pPr" => self.in_ppr = true,
            "pStyle" if self.in_ppr => self.style = attr(e, "val"),
            "ilvl" if self.in_ppr => self.num.1 = attr(e, "val").and_then(|v| v.parse().ok()).unwrap_or(0),
            "numId" if self.in_ppr => self.num.0 = attr(e, "val"),
            "r" => (self.bold, self.italic) = (false, false),
            "rPr" => self.in_rpr = true,
            "b" if self.in_rpr && !self.in_ppr => self.bold = on(e),
            "i" if self.in_rpr && !self.in_ppr => self.italic = on(e),
            "t" if !self.in_ppr => self.in_text = true,
            "tab" if !self.in_ppr => self.para.push(" ", self.bold, self.italic),
            "br" | "cr" if !self.in_ppr && attr(e, "type").is_none_or(|t| t == "textWrapping") => {
                self.para.push("\n", self.bold, self.italic);
            }
            "noBreakHyphen" => self.para.push("-", self.bold, self.italic),
            "tbl" => self.tables.push(Vec::new()),
            "tr" => {
                if let Some(t) = self.tables.last_mut() {
                    t.push(Vec::new());
                }
            }
            "tc" => {
                if let Some(row) = self.tables.last_mut().and_then(|t| t.last_mut()) {
                    row.push(String::new());
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, e: &BytesStart) {
        let name = local(e);
        if self.skip > 0 {
            if matches!(name.as_str(), "del" | "drawing" | "pict" | "object" | "Fallback") {
                self.skip -= 1;
            }
            return;
        }
        match name.as_str() {
            "pPr" => self.in_ppr = false,
            "rPr" => self.in_rpr = false,
            "t" => self.in_text = false,
            "p" => self.end_paragraph(),
            "tbl" => {
                let Some(rows) = self.tables.pop() else { return };
                match self.cell() {
                    // A table in a table: its text goes in the outer cell.
                    Some(cell) => {
                        let text: Vec<String> = rows.iter().map(|r| r.join(" ")).collect();
                        push_cell_text(cell, &text.join(" "));
                    }
                    None => self.blocks.table(&rows),
                }
            }
            _ => {}
        }
    }

    /// The table cell being read, if any.
    fn cell(&mut self) -> Option<&mut String> {
        self.tables.last_mut().and_then(|t| t.last_mut()).and_then(|r| r.last_mut())
    }

    fn end_paragraph(&mut self) {
        let para = std::mem::take(&mut self.para);
        if para.is_blank() {
            return;
        }
        if let Some(cell) = self.cell() {
            push_cell_text(cell, &para.markdown(" "));
            return;
        }
        let heading = self.style.as_ref().and_then(|s| {
            self.styles.get(s).copied().or_else(|| {
                // Without styles.xml, the ids Word and LibreOffice use.
                if s == "Title" { Some(1) } else { s.strip_prefix("Heading").and_then(|n| n.parse().ok()) }
            })
        });
        if let Some(level) = heading {
            self.blocks.heading(level, &para.markdown(" "));
            return;
        }
        // numId 0 means "not in a list". A list Word didn't describe is shown as bullets.
        if let (Some(num), level) = &self.num
            && num != "0"
            && let Some(numbered) = self.lists.get(&(num.clone(), *level)).copied().unwrap_or(Some(false))
        {
            self.blocks.item(*level, numbered, &para.markdown("  \n"));
            return;
        }
        self.blocks.paragraph(&para.markdown("  \n"));
    }
}

/// Adds a paragraph's text to a table cell, on one line.
fn push_cell_text(cell: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !cell.is_empty() {
        cell.push(' ');
    }
    cell.push_str(text);
}

// ----- pptx -----

/// The relationship ids of the slides, in the presentation's order.
fn slide_ids(xml: &str) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    walk(xml, |ev, _| {
        if let Some((true, e)) = ev
            && local(e) == "sldId"
            && let Some(id) = rel_id(e)
        {
            ids.push(id);
        }
    })?;
    Ok(ids)
}

/// The `r:id` of an element: an `id` with a prefix (the relationships namespace), not its own plain `id`.
fn rel_id(e: &BytesStart) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.as_ref().ends_with(":id"))
        .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned()))
}

/// What a shape on a slide is, from its placeholder type.
#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Title,
    /// A body or content placeholder: its paragraphs are bullets unless they say otherwise.
    Body,
    /// Any other shape with text, such as a text box or a subtitle: plain unless a paragraph has a bullet.
    Other,
    /// Dates, footers and slide numbers, which are left out.
    Skip,
}

/// One slide as Markdown, added to `out`.
fn slide(xml: &str, number: usize, out: &mut String) -> Result<(), String> {
    let mut title = Inline::default();
    let mut blocks = Blocks::default();
    let mut hidden = false;
    let mut shape = Shape::Other;
    let mut skip = 0usize;
    let mut para = Inline::default();
    // Bullet from the paragraph's own properties: Some(Some(numbered)), Some(None) for none, None to inherit.
    let mut bullet: Option<Option<bool>> = None;
    let mut level = 0usize;
    let (mut bold, mut italic, mut in_text) = (false, false, false);
    let mut tables: Vec<Vec<Vec<String>>> = Vec::new();
    walk(xml, |ev, text| match ev {
        Some((true, e)) => {
            let name = local(e);
            if skip > 0 {
                if matches!(name.as_str(), "Fallback") {
                    skip += 1;
                }
                return;
            }
            match name.as_str() {
                "sld" => hidden = attr(e, "show").as_deref() == Some("0"),
                "Fallback" => skip = 1,
                "sp" => shape = Shape::Other,
                "ph" => {
                    shape = match attr(e, "type").as_deref() {
                        Some("title" | "ctrTitle") => Shape::Title,
                        None | Some("body" | "obj") => Shape::Body,
                        Some("dt" | "ftr" | "sldNum" | "hdr") => Shape::Skip,
                        Some(_) => Shape::Other,
                    }
                }
                "pic" => blocks.paragraph("[image]"),
                "p" => {
                    para = Inline::default();
                    bullet = None;
                    level = 0;
                }
                "pPr" => level = attr(e, "lvl").and_then(|l| l.parse().ok()).unwrap_or(0),
                "buNone" => bullet = Some(None),
                "buChar" => bullet = Some(Some(false)),
                "buAutoNum" => bullet = Some(Some(true)),
                "r" | "fld" => (bold, italic) = (false, false),
                "rPr" => {
                    bold = attr(e, "b").is_some_and(|v| v == "1" || v == "true");
                    italic = attr(e, "i").is_some_and(|v| v == "1" || v == "true");
                }
                "t" => in_text = true,
                "br" => para.push("\n", bold, italic),
                "tbl" => tables.push(Vec::new()),
                "tr" => {
                    if let Some(t) = tables.last_mut() {
                        t.push(Vec::new());
                    }
                }
                "tc" => {
                    if let Some(row) = tables.last_mut().and_then(|t| t.last_mut()) {
                        row.push(String::new());
                    }
                }
                _ => {}
            }
        }
        Some((false, e)) => {
            let name = local(e);
            if skip > 0 {
                if name == "Fallback" {
                    skip -= 1;
                }
                return;
            }
            match name.as_str() {
                "t" => in_text = false,
                "sp" => shape = Shape::Other,
                "p" => {
                    let p = std::mem::take(&mut para);
                    if p.is_blank() || shape == Shape::Skip {
                        return;
                    }
                    if let Some(cell) = tables.last_mut().and_then(|t| t.last_mut()).and_then(|r| r.last_mut()) {
                        push_cell_text(cell, &p.markdown(" "));
                        return;
                    }
                    if shape == Shape::Title {
                        if !title.is_blank() {
                            title.push(" ", false, false);
                        }
                        for (t, b, i) in &p.pieces {
                            title.push(&t.replace('\n', " "), *b, *i);
                        }
                        return;
                    }
                    let numbered = match bullet {
                        Some(b) => b,
                        None => (shape == Shape::Body).then_some(false),
                    };
                    match numbered {
                        Some(numbered) => blocks.item(level, numbered, &p.markdown("  \n")),
                        None => blocks.paragraph(&p.markdown("  \n")),
                    }
                }
                "tbl" => {
                    if let Some(rows) = tables.pop() {
                        blocks.table(&rows);
                    }
                }
                _ => {}
            }
        }
        None if in_text && skip == 0 => para.push(text, bold, italic),
        None => {}
    })?;
    let title = title.markdown(" ");
    out.push_str(&format!("## {number}"));
    if !title.is_empty() {
        out.push_str(&format!(" · {title}"));
    }
    if hidden {
        out.push_str(" (hidden)");
    }
    out.push_str("\n\n");
    let body = blocks.finish();
    if !body.trim().is_empty() {
        out.push_str(&body);
        out.push('\n');
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn data(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)
    }

    #[test]
    fn word_document() {
        let doc = docx(&data("office.docx"), 1 << 20).unwrap();
        assert_eq!(
            doc.markdown,
            "# Trip report\n\n\
             We went to **Lisbon** in *November*, and it was 3 \\* warmer.\n\n\
             ## What we saw\n\n\
             - The castle\n\
             - Trams\n  \
               - Line 28\n\
             1. First day\n\
             1. Second day\n\n\
             | Item | Cost |\n|---|---|\n| Hotel | 420 |\n| Food \\| drink | 180 |\n\n\
             The end.\n"
        );
        assert!(!doc.truncated && doc.slides.is_none());
    }

    #[test]
    fn presentation() {
        let doc = pptx(&data("deck.pptx"), 1 << 20).unwrap();
        assert_eq!(
            doc.markdown,
            "## 1 · Quarterly review\n\n\
             Prepared by the team\n\n\
             ## 2 · Results\n\n\
             - Revenue up\n  \
               - Mostly in Europe\n\
             - Costs flat\n\n\
             ## 3 · Next steps\n\n\
             - Hire two people\n\n\
             Questions?\n"
        );
        assert_eq!(doc.slides, Some(3));
    }

    #[test]
    fn cut_at_the_limit() {
        let doc = docx(&data("office.docx"), 30).unwrap();
        assert!(doc.truncated);
        assert_eq!(doc.markdown, "# Trip report\n\n");
    }

    #[test]
    fn locked_broken_and_old_files() {
        assert_eq!(docx(&data("locked.docx"), 1000), Err("password-protected document".into()));
        let dir = tempfile::tempdir().unwrap();
        let cut = dir.path().join("cut.docx");
        let bytes = std::fs::read(data("office.docx")).unwrap();
        std::fs::write(&cut, &bytes[..bytes.len() * 6 / 10]).unwrap();
        assert!(docx(&cut, 1000).unwrap_err().starts_with("not a zip file"), "{:?}", docx(&cut, 1000));
        // An old binary .xls renamed .docx is an OLE file without encryption.
        let renamed = dir.path().join("old.docx");
        std::fs::copy(data("sheet.xls"), &renamed).unwrap();
        assert_eq!(
            docx(&renamed, 1000).unwrap_err(),
            "not a real .docx file: an older Office format under a new name?"
        );
        // A spreadsheet isn't a presentation.
        assert_eq!(pptx(&data("sheet.xlsx"), 1000).unwrap_err(), "no ppt/presentation.xml: not a presentation");
    }

    #[test]
    fn markdown_pieces() {
        assert_eq!(escape_start("1. not a list"), "1\\. not a list");
        assert_eq!(escape_start("- not a bullet"), "\\- not a bullet");
        assert_eq!(escape_md("a_b*c"), "a\\_b\\*c");
        let mut i = Inline::default();
        i.push("plain ", false, false);
        i.push("bold ", true, false);
        i.push("both", true, true);
        assert_eq!(i.markdown(" "), "plain **bold** ***both***");
        assert_eq!(resolve("ppt", "slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(resolve("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(resolve("ppt", "/ppt/slides/slide2.xml"), "ppt/slides/slide2.xml");
    }
}
