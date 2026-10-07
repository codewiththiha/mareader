/
/
!

D
o
m
i
n
a
n
t
-
c
o
l
o
u
r

d
e
t
e
c
t
i
o
n
:

q
u
a
n
t
i
s
e

p
i
x
e
l
s

i
n
t
o

5
-
b
i
t

b
u
c
k
e
t
s
.

use std::collections::HashMap;

use crate::color::Rgb;
use crate::config::PaperArea;

/
/
/

A

b
o
o
k
'
s

p
a
p
e
r

m
u
s
t

o
w
n

a
t

l
e
a
s
t

t
h
i
s

s
h
a
r
e

o
f

t
h
e

p
i
x
e
l
s
.
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

/
/
/

R
o
u
n
d

a

n
o
n
-
n
e
g
a
t
i
v
e

m
e
a
n

t
o

t
h
e

n
e
a
r
e
s
t

c
h
a
n
n
e
l

v
a
l
u
e
.
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

    /
    /
    /

    F
    e
    e
    d

    o
    n
    e

    f
    r
    a
    m
    e
    '
    s

    p
    i
    x
    e
    l
    s
    ,

    h
    o
    n
    o
    u
    r
    i
    n
    g

    t
    h
    e

    c
    o
    n
    f
    i
    g
    u
    r
    e
    d

    a
    r
    e
    a
    .
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

    /
    /
    /

    C
    o
    u
    n
    t

    o
    n
    l
    y

    t
    h
    e

    m
    a
    r
    g
    i
    n

    b
    a
    n
    d
    s
    .
    fn feed_edges(&mut self, width: usize, height: usize, rgba: &[u8], edge_width: usize) -> usize {
        /
        /

        T
        w
        o

        o
        p
        p
        o
        s
        i
        n
        g

        s
        t
        r
        i
        p
        s

        o
        f

        H
        A
        L
        F

        t
        h
        e

        e
        x
        t
        e
        n
        t

        e
        a
        c
        h

        t
        i
        l
        e

        t
        h
        e

        w
        h
        o
        l
        e

        a
        x
        i
        s
        .
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

    /
    /
    /

    T
    h
    e

    d
    o
    m
    i
    n
    a
    n
    t

    c
    o
    l
    o
    u
    r
    ,

    i
    f

    o
    n
    e

    b
    u
    c
    k
    e
    t

    o
    w
    n
    s

    `
    m
    i
    n
    _
    s
    h
    a
    r
    e
    `

    o
    f

    t
    h
    e

    p
    i
    x
    e
    l
    s
    .
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

    /
    /
    /

    T
    o
    t
    a
    l

    p
    i
    x
    e
    l
    s

    c
    o
    u
    n
    t
    e
    d

    a
    c
    r
    o
    s
    s

    e
    v
    e
    r
    y

    f
    e
    e
    d
    .
    fn pixels(&self) -> u64 {
        self.pixels
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.pixels == 0
    }

    fn count(&mut self, r: u8, g: u8, b: u8) {
        /
        /

        F
        i
        v
        e

        b
        i
        t
        s

        p
        e
        r

        c
        h
        a
        n
        n
        e
        l
        ;

        f
        o
        u
        r

        m
        e
        r
        g
        e
        d

        m
        a
        r
        g
        i
        n
        s

        i
        n
        t
        o

        b
        o
        d
        i
        e
        s
        .
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

    /
    /
    /

    P
    a
    i
    n
    t

    a

    `
    t
    `
    -
    d
    e
    e
    p

    b
    a
    n
    d

    a
    l
    o
    n
    g

    a
    l
    l

    f
    o
    u
    r

    s
    i
    d
    e
    s

    o
    f

    a

    b
    u
    f
    f
    e
    r
    .
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
        /
        /

        A

        0
        .
        5

        m
        e
        a
        n

        e
        x
        p
        o
        s
        e
        s

        t
        h
        e

        o
        l
        d

        t
        r
        u
        n
        c
        a
        t
        i
        o
        n
        .
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
        /
        /

        A

        s
        c
        a
        n
        n
        e
        d

        p
        a
        g
        e
        :

        c
        r
        e
        a
        m

        m
        a
        r
        g
        i
        n
        s
        ,

        a

        d
        a
        r
        k

        p
        h
        o
        t
        o

        i
        n

        t
        h
        e

        m
        i
        d
        d
        l
        e
        .
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
        /
        /

        A

        4
        p
        x

        b
        a
        n
        d

        e
        a
        c
        h

        s
        i
        d
        e

        o
        f

        a

        4
        0
        p
        x

        p
        a
        g
        e
        :

        t
        h
        e

        m
        i
        d
        d
        l
        e

        n
        e
        v
        e
        r

        v
        o
        t
        e
        s
        .
        assert_eq!(edges.pixels(), 40 * 40 - 32 * 32);
    }

    #[test]
    fn an_oversized_edge_strip_stays_a_margin_not_the_whole_page() {
        let mut d = PaperDetector::new();
        // 40×40: a cream centre that dominates by area, a maroon 5px margin.
        let mut buf = frame(40, 40, CREAM);
        ring(&mut buf, 40, 5, MAROON);
        let fed = d.feed(PaperArea::Edges, 40, 40, &buf, 999);
        /
        /

        T
        h
        e

        s
        t
        r
        i
        p
        s

        c
        a
        p

        a
        t

        a

        q
        u
        a
        r
        t
        e
        r

        o
        f

        t
        h
        e

        s
        h
        o
        r
        t
        e
        r

        a
        x
        i
        s
        .
        assert_eq!(fed, 40 * 40 - 20 * 20);
        /
        /

        T
        h
        e

        m
        a
        r
        o
        o
        n

        r
        i
        n
        g

        o
        w
        n
        s

        t
        h
        e

        c
        o
        u
        n
        t
        e
        d

        b
        a
        n
        d
        s
        ,

        n
        o
        t

        t
        h
        e

        c
        e
        n
        t
        r
        e
        .
        assert_eq!(d.dominant(PAPER_SHARE), Some(rgb(MAROON)));
    }

    #[test]
    fn edges_and_whole_page_disagree_when_the_centre_dominates() {
        /
        /

        T
        h
        e

        r
        e
        g
        r
        e
        s
        s
        i
        o
        n

        d
        o
        c
        u
        m
        e
        n
        t
        '
        s

        s
        h
        a
        p
        e
        :

        a

        c
        e
        n
        t
        r
        e

        c
        o
        l
        o
        u
        r

        t
        h
        a
        t

        w
        i
        n
        s
        .
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
        /
        /

        T
        H
        E

        r
        e
        g
        r
        e
        s
        s
        i
        o
        n
        ,

        i
        n

        t
        h
        e

        d
        a
        r
        k
        :

        m
        a
        r
        g
        i
        n
        s

        o
        n
        e

        4
        -
        b
        i
        t

        s
        t
        e
        p

        o
        f
        f

        t
        h
        e

        b
        o
        d
        y
        .
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
        /
        /

        T
        w
        o

        f
        r
        a
        m
        e
        s
        :

        p
        a
        g
        e

        1

        m
        o
        s
        t
        l
        y

        c
        r
        e
        a
        m

        w
        i
        t
        h

        i
        n
        k
        ,

        p
        a
        g
        e

        2

        a
        l
        l

        c
        r
        e
        a
        m
        .
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
