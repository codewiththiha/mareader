//! Highlighter fills that sit under the ink, as a pen on paper does.

use crate::color::Rgb;
use crate::filter::{Blend, PageTone};

/// Ink against a mark keeps this contrast, or a share of the page's own.
pub const MARK_CONTRAST: f64 = 4.5;
/// That share, for pages already softer than [`MARK_CONTRAST`].
const MARK_SHARE: f64 = 0.6;
/// Screened fills glow on dark paper, so they start weaker.
const DARK_STRENGTH: f32 = 0.8;
/// Paper this dark, under darker ink, lifts its marked text.
const DIM_PAPER: f64 = 0.2;
/// The near-white a lifted mark draws its text in.
const LIFT_INK: [f32; 3] = [0.93, 0.93, 0.94];
/// Its contrast on the wash, with room for grain laid over the page.
const LIFT_CONTRAST: f64 = 5.0;

/// A highlighter's fill as the page pass draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkFill {
    /// Multiply on light paper, screen on dark: either keeps the ink.
    pub blend: Blend,
    /// sRGB-encoded, 0..1.
    pub color: [f32; 3],
    /// How far the fill goes, 0..1.
    pub strength: f32,
    /// Set on dim pages: the mark redraws its text in this light ink.
    pub lift: Option<[f32; 3]>,
}

impl MarkFill {
    /// What `backdrop` turns into: the shader's oracle; lifted, the colour.
    pub fn over(&self, backdrop: [f32; 3]) -> [f32; 3] {
        if self.lift.is_some() {
            return self.color;
        }
        let s = self.strength;
        let one = |b: f32, c: f32| match self.blend {
            Blend::Screen => b + c * s * (1.0 - b),
            _ => b * (1.0 - s + c * s),
        };
        let ([br, bg, bb], [cr, cg, cb]) = (backdrop, self.color);
        [one(br, cr), one(bg, cg), one(bb, cb)]
    }

    /// The colour `ink` shows in under the fill.
    pub fn ink_over(&self, ink: [f32; 3]) -> [f32; 3] {
        self.lift.unwrap_or_else(|| self.over(ink))
    }

    /// A lifted mark's tone: page white to its colour, black to its ink.
    pub fn tone(&self) -> Option<PageTone> {
        let ink = self.lift?;
        Some(PageTone {
            blend: Some(Blend::Multiply),
            paper: self.color,
            ink: Some(ink),
            ..PageTone::NONE
        })
    }
}

impl PageTone {
    /// The paper and ink this tone shows a plain page in.
    pub fn paper_and_ink(&self) -> ([f32; 3], [f32; 3]) {
        (self.apply([1.0; 3]), self.apply([0.0; 3]))
    }

