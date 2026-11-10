//! Real images in the finder (step 5): fitting an image to a box, encoding it for the kitty, iTerm2 or sixel
//! protocol, and a background thread that does the encoding.
//!
//! Each client says which protocol its terminal speaks; the server encodes per client and places the image
//! after the frame's text (see `Server::render_all`).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::mpsc::{self, Sender};
use std::thread;

use base64::Engine;
use image::codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder};
use image::imageops::FilterType;
use image::{ExtendedColorType, ImageEncoder, RgbImage};
use ratatui::layout::Rect;

use super::Ev;
use crate::protocol::ImageProtocol;

/// Cell size to assume when the terminal didn't tell us (only the aspect ratio matters then).
pub const DEFAULT_CELL: (u16, u16) = (8, 16);

/// One encoding of one image: which image, for which protocol, at which size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageKey {
    pub id: u64,
    pub protocol: ImageProtocol,
    pub cells: (u16, u16),
    pub px: (u32, u32),
}

impl ImageKey {
    /// The kitty image id: never zero, and stable for the same picture.
    pub fn kitty_id(&self) -> u32 {
        (self.id % 0xFF_FFFF) as u32 + 1
    }
}

/// An image a client should show: the cells it covers and the bytes that draw it there.
#[derive(Clone, Debug)]
pub struct Placed {
    pub key: ImageKey,
    pub rect: Rect,
    pub bytes: Arc<Vec<u8>>,
}

pub struct Job {
    pub key: ImageKey,
    pub img: Arc<RgbImage>,
}

/// Fits an image of `w`×`h` pixels into `area`, never enlarging it, centred across the top. Returns the cells
/// it covers and its size in pixels. Sixel heights are whole multiples of 6, the height of one sixel row.
pub fn fit(w: u32, h: u32, area: Rect, cell: (u16, u16), protocol: ImageProtocol) -> (Rect, (u32, u32)) {
    let (cw, ch) = (u32::from(cell.0.max(1)), u32::from(cell.1.max(1)));
    let (max_w, max_h) = (u32::from(area.width) * cw, u32::from(area.height) * ch);
    let scale = (max_w as f32 / w.max(1) as f32).min(max_h as f32 / h.max(1) as f32).min(1.0);
    let px_w = ((w as f32 * scale).round() as u32).clamp(1, max_w.max(1));
    let mut px_h = ((h as f32 * scale).round() as u32).clamp(1, max_h.max(1));
    if protocol == ImageProtocol::Sixel {
        px_h = (px_h / 6 * 6).max(6.min(max_h));
    }
    let cols = (px_w.div_ceil(cw) as u16).clamp(1, area.width.max(1));
    let rows = (px_h.div_ceil(ch) as u16).clamp(1, area.height.max(1));
    let x = area.x + (area.width.saturating_sub(cols)) / 2;
    (Rect::new(x, area.y, cols, rows), (px_w, px_h))
}

fn png(img: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    let encoder = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, PngFilter::Adaptive);
    let _ = encoder.write_image(img.as_raw(), img.width(), img.height(), ExtendedColorType::Rgb8);
    out
}

