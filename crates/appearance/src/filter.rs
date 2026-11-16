//! The page colour pipeline as matrices; the GPU pass is tested against it.

use crate::model::{Appearance, BaseMode};
use crate::pattern::Pattern;

/// The page shader's uniform array has exactly this many slots.
pub const MAX_FILTER_OPS: usize = 7;

/// One CSS filter function, with the Filter Effects spec semantics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterOp {
    Invert(f32),
    /// Degrees.
    HueRotate(f32),
    Saturate(f32),
    Brightness(f32),
    Contrast(f32),
    Sepia(f32),
    /// Levels: this much and darker turns black; the rest stretch to white.
    BlackPoint(f32),
}

/// Rows are output r, g, b; columns are input r, g, b and a constant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorMatrix(pub [[f32; 4]; 3]);

impl ColorMatrix {
    fn linear(m: [[f32; 3]; 3]) -> Self {
        let [r, g, b] = m;
        Self([
            [r[0], r[1], r[2], 0.0],
            [g[0], g[1], g[2], 0.0],
            [b[0], b[1], b[2], 0.0],
        ])
    }

    fn scale_offset(scale: f32, offset: f32) -> Self {
        Self([
            [scale, 0.0, 0.0, offset],
            [0.0, scale, 0.0, offset],
            [0.0, 0.0, scale, offset],
        ])
    }

    /// `self` after `first` as one matrix; `first` must stay in range.
    fn after(self, first: Self) -> Self {
        let [b0, b1, b2] = first.0;
        Self(self.0.map(|[x, y, z, w]| {
            let mut row = [0.0, 0.0, 0.0, w];
            for (k, b) in [(x, b0), (y, b1), (z, b2)] {
                for (r, v) in row.iter_mut().zip(b) {
                    *r += k * v;
                }
            }
            row
        }))
    }

    /// Whether every in-range colour stays in range, so no clamp follows it.
    fn keeps_range(&self) -> bool {
        self.0.iter().all(|&[x, y, z, w]| {
            let low = w + x.min(0.0) + y.min(0.0) + z.min(0.0);
            let high = w + x.max(0.0) + y.max(0.0) + z.max(0.0);
            low >= -1e-6 && high <= 1.0 + 1e-6
        })
    }

    /// Applies the matrix and clamps, as CSS does after every function.
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        self.0
            .map(|row| (row[0] * c[0] + row[1] * c[1] + row[2] * c[2] + row[3]).clamp(0.0, 1.0))
    }
}

