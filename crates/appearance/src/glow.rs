//! The AI glow's colours, mixed from a look's paper and accent.

use crate::color::{Oklch, Rgb};
use crate::model::Appearance;

/// The light the AI glow casts in one look.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlowPalette {
    /// Blue, green, yellow and red along the arc, the accent taking one.
    pub hues: [Rgb; 4],
    /// 0 laid over the page to 1 added to it as light.
    pub add: f32,
    /// How strong the light is at the outline, from 0 to 1.
    pub strength: f32,
}

impl GlowPalette {
    /// `k` of the way toward `to`, for a look's crossfade.
    pub fn mix(self, to: Self, k: f64) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * k.clamp(0.0, 1.0) as f32;
        let mut hues = self.hues;
        for (h, to) in hues.iter_mut().zip(to.hues) {
            *h = h.mix(to, k);
        }
        Self {
            hues,
            add: lerp(self.add, to.add),
            strength: lerp(self.strength, to.strength),
        }
    }
}

/// Google's blue, green, yellow and red in Oklch: lightness, chroma, hue.
const GOOGLE: [(f64, f64, f64); 4] = [
    (0.630, 0.180, 260.0),
    (0.648, 0.160, 148.5),
    (0.812, 0.168, 79.8),
    (0.627, 0.207, 28.6),
];
/// Paper lighter than the first is light, darker than the second dark.
const GROUND: (f64, f64) = (0.7, 0.3);
/// The lightness dark paper lifts colours toward, and the chroma it sheds.
const GLOW_L: f64 = 0.86;
const SHED: f64 = 0.22;
/// The most degrees the theme bends each colour.
const BEND: f64 = 10.0;

impl Appearance {
    /// The AI glow's colours: the accent takes the nearest of Google's four,
    /// the theme bends the rest, and dark paper turns them to brighter light.
    pub fn glow_palette(&self) -> GlowPalette {
        let t = self.ui_tokens();
        let (paper, accent) = (t.paper.to_oklch(), t.accent.to_oklch());
        let dark = darkness(paper.l);
        // A grey accent or paper has no hue worth bending toward.
        let accent_k = (accent.c / 0.08).min(1.0);
        let paper_k = (paper.c / 0.03).min(1.0);
        let gap = |h: f64| ((accent.h - h + 180.0).rem_euclid(360.0) - 180.0).abs();
        let nearest = GOOGLE
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| gap(a.2).total_cmp(&gap(b.2)))
            .map_or(0, |(i, _)| i);
        let mut slot = 0;
        let hues = GOOGLE.map(|(l, c, h)| {
            slot += 1;
            let (c, h) = match slot - 1 == nearest {
                true => (c * accent_k.max(0.6), accent.h),
                false => (c, bend(h, accent.h, BEND * accent_k)),
            };
            Oklch {
                l: l + (GLOW_L - l) * 0.5 * dark,
                c: c * (1.0 - SHED * dark),
                h: bend(h, paper.h, 0.5 * BEND * paper_k),
            }
            .to_rgb_in_gamut()
        });
        GlowPalette {
            hues,
            add: (0.8 * dark) as f32,
            strength: (1.0 - 0.2 * dark) as f32,
        }
    }
}

/// 0 on light paper to 1 on dark, eased between.
fn darkness(l: f64) -> f64 {
    let x = ((GROUND.0 - l) / (GROUND.0 - GROUND.1)).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// `h` bent toward `to` by up to `most` degrees, never flipping past it.
fn bend(h: f64, to: f64, most: f64) -> f64 {
    let delta = (to - h + 180.0).rem_euclid(360.0) - 180.0;
    (h + most * delta.to_radians().sin()).rem_euclid(360.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BaseMode;

    fn look(base: BaseMode, tint: u8) -> Appearance {
        Appearance {
            base,
            tint_strength: tint,
            ..Appearance::default()
        }
    }

    #[test]
    fn dark_paper_turns_the_glow_into_brighter_added_light() {
        let light = look(BaseMode::Light, 0).glow_palette();
        assert_eq!(light.add, 0.0);
        for base in [BaseMode::Dark, BaseMode::Dim, BaseMode::Black] {
            let p = look(base, 0).glow_palette();
            assert!(p.add > 0.5, "{base:?}");
            for (d, l) in p.hues.iter().zip(light.hues) {
                assert!(d.to_oklch().l > l.to_oklch().l - 1e-6, "{base:?}");
            }
        }
    }

    #[test]
    fn the_accent_takes_a_colour_and_neighbours_stay_apart() {
        for base in BaseMode::ALL {
            for (hue, tint) in [(34, 0), (34, 60), (140, 60), (300, 100)] {
                let a = Appearance {
                    tint_hue: hue,
                    ..look(base, tint)
                };
                let p = a.glow_palette();
                let accent = a.ui_tokens().accent.to_oklch().h;
                let gap = |c: &Rgb| {
                    let off = c.to_oklch().h - accent + 180.0;
                    (off.rem_euclid(360.0) - 180.0).abs()
                };
                let closest = p.hues.iter().map(gap).fold(f64::MAX, f64::min);
                assert!(closest < 25.0, "{base:?} {hue} {tint}: {closest}");
                for i in 0..p.hues.len() {
                    let next = p.hues[(i + 1) % p.hues.len()];
                    assert!(p.hues[i].distance(next) > 0.05, "{base:?} {hue} {tint}");
                }
            }
        }
    }

    #[test]
    fn a_tint_bends_the_colours() {
        let plain = look(BaseMode::Light, 0).glow_palette();
        let warm = Appearance {
            tint_hue: 34,
            ..look(BaseMode::Light, 60)
        };
        assert_ne!(plain.hues, warm.glow_palette().hues);
    }
}