/// kitty graphics protocol: transmit and display in one go, in base64 chunks of 4096 bytes. `C=1` keeps the
/// cursor still and `q=2` stops the terminal from answering (answers would arrive as typed input).
pub fn kitty(png: &[u8], id: u32, cols: u16, rows: u16) -> Vec<u8> {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(4096).collect();
    let mut out = Vec::with_capacity(data.len() + chunks.len() * 16 + 64);
    for (i, chunk) in chunks.iter().enumerate() {
        let more = u8::from(i + 1 < chunks.len());
        if i == 0 {
            let _ = write!(VecWriter(&mut out), "\x1b_Ga=T,f=100,t=d,i={id},c={cols},r={rows},C=1,q=2,m={more};");
        } else {
            let _ = write!(VecWriter(&mut out), "\x1b_Gm={more};");
        }
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

/// Deletes a kitty image (its placements and data).
pub fn kitty_delete(id: u32) -> Vec<u8> {
    format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\").into_bytes()
}

/// iTerm2 inline image, scaled by the terminal into `cols`×`rows` cells.
pub fn iterm2(png: &[u8], cols: u16, rows: u16) -> Vec<u8> {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    format!("\x1b]1337;File=inline=1;size={};width={cols};height={rows};preserveAspectRatio=1:{data}\x07", png.len())
        .into_bytes()
}

/// Sixel, with a 6×6×6 colour cube and Floyd–Steinberg dithering.
pub fn sixel(img: &RgbImage) -> Vec<u8> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let index = dither(img);
    let mut out = String::with_capacity(w * h / 2);
    // P2=1: pixels we don't set keep the background. The raster attributes set a 1:1 pixel aspect.
    let _ = write!(out, "\x1bP0;1q\"1;1;{w};{h}");
    for i in 0..216u32 {
        let (r, g, b) = (i / 36, (i / 6) % 6, i % 6);
        let _ = write!(out, "#{i};2;{};{};{}", r * 20, g * 20, b * 20);
    }
    let mut bits = vec![0u8; 216 * w];
    let mut present = [false; 216];
    for band in (0..h).step_by(6) {
        present.fill(false);
        for k in 0..6 {
            let y = band + k;
            if y >= h {
                break;
            }
            for x in 0..w {
                let c = usize::from(index[y * w + x]);
                present[c] = true;
                bits[c * w + x] |= 1 << k;
            }
        }
        let mut first = true;
        for c in (0..216).filter(|c| present[*c]) {
            if !first {
                out.push('$');
            }
            first = false;
            let _ = write!(out, "#{c}");
            let row = &mut bits[c * w..(c + 1) * w];
            let mut x = 0;
            while x < w {
                let v = row[x];
                let mut run = 1;
                while x + run < w && row[x + run] == v {
                    run += 1;
                }
                let ch = char::from(63 + v);
                if run > 3 {
                    let _ = write!(out, "!{run}{ch}");
                } else {
                    (0..run).for_each(|_| out.push(ch));
                }
                x += run;
            }
            row.fill(0);
        }
        out.push('-');
    }
    out.push_str("\x1b\\");
    out.into_bytes()
}

/// Maps each pixel to a colour-cube index (r*36 + g*6 + b), spreading the rounding error to its neighbours.
fn dither(img: &RgbImage) -> Vec<u8> {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut cur = vec![[0f32; 3]; w + 2];
    let mut next = vec![[0f32; 3]; w + 2];
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x as u32, y as u32);
            let mut level = [0u8; 3];
            for ch in 0..3 {
                let v = (f32::from(p[ch]) + cur[x + 1][ch]).clamp(0.0, 255.0);
                let q = (v * 5.0 / 255.0).round();
                level[ch] = q as u8;
                let e = v - q * 51.0;
                cur[x + 2][ch] += e * 7.0 / 16.0;
                next[x][ch] += e * 3.0 / 16.0;
                next[x + 1][ch] += e * 5.0 / 16.0;
                next[x + 2][ch] += e / 16.0;
            }
            out[y * w + x] = level[0] * 36 + level[1] * 6 + level[2];
        }
        std::mem::swap(&mut cur, &mut next);
        next.iter_mut().for_each(|e| *e = [0.0; 3]);
    }
    out
}

/// Resizes and encodes an image for one protocol and size.
pub fn encode(img: &RgbImage, key: &ImageKey) -> Vec<u8> {
    let (pw, ph) = key.px;
    let resized;
    let img = if (img.width(), img.height()) == (pw, ph) {
        img
    } else {
        resized = image::imageops::resize(img, pw.max(1), ph.max(1), FilterType::Triangle);
        &resized
    };
    let (cols, rows) = key.cells;
    match key.protocol {
        ImageProtocol::Kitty => kitty(&png(img), key.kitty_id(), cols, rows),
        ImageProtocol::Iterm2 => iterm2(&png(img), cols, rows),
        ImageProtocol::Sixel => sixel(img),
        ImageProtocol::Blocks => Vec::new(),
    }
}

/// Encoded images, and the thread that makes them.
pub struct Images {
    cache: HashMap<ImageKey, Arc<Vec<u8>>>,
    order: VecDeque<ImageKey>,
    pending: HashSet<ImageKey>,
    jobs: Sender<Job>,
}

/// Encodings kept; a few per client and pane is plenty.
const CACHE: usize = 24;

