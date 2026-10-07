//! Dominant-colour detection: quantise pixels into 5-bit buckets.
use std::collections::HashMap;

use crate::color::Rgb;
use crate::config::PaperArea;

/// The share of sampled pixels a paper colour must own before it is published.
pub const PAPER_SHARE: f64 = 0.1;

#[derive(Default)]
struct Bucket {
    n: u64,
    r: u64,
    g: u64,
    b: u64,
}

/// A running histogram of raw RGBA pixels, keyed by bucket.
#[derive(Default)]
pub struct PaperDetector {
    buckets: HashMap<u16, Bucket>,
    pixels: u64,
}

/// Round a non-negative mean to the nearest channel value.
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

    /// Feed one frame's pixels, honouring the configured area.
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

    fn feed_rgba(&mut self, rgba: &[u8]) -> usize {
        for px in rgba.as_chunks::<4>().0 {
            self.count(px[0], px[1], px[2]);
        }
        rgba.len() / 4
    }

    /// Count only the margin bands.
    fn feed_edges(&mut self, width: usize, height: usize, rgba: &[u8], edge_width: usize) -> usize {
        // At half the extent each, opposing strips tile the axis whole.
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

    /// The dominant colour, if one bucket owns `min_share` of the pixels.
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

    /// Total pixels counted across every feed.
    #[cfg(test)]
    fn pixels(&self) -> u64 {
        self.pixels
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.pixels == 0
    }

    fn count(&mut self, r: u8, g: u8, b: u8) {
        // Five bits a channel: at four, a low-contrast margin merges
        // into its body's bucket.
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

    /// The colours the regression cases build from: a body's cream, a
    /// scanned margin's maroon.
    const CREAM: [u8; 3] = [0xfa, 0xf4, 0xe8];
    const MAROON: [u8; 3] = [0x80, 0x00, 0x00];

    /// A `w × h` RGBA buffer; `paint` and `ring` overwrite regions.
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

    /// Paint a `t`-deep band along all four sides of a buffer.
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

    /// A test constant's colour as an `Rgb`.
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
        // A 0.5 mean is where truncation and rounding disagree.
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
        // A scanned page: cream margins, a dark photo in the middle.
        let mut buf = frame(40, 40, [0x20, 0x20, 0x30]);
        ring(&mut buf, 40, 4, CREAM);
        let mut whole = PaperDetector::new();
        whole.feed(PaperArea::WholePage, 40, 40, &buf, 4);
        assert_eq!(
            whole.dominant(PAPER_SHARE),
            Some(Rgb::new(0x20, 0x20, 0x30))
        );

        let mut edges = PaperDetector::new();
        edges.feed(PaperArea::Edges, 40, 40, &buf, 4);
        assert_eq!(edges.dominant(PAPER_SHARE), Some(rgb(CREAM)));
        // A 4px band each side of a 40px page: the middle never votes.
        assert_eq!(edges.pixels(), 40 * 40 - 32 * 32);
    }

    #[test]
    fn an_oversized_edge_strip_stays_a_margin_not_the_whole_page() {
        let mut d = PaperDetector::new();
        // 40×40: a cream centre that dominates by area, a maroon 5px margin.
        let mut buf = frame(40, 40, CREAM);
        ring(&mut buf, 40, 5, MAROON);
        let fed = d.feed(PaperArea::Edges, 40, 40, &buf, 999);
        // The strips cap at a quarter of the shorter axis.
        assert_eq!(fed, 40 * 40 - 20 * 20);
        // The maroon ring owns the counted bands, not the centre.
        assert_eq!(d.dominant(PAPER_SHARE), Some(rgb(MAROON)));
    }

    #[test]
    fn edges_and_whole_page_disagree_when_the_centre_dominates() {
        // The regression document's shape: a centre colour that wins.
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
        // A dark sheet whose margin sits one 4-bit step off the body.
        let mut buf = frame(40, 40, [0x1e, 0x1e, 0x1e]);
        ring(&mut buf, 40, 4, [0x10, 0x10, 0x10]);
        let mut whole = PaperDetector::new();
        whole.feed(PaperArea::WholePage, 40, 40, &buf, 4);
        assert_eq!(
            whole.dominant(PAPER_SHARE),
            Some(Rgb::new(0x1e, 0x1e, 0x1e))
        );

        let mut edges = PaperDetector::new();
        edges.feed(PaperArea::Edges, 40, 40, &buf, 4);
        assert_eq!(
            edges.dominant(PAPER_SHARE),
            Some(Rgb::new(0x10, 0x10, 0x10))
        );
    }

    #[test]
    fn feeds_pool_across_pages() {
        // Two frames: page 1 mostly cream with ink, page 2 all cream.
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
}