    /// A fill of `color` at `opacity` on this page, the ink kept legible.
    pub fn mark(&self, color: [f32; 3], opacity: f32) -> MarkFill {
        let (paper, ink) = self.paper_and_ink();
        let dark = luminance(paper) < luminance(ink);
        if !dark && luminance(paper) < DIM_PAPER {
            return lifted(paper, color, opacity);
        }
        let (blend, top) = match dark {
            true => (Blend::Screen, opacity * DARK_STRENGTH),
            false => (Blend::Multiply, opacity),
        };
        let fill = |strength| MarkFill {
            blend,
            color,
            strength,
            lift: None,
        };
        let floor = MARK_CONTRAST.min(contrast(paper, ink) * MARK_SHARE);
        let legible = |s| {
            let f = fill(s);
            contrast(f.over(paper), f.over(ink)) >= floor
        };
        let top = top.clamp(0.0, 1.0);
        if legible(top) {
            return fill(top);
        }
        // Contrast falls as the fill grows: the strongest that holds.
        let (mut lo, mut hi) = (0.0, top);
        for _ in 0..14 {
            let mid = (lo + hi) * 0.5;
            if legible(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        fill(lo)
    }
}

/// Dark ink on dark paper: the pen's wash, dark enough for light text.
fn lifted(paper: [f32; 3], color: [f32; 3], opacity: f32) -> MarkFill {
    let s = opacity.clamp(0.0, 1.0);
    let mut wash = paper;
    for (w, c) in wash.iter_mut().zip(color) {
        *w += (c - *w) * s;
    }
    let shade = |k: f32| wash.map(|c| c * k);
    let legible = |k| contrast(shade(k), LIFT_INK) >= LIFT_CONTRAST;
    // Darker reads better: the lightest shade that holds.
    let (mut lo, mut hi) = (0.0, 1.0);
    if !legible(hi) {
        for _ in 0..14 {
            let mid = (lo + hi) * 0.5;
            if legible(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        hi = lo;
    }
    MarkFill {
        blend: Blend::Multiply,
        color: shade(hi),
        strength: 1.0,
        lift: Some(LIFT_INK),
    }
}

fn luminance(c: [f32; 3]) -> f64 {
    Rgb::from_unit(c).luminance()
}

fn contrast(a: [f32; 3], b: [f32; 3]) -> f64 {
    Rgb::from_unit(a).contrast(Rgb::from_unit(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Appearance, BaseMode};

    const PENS: [[u8; 3]; 8] = [
        [0xff, 0xff, 0x00],
        [0xff, 0xbe, 0x00],
        [0xff, 0x00, 0x00],
        [0x83, 0xd3, 0x3c],
        [0x00, 0xb2, 0xf4],
        [0x16, 0x77, 0xff],
        [0xa5, 0x8a, 0xf0],
        [0xff, 0x00, 0xff],
    ];

    fn tones() -> Vec<(String, PageTone)> {
        let mut out = Vec::new();
        for base in BaseMode::ALL {
            let a = Appearance {
                base,
                ..Default::default()
            };
            out.push((format!("{base:?} book"), a.text_tone()));
            out.push((format!("{base:?} pdf"), a.page_tone()));
        }
        out
    }

    fn unit([r, g, b]: [u8; 3]) -> [f32; 3] {
        Rgb(r, g, b).to_unit()
    }

    #[test]
    fn ink_stays_legible_on_every_mark_in_every_look() {
        for (name, tone) in tones() {
            let (paper, ink) = tone.paper_and_ink();
            let floor = MARK_CONTRAST.min(contrast(paper, ink) * MARK_SHARE);
            for pen in PENS {
                for opacity in [0.2, 0.5, 0.8] {
                    let f = tone.mark(unit(pen), opacity);
                    let c = contrast(f.over(paper), f.ink_over(ink));
                    assert!(c >= floor - 0.05, "{name} {pen:?} {opacity}: {c}");
                }
            }
        }
    }

    #[test]
    fn marks_multiply_on_light_paper_and_screen_on_dark() {
        for (name, tone) in tones() {
            let (paper, ink) = tone.paper_and_ink();
            let f = tone.mark(unit(PENS[0]), 0.5);
            let dark = luminance(paper) < luminance(ink);
            let want = if dark { Blend::Screen } else { Blend::Multiply };
            assert_eq!(f.blend, want, "{name}");
            // Still a mark: the paper visibly takes its colour.
            let lit = f.over(paper);
            let moved = lit.iter().zip(paper).map(|(a, b)| (a - b).abs());
            assert!(moved.fold(0.0, f32::max) > 0.08, "{name}");
        }
    }

    #[test]
    fn dim_pages_lift_marked_text_to_light_ink() {
        let dim = Appearance {
            base: BaseMode::Dim,
            ..Default::default()
        };
        for tone in [dim.text_tone(), dim.page_tone()] {
            for pen in PENS {
                for opacity in [0.2, 0.5, 0.8] {
                    let f = tone.mark(unit(pen), opacity);
                    let Some(ink) = f.lift else {
                        panic!("{pen:?} {opacity}: not lifted");
                    };
                    assert!(contrast(f.color, ink) >= LIFT_CONTRAST - 0.05);
                    assert!(luminance(ink) > luminance(f.color));
                    // The redraw: page white to the fill, black to the ink.
                    let Some((paper, dark)) = f.tone().map(|t| t.paper_and_ink()) else {
                        panic!("{pen:?}: no tone");
                    };
                    let near = |a: [f32; 3], b: [f32; 3]| {
                        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.004)
                    };
                    assert!(near(paper, f.color) && near(dark, ink), "{pen:?}");
                }
            }
        }
        // Other looks keep the page's own ink.
        let light = Appearance::default().page_tone();
        assert_eq!(light.mark(unit(PENS[0]), 0.5).lift, None);
    }

    #[test]
    fn a_light_page_keeps_black_ink_black_under_a_mark() {
        let tone = Appearance::default().page_tone();
        let f = tone.mark(unit(PENS[0]), 0.8);
        assert_eq!(f.blend, Blend::Multiply);
        assert!((f.strength - 0.8).abs() < 1e-6);
        assert_eq!(f.over([0.0; 3]), [0.0; 3]);
        let lit = f.over([1.0; 3]);
        assert!(lit[0] > 0.99 && lit[2] < 0.25);
    }
}