impl Images {
    pub fn new(tx: Sender<Ev>) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        thread::spawn(move || {
            while let Ok(mut job) = rx.recv() {
                // Skip jobs overtaken while we were busy (scrolling through a folder of photos). The server
                // asks again for anything it still needs.
                while let Ok(newer) = rx.try_recv() {
                    let _ = tx.send(Ev::Image(job.key, None));
                    job = newer;
                }
                let bytes = encode(&job.img, &job.key);
                if tx.send(Ev::Image(job.key, Some(Arc::new(bytes)))).is_err() {
                    break;
                }
            }
        });
        Images { cache: HashMap::new(), order: VecDeque::new(), pending: HashSet::new(), jobs }
    }

    pub fn get(&self, key: &ImageKey) -> Option<Arc<Vec<u8>>> {
        self.cache.get(key).cloned()
    }

    pub fn request(&mut self, job: Job) {
        if !self.cache.contains_key(&job.key) && self.pending.insert(job.key) {
            let _ = self.jobs.send(job);
        }
    }

    pub fn done(&mut self, key: ImageKey, bytes: Option<Arc<Vec<u8>>>) {
        self.pending.remove(&key);
        let Some(bytes) = bytes else { return };
        if self.cache.insert(key, bytes).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE {
            if let Some(old) = self.order.pop_front() {
                self.cache.remove(&old);
            }
        }
    }
}

/// The picture a client already shows inside `area`, to keep up while a new one for that area is encoded, so a
/// scrolled PDF page doesn't flash blocks between steps.
pub fn still_shown(shown: &[Placed], area: Rect, protocol: ImageProtocol) -> Option<Placed> {
    shown.iter().find(|s| s.key.protocol == protocol && area.union(s.rect) == area).cloned()
}

/// `write!` into a byte vector.
struct VecWriter<'a>(&'a mut Vec<u8>);

impl std::fmt::Write for VecWriter<'_> {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

/// What a client's terminal needs to go from showing `shown` to showing `wanted`. `touched` says whether text
/// was drawn inside a rectangle this frame (which wipes sixel and iTerm2 images, and covers kitty ones).
pub struct Changes {
    /// Images to take away: kitty ones are deleted by id; sixel and iTerm2 ones by redrawing their cells.
    pub remove: Vec<Placed>,
    /// Images to draw, after the text.
    pub add: Vec<Placed>,
}

