/
/
!

T
h
e

p
e
r
-
p
a
g
e

p
a
l
e
t
t
e

a
n
d

t
h
e

i
n
t
e
r
p
o
l
a
t
i
o
n

t
h
a
t

w
a
l
k
s

i
t
.

use std::collections::BTreeMap;

use crate::color::{Rgb, lerp};

/// Known paper colours, keyed by 1-based page number.
#[derive(Debug, Default, Clone)]
pub struct PagePalette {
    pages: BTreeMap<u32, Rgb>,
}

impl PagePalette {
    pub fn new() -> Self {
        Self::default()
    }

    /
    /
    /

    R
    e
    m
    e
    m
    b
    e
    r

    p
    a
    g
    e

    `
    p
    a
    g
    e
    `
    '
    s

    c
    o
    l
    o
    u
    r
    ;

    a

    r
    e
    -
    d
    e
    t
    e
    c
    t
    i
    o
    n

    o
    v
    e
    r
    w
    r
    i
    t
    e
    s
    .
    pub fn set(&mut self, page: u32, colour: Rgb) {
        self.pages.insert(page, colour);
    }

    pub fn get(&self, page: u32) -> Option<Rgb> {
        self.pages.get(&page).copied()
    }

    pub fn contains(&self, page: u32) -> bool {
        self.pages.contains_key(&page)
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    /
    /
    /

    T
    h
    e

    c
    o
    l
    o
    u
    r

    a
    t

    a

    f
    r
    a
    c
    t
    i
    o
    n
    a
    l

    p
    a
    g
    e

    `
    p
    o
    s
    i
    t
    i
    o
    n
    `

    (
    1
    -
    b
    a
    s
    e
    d
    )
    .
    pub fn colour_at(&self, position: f64) -> Option<Rgb> {
        if self.pages.is_empty() || !position.is_finite() {
            return None;
        }
        let first = *self.pages.keys().next()? as f64;
        let last = *self.pages.keys().next_back()? as f64;
        let pos = position.clamp(first, last);

        /
        /

        T
        h
        e

        n
        e
        a
        r
        e
        s
        t

        k
        n
        o
        w
        n

        p
        a
        g
        e

        a
        t

        o
        r

        b
        e
        l
        o
        w

        `
        p
        o
        s
        `
        ,

        a
        n
        d

        t
        h
        e

        o
        n
        e

        a
        b
        o
        v
        e
        .
        let floor = pos.floor() as u32;
        let (lo_key, lo_colour) = self.pages.range(..=floor).next_back()?;
        let hi = self.pages.range(floor + 1..).next();
        let Some((hi_key, hi_colour)) = hi else {
            return Some(*lo_colour); // at (or past) the last known page
        };
        let span = f64::from(*hi_key - *lo_key);
        if span <= f64::EPSILON {
            return Some(*lo_colour);
        }
        let t = (pos - f64::from(*lo_key)) / span;
        Some(lerp(*lo_colour, *hi_colour, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREAM: Rgb = Rgb::new(0xfa, 0xf4, 0xe8);
    const INK: Rgb = Rgb::new(0x40, 0x40, 0x40);
    const WHITE: Rgb = Rgb::new(0xff, 0xff, 0xff);

    #[test]
    fn an_empty_palette_has_no_opinion() {
        assert_eq!(PagePalette::new().colour_at(1.0), None);
        assert!(PagePalette::new().is_empty());
    }

    #[test]
    fn resting_on_a_page_is_that_pages_colour() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.set(2, INK);
        assert_eq!(p.colour_at(1.0), Some(CREAM));
        assert_eq!(p.colour_at(2.0), Some(INK));
    }

    #[test]
    fn the_midpoint_of_two_pages_blends_their_papers() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.set(2, WHITE);
        /
        /

        T
        h
        e

        a
        s
        s
        e
        r
        t
        i
        o
        n

        c
        o
        m
        p
        a
        r
        e
        s

        a
        g
        a
        i
        n
        s
        t

        t
        h
        e

        s
        a
        m
        e

        l
        e
        r
        p
        .
        let mid = p.colour_at(1.5).unwrap();
        assert_eq!(mid, lerp(CREAM, WHITE, 0.5));
    }

    #[test]
    fn a_weighted_position_carries_both_pages_shares() {
        /
        /

        4
        0
        %

        p
        a
        g
        e

        1

        +

        6
        0
        %

        p
        a
        g
        e

        2
        :

        p
        o
        s
        i
        t
        i
        o
        n

        1
        .
        6
        ,

        6
        0
        %

        o
        f

        p
        a
        g
        e

        2
        .
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.set(2, WHITE);
        assert_eq!(p.colour_at(1.6), Some(lerp(CREAM, WHITE, 0.6)));
        assert_eq!(p.colour_at(1.25), Some(lerp(CREAM, WHITE, 0.25)));
    }

    #[test]
    fn positions_clamp_to_the_known_span() {
        let mut p = PagePalette::new();
        p.set(3, CREAM);
        p.set(5, INK);
        assert_eq!(p.colour_at(0.0), Some(CREAM)); // below the first page
        assert_eq!(p.colour_at(9.0), Some(INK)); // past the last page
        assert_eq!(p.colour_at(3.0), Some(CREAM));
    }

    #[test]
    fn an_unknown_page_blends_across_the_gap() {
        /
        /

        P
        a
        g
        e

        4

        n
        e
        v
        e
        r

        r
        e
        s
        o
        l
        v
        e
        d
        ;

        3
        .
        5

        i
        s

        h
        a
        l
        f

        w
        a
        y

        b
        e
        t
        w
        e
        e
        n

        3

        a
        n
        d

        5
        .
        let mut p = PagePalette::new();
        p.set(3, CREAM);
        p.set(5, INK);
        assert_eq!(p.colour_at(3.5), Some(lerp(CREAM, INK, 0.25)));
        assert_eq!(p.colour_at(4.0), Some(lerp(CREAM, INK, 0.5)));
        assert_eq!(p.colour_at(4.9), Some(lerp(CREAM, INK, 0.95)));
    }

    #[test]
    fn a_single_known_page_holds_its_colour_everywhere() {
        let mut p = PagePalette::new();
        p.set(7, INK);
        for pos in [0.0, 1.0, 6.9, 7.0, 7.5, 100.0] {
            assert_eq!(p.colour_at(pos), Some(INK), "pos {pos}");
        }
    }

    #[test]
    fn non_finite_positions_hold_their_tongue() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        assert_eq!(p.colour_at(f64::NAN), None);
        assert_eq!(p.colour_at(f64::INFINITY), None);
    }

    #[test]
    fn clear_empties_the_palette() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.clear();
        assert!(p.is_empty());
        assert_eq!(p.colour_at(1.0), None);
    }
}