impl FilterOp {
    pub fn matrix(self) -> ColorMatrix {
        match self {
            Self::Invert(a) => {
                let a = a.clamp(0.0, 1.0);
                ColorMatrix::scale_offset(1.0 - 2.0 * a, a)
            }
            Self::Brightness(b) => ColorMatrix::scale_offset(b.max(0.0), 0.0),
            Self::Contrast(c) => {
                let c = c.max(0.0);
                ColorMatrix::scale_offset(c, 0.5 - 0.5 * c)
            }
            Self::Saturate(s) => {
                let s = s.max(0.0);
                ColorMatrix::linear([
                    [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
                    [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
                    [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
                ])
            }
            Self::HueRotate(deg) => {
                let (sin, cos) = deg.to_radians().sin_cos();
                ColorMatrix::linear([
                    [
                        0.213 + cos * 0.787 - sin * 0.213,
                        0.715 - cos * 0.715 - sin * 0.715,
                        0.072 - cos * 0.072 + sin * 0.928,
                    ],
                    [
                        0.213 - cos * 0.213 + sin * 0.143,
                        0.715 + cos * 0.285 + sin * 0.140,
                        0.072 - cos * 0.072 - sin * 0.283,
                    ],
                    [
                        0.213 - cos * 0.213 - sin * 0.787,
                        0.715 - cos * 0.715 + sin * 0.715,
                        0.072 + cos * 0.928 + sin * 0.072,
                    ],
                ])
            }
            Self::BlackPoint(k) => {
                let k = k.clamp(0.0, 0.9);
                ColorMatrix::scale_offset(1.0 / (1.0 - k), -k / (1.0 - k))
            }
            Self::Sepia(a) => {
                let k = 1.0 - a.clamp(0.0, 1.0);
                ColorMatrix::linear([
                    [0.393 + 0.607 * k, 0.769 - 0.769 * k, 0.189 - 0.189 * k],
                    [0.349 - 0.349 * k, 0.686 + 0.314 * k, 0.168 - 0.168 * k],
                    [0.272 - 0.272 * k, 0.534 - 0.534 * k, 0.131 + 0.869 * k],
                ])
            }
        }
    }
}

/// Runs a chain on one sRGB-encoded colour, clamping between steps.
pub fn apply_filter(ops: &[FilterOp], c: [f32; 3]) -> [f32; 3] {
    ops.iter().fold(c, |c, op| op.matrix().apply(c))
}

/// How the themed page composites over its paper and texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Multiply,
    Screen,
    SoftLight,
}

impl Blend {
    /// W3C compositing formulas; `b` is the backdrop, `s` the page.
    pub fn apply(self, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
        let one = |b: f32, s: f32| match self {
            Self::Multiply => b * s,
            Self::Screen => b + s - b * s,
            Self::SoftLight => {
                if s <= 0.5 {
                    b - (1.0 - 2.0 * s) * b * (1.0 - b)
                } else {
                    let d = if b <= 0.25 {
                        ((16.0 * b - 12.0) * b + 4.0) * b
                    } else {
                        b.sqrt()
                    };
                    b + (2.0 * s - 1.0) * (d - b)
                }
            }
        };
        [one(b[0], s[0]), one(b[1], s[1]), one(b[2], s[2])]
    }
}

impl Appearance {
    /// The page chain, in mareader's order: base first, tint after.
    pub fn page_filter(&self) -> Vec<FilterOp> {
        let mut ops = match self.base {
            BaseMode::Light => Vec::new(),
            BaseMode::Dark => vec![
                FilterOp::Invert(0.92),
                FilterOp::HueRotate(180.0),
                FilterOp::Saturate(0.85),
                FilterOp::Brightness(1.02),
            ],
            BaseMode::Dim => vec![
                FilterOp::Brightness(0.8),
                FilterOp::Saturate(0.75),
                FilterOp::Contrast(0.9),
            ],
            // Smart invert: hues come back, near-white paper becomes black.
            BaseMode::Black => vec![
                FilterOp::Invert(1.0),
                FilterOp::BlackPoint(BLACK_POINT),
                FilterOp::HueRotate(180.0),
            ],
        };
        if self.has_tint() {
            let t = self.tint_amount() as f32;
            ops.push(FilterOp::Sepia((t * 0.55).clamp(0.0, 0.55)));
            ops.push(FilterOp::Saturate(1.0 + t * 0.6));
            // sepia() lands near 34°, so hue is measured from there.
            ops.push(FilterOp::HueRotate(f32::from(self.tint_hue) - 34.0));
        }
        ops
    }

    /// Everything the page pass needs to theme a page.
    pub fn page_tone(&self) -> PageTone {
        let (matrices, count) = folded(&self.page_filter());
        let p = self.ui_tokens().paper;
        PageTone {
            matrices,
            count,
            blend: Some(self.page_blend()),
            paper: [p.0, p.1, p.2].map(|c| f32::from(c) / 255.0),
            ink: None,
            pattern: self.pattern(),
        }
    }

    /// A book's page: its palette's paper and ink, as mareader's text pages.
    pub fn text_tone(&self) -> PageTone {
        let p = self.text_palette();
        PageTone {
            ink: Some(p.ink.to_unit()),
            paper: p.paper.to_unit(),
            ..self.page_tone().flat()
        }
    }

    pub fn page_blend(&self) -> Blend {
        match self.base {
            BaseMode::Light => Blend::Multiply,
            BaseMode::Dark => Blend::Screen,
            BaseMode::Dim => Blend::SoftLight,
            // Screen over black paper leaves the page as the chain made it.
            BaseMode::Black => Blend::Screen,
        }
    }
}

/// Paper this light, after inverting, is black: scans' grey paper included.
const BLACK_POINT: f32 = 0.15;

/// The chain as few matrices: a step joins one whose output needs no clamp.
fn folded(ops: &[FilterOp]) -> ([ColorMatrix; MAX_FILTER_OPS], usize) {
    let mut out = [ColorMatrix::linear(IDENTITY); MAX_FILTER_OPS];
    let mut count: usize = 0;
    for op in ops {
        let m = op.matrix();
        let last = count.checked_sub(1).and_then(|i| out.get_mut(i));
        match last {
            Some(prev) if prev.keeps_range() => *prev = m.after(*prev),
            _ => {
                let Some(slot) = out.get_mut(count) else {
                    break;
                };
                *slot = m;
                count += 1;
            }
        }
    }
    (out, count)
}

/// `a` moved toward `b` by `t`.
fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let mut out = a;
    for (o, (x, y)) in out.iter_mut().zip(a.into_iter().zip(b)) {
        *o = x + (y - x) * t;
    }
    out
}

/// Rec. 709 luma of a grey-on-white raster colour.
fn luma(c: [f32; 3]) -> f32 {
    (0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]).clamp(0.0, 1.0)
}