pub fn changes(shown: &[Placed], wanted: &[Placed], touched: impl Fn(Rect) -> bool) -> Changes {
    let same = |a: &Placed, b: &Placed| a.key == b.key && a.rect == b.rect;
    let mut remove = Vec::new();
    let mut keep = Vec::new();
    for s in shown {
        if wanted.iter().any(|w| same(w, s)) && !touched(s.rect) {
            keep.push(s.clone());
        } else {
            remove.push(s.clone());
        }
    }
    let add = wanted.iter().filter(|w| !keep.iter().any(|k| same(k, w))).cloned().collect();
    Changes { remove, add }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: u64) -> ImageKey {
        ImageKey { id, protocol: ImageProtocol::Kitty, cells: (10, 5), px: (80, 80) }
    }

    fn placed(id: u64, x: u16) -> Placed {
        Placed { key: key(id), rect: Rect::new(x, 2, 10, 5), bytes: Arc::new(Vec::new()) }
    }

    #[test]
    fn images_fit_without_growing() {
        let area = Rect::new(10, 2, 40, 20);
        // 640×160 in 8×16 cells: fits the width (320 px wide → scale 0.5), 40×5 cells, at the top.
        let (r, px) = fit(640, 160, area, (8, 16), ImageProtocol::Kitty);
        assert_eq!((r, px), (Rect::new(10, 2, 40, 5), (320, 80)));
        // A small image keeps its size and is centred.
        let (r, px) = fit(80, 32, area, (8, 16), ImageProtocol::Kitty);
        assert_eq!((r, px), (Rect::new(25, 2, 10, 2), (80, 32)));
        // Sixel heights are multiples of 6.
        let (_, px) = fit(80, 35, area, (8, 16), ImageProtocol::Sixel);
        assert_eq!(px, (80, 30));
    }

    #[test]
    fn kitty_sends_chunks() {
        let data = vec![7u8; 5000];
        let out = String::from_utf8(kitty(&data, 3, 10, 4)).unwrap();
        assert!(out.starts_with("\x1b_Ga=T,f=100,t=d,i=3,c=10,r=4,C=1,q=2,m=1;"));
        assert_eq!(out.matches("\x1b_G").count(), 2, "5000 bytes are 6668 base64 characters: two chunks");
        assert!(out.contains("\x1b_Gm=0;"));
        assert!(out.ends_with("\x1b\\"));
    }

    #[test]
    fn iterm2_and_sixel_are_well_formed() {
        let out = String::from_utf8(iterm2(b"png", 12, 6)).unwrap();
        assert_eq!(out, "\x1b]1337;File=inline=1;size=3;width=12;height=6;preserveAspectRatio=1:cG5n\x07");

        let mut img = RgbImage::from_pixel(10, 7, image::Rgb([255, 0, 0]));
        img.put_pixel(0, 6, image::Rgb([0, 0, 255]));
        let out = String::from_utf8(sixel(&img)).unwrap();
        assert!(out.starts_with("\x1bP0;1q\"1;1;10;7"));
        assert!(out.ends_with("-\x1b\\"));
        assert_eq!(out.matches('-').count(), 2, "7 rows are two sixel bands");
        // Pure red is cube colour 5*36 = 180: a full first band of it.
        assert!(out.contains("#180!10~"), "{out}");
        // Pure blue (index 5) appears only in the second band's first column, top bit.
        assert!(out.contains("#5@"), "{out}");
    }

    #[test]
    fn changes_keep_untouched_images() {
        let shown = vec![placed(1, 0), placed(2, 20)];
        let wanted = vec![placed(1, 0), placed(3, 20)];
        let c = changes(&shown, &wanted, |_| false);
        assert_eq!(c.remove.iter().map(|p| p.key.id).collect::<Vec<_>>(), vec![2]);
        assert_eq!(c.add.iter().map(|p| p.key.id).collect::<Vec<_>>(), vec![3]);
        // Text drawn over image 1 means it's drawn again.
        let c = changes(&shown, &wanted, |r| r.x == 0);
        assert_eq!(c.remove.iter().map(|p| p.key.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(c.add.iter().map(|p| p.key.id).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn keeps_a_picture_inside_the_area() {
        let shown = [placed(1, 2), placed(2, 40)];
        let area = Rect::new(0, 0, 20, 10);
        assert_eq!(still_shown(&shown, area, ImageProtocol::Kitty).map(|p| p.key.id), Some(1));
        assert!(still_shown(&shown, Rect::new(30, 0, 5, 10), ImageProtocol::Kitty).is_none(), "doesn't fit");
        assert!(still_shown(&shown, area, ImageProtocol::Sixel).is_none(), "another protocol");
    }

    /// Times one scroll step of a PDF page at fit width: cutting a 1000×900 piece out of the page and encoding
    /// it for each protocol (step 7 of docs/plans/2026-11-05-pdf-preview.md). Needs poppler. Run with:
    /// `TOS_BENCH_PDF=file.pdf cargo test --release -p tos --bin tos scroll_step_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn scroll_step_cost() {
        use std::time::{Duration, Instant};
        let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/../finder-core/tests/data/two-pages.pdf");
        let path = std::env::var("TOS_BENCH_PDF").unwrap_or_else(|_| fixture.into());
        let limits = finder_core::Limits::default();
        let page = finder_core::pdf::page(path.as_ref(), 1, 1000, true, &limits, &|| false, &|| {}).unwrap().image;
        println!("{path}: page 1 rendered {}×{}", page.width(), page.height());
        let median = |mut v: Vec<Duration>| {
            v.sort();
            v[v.len() / 2].as_secs_f64() * 1000.0
        };
        // Steps of 3 rows of 20 pixels, as `j` makes, down from the top.
        let crops: Vec<RgbImage> = (0..10)
            .map(|i| image::imageops::crop_imm(&page, 0, i * 60, 1000, 900.min(page.height() - i * 60)).to_image())
            .collect();
        let mut times = Vec::new();
        for i in 0..10 {
            let t = Instant::now();
            let _ = image::imageops::crop_imm(&page, 0, i * 60, 1000, 900.min(page.height() - i * 60)).to_image();
            times.push(t.elapsed());
        }
        println!("| cut out 1000×900 | {:.1} ms | |", median(times));
        for protocol in [ImageProtocol::Kitty, ImageProtocol::Iterm2, ImageProtocol::Sixel] {
            let key = ImageKey { id: 1, protocol, cells: (100, 45), px: (1000, 900) };
            let mut times = Vec::new();
            let mut bytes = Vec::new();
            for crop in &crops {
                let k = ImageKey { px: (crop.width(), crop.height()), ..key };
                let t = Instant::now();
                let out = encode(crop, &k);
                times.push(t.elapsed());
                bytes.push(out.len());
            }
            bytes.sort();
            println!("| encode {protocol:?} | {:.1} ms | {} bytes |", median(times), bytes[bytes.len() / 2]);
        }
    }
}
