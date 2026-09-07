//! Dominant-colour detection: quantise pixels into 5-bit-per-channel buckets
//! and let the largest bucket's exact mean stand for "the paper".
//!
//! One detector, two ways to feed it. [`PaperDetector::feed`] routes by
//! [`PaperArea`]: the whole frame, or just the margin bands along the four
//! edges (where artwork-heavy pages still show honest paper). A detector
//! also POOLS: feeding it several rasters, one at a time, yields their
//! combined dominant colour without ever holding more than one raster's
//! pixels.
//!
//! The bucket grain is the detector's honesty: a cell wide enough to swallow
//! a page's margin and its body into one bucket makes the two areas ask the
//! same question — WholePage's mean is the body-and-margin mix and Edges'
//! margin-only mean sits a few units off it, so both modes publish the same
//! mud to the eye and the backdrop matches neither surface. Five bits per
//! channel (32-wide cells) still folds scan noise and JPEG ringing into one
//! bucket per flat region, but keeps a margin its own colour.

use std::collections::HashMap;

use crate::color::Rgb;
use crate::config::PaperArea;

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
    /// only the margin bands. Returns the number of pixels counted.
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

    /// Count only the margin bands: the pixels within `edge_width` of any of
    /// the frame's four sides.
    fn feed_edges(
        &mut self,
        width: usize,
        height: usize,
        rgba: &[u8],
        edge_width: usize,
    ) -> usize {
        // The strips are `edge` px deep on all four sides. Two opposing
        // strips of HALF the extent each tile the entire axis, so the old
        // half-of-width clamp turned Edges into WholePage in disguise
        // whenever the width — a page-scale number applied to a frame
        // downscaled to ≤96px — reached half the shorter axis, and the body
        // colour rode home in the margin's histogram. A quarter of the
        // shorter axis keeps the four strips a margin at every frame scale;
        // a sane edge width on a full-resolution frame never meets the cap.
        let edge = edge_width.clamp(1, (width.min(height) / 4).max(1));
        let mut fed = 0;
        for y in 0..height {
            let in_band = y < edge || y + edge >= height;
            for x in 0..width {
                if in_band || x < edge || x + edge >= width {
                    let i = (y * width + x) * 4;
                    self.count(rgba[i], rgba[i + 1], rgba[i + 2]);
                    fed += 1;
                }
            }
        }
        fed
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

    /// Total pixels counted across every feed (the pooled denominator).
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
        // Five bits per channel: a 32-wide cell. Four bits merged margins
        // into bodies on low-contrast sheets (see the module doc); fifteen
        // bits of key still fit the u16 the histogram is keyed by.
        let key = ((u16::from(r) >> 3) << 10) | ((u16::from(g) >> 3) << 5) | (u16::from(b) >> 3);
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

    /// The regression colours: a page body's cream against a scanned
    /// margin's maroon.
    const CREAM: [u8; 3] = [0xfa, 0xf4, 0xe8];
    const MAROON: [u8; 3] = [0x80, 0x00, 0x00];

    /// A `w × h` RGBA buffer: `fill` paints every pixel; `paint` and `ring`
    /// overwrite regions.
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

    /// Paint a `t`-deep band along all four sides of a `w`-wide buffer —
    /// the shape of a scanned page's coloured margin.
    fn ring(buf: &mut [u8], w: usize, t: usize, colour: [u8; 3]) {
        let rows = buf.len() / (w * 4);
        for y in 0..rows {
            for x in 0..w {
                if y < t || y + t >= rows || x < t || x + t >= w {
                    let i = (y * w + x) * 4;
                    buf[i] = colour[0];
                    buf[i + 1] = colour[1];
                    buf[i + 2] = colour[2];
                }
            }
        }
    }

    /// The colour a test constant detects as.
    fn rgb(c: [u8; 3]) -> Rgb {
        Rgb::new(c[0], c[1], c[2])
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
        // Whole-page detection sees more photo than cream and calls the
        // photo the paper; edge detection reads only the margin bands.
        let mut buf = frame(40, 40, [0x20, 0x20, 0x30]);
        ring(&mut buf, 40, 4, CREAM);
        let mut whole = PaperDetector::new();
        whole.feed(PaperArea::WholePage, 40, 40, &buf, 4);
        assert_eq!(whole.dominant(PAPER_SHARE), Some(Rgb::new(0x20, 0x20, 0x30)));

        let mut edges = PaperDetector::new();
        edges.feed(PaperArea::Edges, 40, 40, &buf, 4);
        assert_eq!(edges.dominant(PAPER_SHARE), Some(rgb(CREAM)));
        // A 4px band on each of the four sides of a 40px page: the 32×32
        // middle never votes.
        assert_eq!(edges.pixels(), 40 * 40 - 32 * 32);
    }

    #[test]
    fn an_oversized_edge_strip_stays_a_margin_not_the_whole_page() {
        let mut d = PaperDetector::new();
        // 40×40: a cream centre that dominates by area, a maroon 5px margin.
        let mut buf = frame(40, 40, CREAM);
        ring(&mut buf, 40, 5, MAROON);
        let fed = d.feed(PaperArea::Edges, 40, 40, &buf, 999);
        // The strips cap at a quarter of the shorter axis (10px), never the
        // half-that-tiles-everything clamp they replace.
        assert_eq!(fed, 40 * 40 - 20 * 20);
        // The maroon ring owns the counted bands even though the cream
        // centre owns the page: Edges reads the margin, not the middle.
        assert_eq!(d.dominant(PAPER_SHARE), Some(rgb(MAROON)));
    }

    #[test]
    fn edges_and_whole_page_disagree_when_the_centre_dominates() {
        // The shape of the regression document: a centre colour that wins
        // the page by area, wrapped in a different-coloured margin. The two
        // areas must answer differently — under the old clamp an oversized
        // edge width made Edges ask the whole page's question and both
        // answers collapsed into the centre colour.
        let mut buf = frame(40, 40, CREAM);
        ring(&mut buf, 40, 5, MAROON);
        let mut whole = PaperDetector::new();
        whole.feed(PaperArea::WholePage, 40, 40, &buf, 4);
        let mut edges = PaperDetector::new();
        edges.feed(PaperArea::Edges, 40, 40, &buf, 999);
        assert_eq!(whole.dominant(PAPER_SHARE), Some(rgb(CREAM)));
        assert_eq!(edges.dominant(PAPER_SHARE), Some(rgb(MAROON)));
    }

    #[test]
    fn a_margin_a_sixteenth_off_the_body_keeps_its_own_colour() {
        // THE regression, in the dark: a sheet whose margins sit one 4-bit
        // step off its body (#10 vs #1e). The old 16-wide cell swallowed
        // both, so WholePage published their mixed mean and Edges published
        // a mean a few units off it — the same mud to the eye, matching
        // neither surface. At the 5-bit grain the two stay separate buckets:
        // the body owns the page, the margin owns the bands.
        let mut buf = frame(40, 40, [0x1e, 0x1e, 0x1e]);
        ring(&mut buf, 40, 4, [0x10, 0x10, 0x10]);
        let mut whole = PaperDetector::new();
        whole.feed(PaperArea::WholePage, 40, 40, &buf, 4);
        assert_eq!(whole.dominant(PAPER_SHARE), Some(Rgb::new(0x1e, 0x1e, 0x1e)));

        let mut edges = PaperDetector::new();
        edges.feed(PaperArea::Edges, 40, 40, &buf, 4);
        assert_eq!(edges.dominant(PAPER_SHARE), Some(Rgb::new(0x10, 0x10, 0x10)));
    }

    #[test]
    fn feeds_pool_across_pages() {
        // Two frames: page 1 mostly cream with ink text, page 2 all cream.
        // Pooled, cream owns the pair even though page 1's own histogram
        // leans the other way.
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