/// How far a colour's grey may move toward the middle, making room for hue.
const HUE_SHIFT: f32 = 0.1;
/// Themes with less contrast than this, as dim, mute hues to match.
const FULL_HUE: f32 = 0.5;

/// A book's colour: its grey mapped to the theme, its hue kept as fits.
fn recolour(ink: [f32; 3], paper: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let g = luma(c);
    let mute = ((luma(paper) - luma(ink)).abs() / FULL_HUE).clamp(0.25, 1.0);
    let off = c.map(|v| (v - g) * mute);
    let grey = mix(ink, paper, g);
    let lo = off
        .iter()
        .zip(grey)
        .map(|(o, b)| -o - b)
        .fold(f32::MIN, f32::max);
    let hi = off
        .iter()
        .zip(grey)
        .map(|(o, b)| 1.0 - o - b)
        .fold(f32::MAX, f32::min);
    let shift = (0.0f32.max(lo).min(hi)).clamp(-HUE_SHIFT, HUE_SHIFT);
    let base = grey.map(|b| b + shift);
    let room = off.into_iter().zip(base).fold(1.0f32, |k, (o, b)| match o {
        o if o > 1e-4 => k.min((1.0 - b) / o),
        o if o < -1e-4 => k.min(b / -o),
        _ => k,
    });
    let room = room.clamp(0.0, 1.0);
    let mut out = base;
    for (v, o) in out.iter_mut().zip(off) {
        *v = (*v + o * room).clamp(0.0, 1.0);
    }
    out
}

const IDENTITY: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A themed page: its filter chain, then blended over the paper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageTone {
    /// The chain; slots past `count` are unused.
    pub matrices: [ColorMatrix; MAX_FILTER_OPS],
    pub count: usize,
    /// None draws the raster as it is: flat fills, or no theme.
    pub blend: Option<Blend>,
    /// sRGB-encoded, 0..1.
    pub paper: [f32; 3],
    /// A book's ink: greys map from it to the paper, with no filter.
    pub ink: Option<[f32; 3]>,
    /// The texture laid over the themed page.
    pub pattern: Pattern,
}

impl PageTone {
    /// Leaves colours alone.
    pub const NONE: Self = Self {
        matrices: [ColorMatrix([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]); MAX_FILTER_OPS],
        count: 0,
        blend: None,
        paper: [1.0; 3],
        ink: None,
        pattern: Pattern::NONE,
    };

