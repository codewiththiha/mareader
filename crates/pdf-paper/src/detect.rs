//! Dominant-colour detection: quantise pixels into 4-bit-per-channel buckets
//! and let the largest bucket's exact mean stand for "the paper".
//!
//! One detector, two ways to feed it. [`PaperDetector::feed`] routes by
//! [`PaperArea`]: the whole frame, or just the left and right edge strips
//! (the margins — where artwork-heavy pages still show honest paper). A
//! detector also POOLS: feeding it every page of a scan, one raster at a
//! time, yields the whole book's dominant colour, which is exactly how the
//! fixed mode finds one colour for a thousand-page document without ever
//! holding more than one raster's pixels.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::color::Rgb;
use crate::config::PaperArea;

thread_local! {
    /// Scratch buffer for callers that sample a downscaled frame before
    /// feeding the detector. Bounded to a 96×96 RGBA tile (~37 KB).
    static SAMPLE_BUF: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Borrow the thread-local sample buffer, sized to `bytes`.
pub fn with_sample_buf<R>(bytes: usize, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    SAMPLE_BUF.with(|buf| {
        let mut buf = buf.borrow_mut();
        if buf.capacity() < bytes {
            buf.reserve(bytes);
        }
        buf.clear();
        buf.resize(bytes, 0);
        f(&mut buf)
    })
}

/// A book's paper has to own at least this share of the sampled pixels; a
/// photo-heavy raster has no paper majority and yields nothing rather than
/// guessing.
pub const PAPER_SHARE: f64 = 0.1;

#[derive(Default)]
struct Bucket {
    n: u64,
    r: u64,
    g: u64,
    b: u64,
}

/// Accumulating bucket histogram over raw RGBA pixels.
#[derive(Default)]
pub struct PaperDetector {
    buckets: HashMap<u16, Bucket>,
    pixels: u64,
}

/// Round a non-negative integer mean to the nearest channel value. Keeping
/// this in integer arithmetic makes the result deterministic across targets
/// while avoiding the downward bias of plain integer division. The remainder
/// comparison is written without `2 * remainder`, so even a very large
/// histogram cannot overflow while deciding whether to round up.
fn rounded_mean(sum: u64, count: u64) -> u8 {
    debug_assert!(count > 0);
    let whole = sum / count;
    let remainder = sum % count;
    let half_up = count / 2 + count % 2;
    (whole + u64::from(remainder >= half_up)) as u8
}

impl PaperDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame's pixels, honouring the configured area. `rgba` is the
    /// frame's full pixel buffer (`width * height * 4`); the edge area walks
    /// only the strip columns. Returns the number of pixels counted.
    pub fn feed(
        &mut self,
        area: PaperArea,
        width: usize,
        height: usize,
        rgba: &[u8],
        edge_width: usize,
    ) -> usize {
        if width == 0 || height == 0 || rgba.len() < width * height * 4 {
            return 0;
        }
        match area {
            PaperArea::WholePage => self.feed_rgba(rgba),
            PaperArea::Edges => self.feed_edges(width, height, rgba, edge_width),
        }
    }

    /// Count every pixel of the buffer.
    fn feed_rgba(&mut self, rgba: &[u8]) -> usize {
        for px in rgba.as_chunks::<4>().0 {
            self.count(px[0], px[1], px[2]);
        }
        rgba.len() / 4
    }

    /// Count only the `edge_width` columns at each side of the frame — the
    /// page's left and right margins. A strip wider than half the page is
    /// the whole page in disguise, so it clamps to `width / 2`.
    fn feed_edges(
        &mut self,
        width: usize,
        height: usize,
        rgba: &[u8],
        edge_width: usize,
    ) -> usize {
        if width == 0 || height == 0 || rgba.len() < width * height * 4 {
            return 0;
        }
        let edge = edge_width.min(width / 2).max(1);
        let stride = width * 4;
        for y in 0..height {
            let row = &rgba[y * stride..y * stride + stride];
            for x in 0..edge {
                let i = x * 4;
                self.count(row[i], row[i + 1], row[i + 2]);
            }
            for x in width - edge..width {
                let i = x * 4;
                self.count(row[i], row[i + 1], row[i + 2]);
            }
        }
        height * edge * 2
    }

    /// The dominant colour, provided one bucket owns at least `min_share` of
    /// every pixel this detector has ever counted. The result is the nearest
    /// representable channel value of that bucket's exact mean, not a bucket
    /// centre — the mean keeps a paper colour that straddles a quantisation
    /// edge from wobbling.
    pub fn dominant(&self, min_share: f64) -> Option<Rgb> {
        let best = self.buckets.values().max_by_key(|b| b.n)?;
        if self.pixels == 0 || best.n == 0 {
            return None;
        }
        let share = best.n as f64 / self.pixels as f64;
        if share < min_share {
            return None;
        }
        Some(Rgb::new(
            rounded_mean(best.r, best.n),
            rounded_mean(best.g, best.n),
            rounded_mean(best.b, best.n),
        ))
    }

    /// Total pixels counted across every feed (the pooled-scan denominator).
    /// A test-only inspector: production reads the dominant colour, never the
    /// raw count.
    #[cfg(test)]
    fn pixels(&self) -> u64 {
        self.pixels
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.pixels == 0
    }

    /// Forget everything — used when the detection area changes, since a
    /// histogram fed through one area says nothing about the other.
    pub fn reset(&mut self) {
        self.buckets.clear();
        self.pixels = 0;
    }

    fn count(&mut self, r: u8, g: u8, b: u8) {
        let key = ((u16::from(r) >> 4) << 8) | ((u16::from(g) >> 4) << 4) | (u16::from(b) >> 4);
        let e = self.buckets.entry(key).or_default();
        e.n += 1;
        e.r += u64::from(r);
        e.g += u64::from(g);
        e.b += u64::from(b);
        self.pixels += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w × h` RGBA buffer: `fill` paints every pixel, then `patch`
    /// overwrites regions.
    fn frame(w: usize, h: usize, fill: [u8; 3]) -> Vec<u8> {
        let mut v = vec![255u8; w * h * 4];
        for i in (0..v.len()).step_by(4) {
            v[i] = fill[0];
            v[i + 1] = fill[1];
            v[i + 2] = fill[2];
        }
        v
    }

    fn paint(buf: &mut [u8], w: usize, x0: usize, x1: usize, colour: [u8; 3]) {
        let rows = buf.len() / (w * 4);
        for y in 0..rows {
            for x in x0..x1 {
                let i = (y * w + x) * 4;
                buf[i] = colour[0];
                buf[i + 1] = colour[1];
                buf[i + 2] = colour[2];
            }
        }
    }

    #[test]
    fn a_uniform_page_finds_its_paper() {
        let mut d = PaperDetector::new();
        let n = d.feed_rgba(&frame(32, 32, [0x40, 0x40, 0x40]));
        assert_eq!(n, 32 * 32);
        assert_eq!(d.dominant(PAPER_SHARE), Some(Rgb::new(0x40, 0x40, 0x40)));
    }

    #[test]
    fn dominant_means_round_to_nearest_channel_value() {
        // Two pixels in one bucket with a 0.5 mean expose the old truncation:
        // the detector must agree with the floating-point compositor and pick
        // 1 rather than the systematically darker 0.
        let rgba = [0, 0, 0, 255, 1, 1, 1, 255];
        let mut d = PaperDetector::new();
        d.feed_rgba(&rgba);
        assert_eq!(d.dominant(PAPER_SHARE), Some(Rgb::new(1, 1, 1)));
    }

    #[test]
    fn a_majority_colour_wins_and_averages_its_own_pixels() {
        // 70% cream + 30% ink: the cream bucket owns the page.
        let mut buf = frame(40, 10, [0xfa, 0xf4, 0xe8]);
        paint(&mut buf, 40, 0, 12, [0x22, 0x22, 0x22]);
        let mut d = PaperDetector::new();
        d.feed_rgba(&buf);
        assert_eq!(d.dominant(0.5), Some(Rgb::new(0xfa, 0xf4, 0xe8)));
    }

    #[test]
    fn no_majority_means_no_answer() {
        // Two 50/50 colours: neither owns the page, so nothing is guessed.
        let mut buf = frame(40, 10, [0x10, 0x10, 0x10]);
        paint(&mut buf, 40, 20, 40, [0xf0, 0xf0, 0xf0]);
        let mut d = PaperDetector::new();
        d.feed_rgba(&buf);
        assert_eq!(d.dominant(0.6), None);
    }

    #[test]
    fn edges_read_the_margins_and_ignore_the_middle() {
        // A scanned page: cream margins, a dark photo filling the middle.
        // Whole-page detection sees 40% cream vs 60% photo and calls the
        // photo the paper; edge detection reads only the margins.
        let mut buf = frame(40, 10, [0xfa, 0xf4, 0xe8]);
        paint(&mut buf, 40, 4, 36, [0x20, 0x20, 0x30]);
        let mut whole = PaperDetector::new();
        whole.feed(PaperArea::WholePage, 40, 10, &buf, 10);
        assert_eq!(whole.dominant(PAPER_SHARE), Some(Rgb::new(0x20, 0x20, 0x30)));

        let mut edges = PaperDetector::new();
        edges.feed(PaperArea::Edges, 40, 10, &buf, 4);
        assert_eq!(edges.dominant(PAPER_SHARE), Some(Rgb::new(0xfa, 0xf4, 0xe8)));
        // 4px strips of a 40px row, both sides, 10 rows.
        assert_eq!(edges.pixels(), 4 * 2 * 10);
    }

    #[test]
    fn an_oversized_edge_strip_clamps_to_half_the_page() {
        let buf = frame(40, 10, [0x80, 0x80, 0x80]);
        let mut d = PaperDetector::new();
        // 99px strips on a 40px page clamp to 20px per side = the whole page.
        let n = d.feed_edges(40, 10, &buf, 99);
        assert_eq!(n, 40 * 10);
        assert_eq!(d.pixels(), 400);
        assert_eq!(d.dominant(PAPER_SHARE), Some(Rgb::new(0x80, 0x80, 0x80)));
    }

    #[test]
    fn feeds_pool_across_pages() {
        // Two frames: page 1 mostly cream with ink text, page 2 all cream.
        // Pooled, cream owns the book even though page 1's own histogram
        // leans the other way — this is the fixed-mode scan's whole trick.
        let mut page1 = frame(40, 10, [0xfa, 0xf4, 0xe8]);
        paint(&mut page1, 40, 0, 30, [0x22, 0x22, 0x22]); // 75% ink
        let page2 = frame(40, 10, [0xfa, 0xf4, 0xe8]);

        let mut d = PaperDetector::new();
        d.feed_rgba(&page1);
        d.feed_rgba(&page2);
        // Cream: 100 + 400 = 500 of 800 pixels.
        assert_eq!(d.dominant(0.5), Some(Rgb::new(0xfa, 0xf4, 0xe8)));
        assert_eq!(d.pixels(), 800);
    }

    #[test]
    fn a_mismatched_buffer_counts_nothing() {
        let mut d = PaperDetector::new();
        assert_eq!(d.feed(PaperArea::WholePage, 10, 10, &[1, 2, 3], 4), 0);
        assert_eq!(d.feed(PaperArea::Edges, 0, 10, &[], 4), 0);
        assert!(d.is_empty());
        assert_eq!(d.dominant(PAPER_SHARE), None);
    }

    #[test]
    fn reset_forgets_the_histogram() {
        let mut d = PaperDetector::new();
        d.feed_rgba(&frame(8, 8, [0x40, 0x40, 0x40]));
        d.reset();
        assert!(d.is_empty());
        assert_eq!(d.dominant(PAPER_SHARE), None);
    }
}