    /// One opaque page colour themed: the oracle for the shader.
    pub fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        let Some(blend) = self.blend else {
            return c;
        };
        if let Some(ink) = self.ink {
            return recolour(ink, self.paper, c);
        }
        let chain = self.matrices.iter().take(self.count);
        let s = chain.fold(c, |c, m| m.apply(c));
        blend.apply(self.paper, s)
    }

    /// mareader's ink dial: greys lightened, keeping `keep` (0 to 1) of
    /// the ink.
    pub fn inked(mut self, keep: f32) -> Self {
        let keep = keep.clamp(0.0, 1.0);
        if keep >= 1.0 || self.blend.is_none() {
            return self;
        }
        if let Some(ink) = &mut self.ink {
            *ink = mix(*ink, self.paper, 1.0 - keep);
            return self;
        }
        // A mix toward white stays in range, so it folds into the first.
        let ink = ColorMatrix::scale_offset(keep, 1.0 - keep);
        if let Some(m) = self.matrices.first_mut() {
            *m = if self.count == 0 { ink } else { m.after(ink) };
        }
        self.count = self.count.max(1);
        self
    }

    /// No filter chain: a book's tone keeps only blend, paper and pattern.
    fn flat(self) -> Self {
        Self {
            matrices: Self::NONE.matrices,
            count: 0,
            ..self
        }
    }

    /// Spares a book's pictures the ink: its filter and blend, its paper.
    pub fn for_pictures(self, page: &Self) -> Self {
        Self {
            paper: page.paper,
            ..self
        }
    }

    /// A blank page themed: what a page shows before it is drawn.
    pub fn blank(&self) -> [f32; 3] {
        self.apply([1.0; 3])
    }

    /// Over white, shows what a page of `paper` shows, pattern included.
    pub fn backdrop(&self, paper: [f32; 3]) -> Self {
        Self {
            count: 0,
            blend: Some(Blend::Multiply),
            paper: self.apply(paper),
            pattern: if self.blend.is_some() {
                self.pattern
            } else {
                Pattern::NONE
            },
            ..Self::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.004)
    }

    #[test]
    fn plain_light_has_no_filter() {
        assert!(Appearance::default().page_filter().is_empty());
    }

    #[test]
    fn the_chain_never_exceeds_the_shader_slots() {
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                tint_strength: 100,
                ..Default::default()
            };
            assert!(a.page_filter().len() <= MAX_FILTER_OPS);
        }
    }

    #[test]
    fn black_turns_paper_pitch_black_and_text_white() {
        let a = Appearance {
            base: BaseMode::Black,
            ..Default::default()
        };
        let tone = a.page_tone();
        assert_eq!(tone.paper, [0.0; 3]);
        // White and a scan's grey paper alike, exactly black.
        for paper in [[1.0; 3], [0.93, 0.92, 0.88]] {
            assert_eq!(tone.apply(paper), [0.0; 3], "{paper:?}");
        }
        assert!(
            close(tone.apply([0.0; 3]), [1.0; 3]),
            "black text turns white"
        );
        let [r, g, b] = tone.apply([0.8, 0.1, 0.1]);
        assert!(r > g + 0.2 && r > b + 0.2, "red stays red: {r} {g} {b}");
    }

    #[test]
    fn folding_the_chain_changes_no_colour() {
        let colours = [
            [0.0; 3],
            [1.0; 3],
            [0.5; 3],
            [0.8, 0.1, 0.1],
            [0.1, 0.6, 0.9],
        ];
        for base in BaseMode::ALL {
            for tint_strength in [0, 25, 100] {
                let a = Appearance {
                    base,
                    tint_strength,
                    tint_hue: 200,
                    ..Default::default()
                };
                let (tone, ops) = (a.page_tone(), a.page_filter());
                for c in colours {
                    let want = a.page_blend().apply(tone.paper, apply_filter(&ops, c));
                    let got = tone.apply(c);
                    assert!(close(got, want), "{base:?} {tint_strength} {c:?}");
                }
            }
        }
        let count = |base| {
            let a = Appearance {
                base,
                ..Default::default()
            };
            a.page_tone().count
        };
        assert_eq!(count(BaseMode::Dim), 1, "three range-keeping steps fold");
        assert_eq!(count(BaseMode::Black), 2);
    }

    #[test]
    fn dark_turns_white_paper_near_black() {
        let a = Appearance {
            base: BaseMode::Dark,
            ..Default::default()
        };
        let out = apply_filter(&a.page_filter(), [1.0, 1.0, 1.0]);
        assert!(out.iter().all(|c| *c < 0.1), "{out:?}");
    }

    #[test]
    fn the_ink_dial_lightens_ink_toward_the_paper_in_every_mode() {
        for base in BaseMode::ALL {
            for tint_strength in [0, 100] {
                let tone = Appearance {
                    base,
                    tint_strength,
                    ..Default::default()
                }
                .page_tone();
                let half = tone.inked(0.5);
                assert_eq!(tone.inked(1.0), tone);
                assert!(close(half.blank(), tone.blank()), "{base:?}");
                for c in [[0.0; 3], [0.33, 0.33, 0.33], [0.1, 0.6, 0.3]] {
                    let mixed = c.map(|v| 0.5 + 0.5 * v);
                    assert!(close(half.apply(c), tone.apply(mixed)), "{base:?} {c:?}");
                }
                // No ink left: text is the paper.
                assert!(close(tone.inked(0.0).apply([0.0; 3]), tone.blank()));
            }
        }
        assert_eq!(PageTone::NONE.inked(0.5), PageTone::NONE);
    }

    #[test]
    fn identity_amounts_are_identity() {
        let c = [0.2, 0.5, 0.9];
        for op in [
            FilterOp::Invert(0.0),
            FilterOp::HueRotate(0.0),
            FilterOp::Saturate(1.0),
            FilterOp::Sepia(0.0),
        ] {
            assert!(close(op.matrix().apply(c), c), "{op:?}");
        }
    }

    #[test]
    fn the_tint_runs_after_the_inversion() {
        let a = Appearance {
            base: BaseMode::Dark,
            tint_hue: 200,
            tint_strength: 60,
            ..Default::default()
        };
        let ops = a.page_filter();
        assert_eq!(ops.first(), Some(&FilterOp::Invert(0.92)));
        assert!(matches!(ops.last(), Some(FilterOp::HueRotate(_))));
    }

    #[test]
    fn sepia_caps_at_055() {
        let a = Appearance {
            tint_strength: 100,
            ..Default::default()
        };
        assert!(a.page_filter().contains(&FilterOp::Sepia(0.55)));
    }

    #[test]
    fn a_tone_runs_the_chain_then_blends_over_the_paper() {
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                tint_strength: 40,
                ..Default::default()
            };
            let tone = a.page_tone();
            assert!(tone.count <= a.page_filter().len());
            let ink = [0.1, 0.3, 0.2];
            let want = a
                .page_blend()
                .apply(tone.paper, apply_filter(&a.page_filter(), ink));
            assert!(close(tone.apply(ink), want), "{base:?}");
        }
    }

    #[test]
    fn book_colours_keep_their_hue_in_every_theme() {
        let red = [0.8, 0.1, 0.1];
        for base in BaseMode::ALL {
            let tone = Appearance {
                base,
                ..Default::default()
            }
            .text_tone();
            let Some(ink) = tone.ink else {
                continue;
            };
            for g in [0.0, 0.4, 1.0] {
                assert!(
                    close(tone.apply([g; 3]), mix(ink, tone.paper, g)),
                    "{base:?}"
                );
            }
            let [r, g, b] = tone.apply(red);
            assert!(r > g + 0.1 && (g - b).abs() < 0.05, "{base:?}");
            // Nearer the ink than the paper, so it reads.
            let l = luma([r, g, b]);
            assert!(
                (l - luma(ink)).abs() < (l - luma(tone.paper)).abs(),
                "{base:?}"
            );
        }
        // Dim's low contrast mutes hues too.
        let spread = |base| {
            let [r, g, _] = Appearance {
                base,
                ..Default::default()
            }
            .text_tone()
            .apply(red);
            r - g
        };
        assert!(spread(BaseMode::Dim) < spread(BaseMode::Light) / 2.0);
    }

    #[test]
    fn a_blank_light_page_shows_the_paper() {
        let tone = Appearance::default().page_tone();
        assert!(close(tone.blank(), tone.paper));
        assert_eq!(PageTone::NONE.apply([0.2; 3]), [0.2; 3]);
    }

    #[test]
    fn blends_match_their_formulas() {
        let (b, s) = ([0.5, 0.2, 1.0], [0.5, 1.0, 0.0]);
        assert!(close(Blend::Multiply.apply(b, s), [0.25, 0.2, 0.0]));
        assert!(close(Blend::Screen.apply(b, s), [0.75, 1.0, 1.0]));
        assert!(
            close(Blend::SoftLight.apply(b, [0.5; 3]), b),
            "s = 0.5 is identity"
        );
    }

    #[test]
    fn a_backdrop_over_white_is_its_paper_themed() {
        let cream = [0.98, 0.95, 0.88];
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                ..Appearance::default()
            };
            let tone = a.page_tone();
            let back = tone.backdrop(cream);
            let (want, got) = (tone.apply(cream), back.apply([1.0; 3]));
            for (w, g) in want.iter().zip(got) {
                assert!((w - g).abs() < 1e-5, "{base:?}: {want:?} vs {got:?}");
            }
            assert_eq!(back.pattern, tone.pattern);
        }
    }

    #[test]
    fn an_untinted_backdrop_is_plain_paper() {
        let back = PageTone::NONE.backdrop([0.5, 0.6, 0.7]);
        assert_eq!(back.paper, [0.5, 0.6, 0.7]);
        assert_eq!(back.apply([1.0; 3]), [0.5, 0.6, 0.7]);
        assert_eq!(back.pattern, Pattern::NONE);
    }

    #[test]
    fn a_book_maps_black_to_its_ink_and_white_to_its_paper() {
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                tint_strength: 60,
                ..Appearance::default()
            };
            let p = a.text_palette();
            let tone = a.text_tone();
            assert!(close(tone.apply([0.0; 3]), p.ink.to_unit()), "{base:?}");
            assert!(close(tone.apply([1.0; 3]), p.paper.to_unit()), "{base:?}");
            assert!(close(tone.blank(), p.paper.to_unit()), "{base:?}");
            // The dial fades ink toward the paper, and never past it.
            let half = tone.inked(0.5).apply([0.0; 3]);
            let mid = mix(p.ink.to_unit(), p.paper.to_unit(), 0.5);
            assert!(close(half, mid), "{base:?}");
            assert!(close(tone.inked(0.0).apply([0.0; 3]), p.paper.to_unit()));
            // Pictures take the PDF filter over the book's paper.
            let pictures = a.page_tone().for_pictures(&tone);
            assert_eq!(pictures.ink, None);
            assert_eq!(pictures.paper, tone.paper);
            assert_eq!(pictures.count, a.page_tone().count);
        }
    }
}
