//! Semantic toast colours harmonised with the active appearance.

use crate::color::{Oklch, Rgb};
use crate::model::{Appearance, BaseMode};
use crate::tokens::legible;

/// One semantic toast's theme-harmonised swatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastSwatch {
    /// Icon, countdown wash hue, and action-button label.
    pub mark: Rgb,
    /// Front card surface: theme surface infused with the semantic hue.
    pub fill: Rgb,
    /// Card surface inside the opened notifications tray.
    pub listed: Rgb,
    /// Card border: theme hairline blended with the semantic mark.
    pub edge: Rgb,
    /// Soft pill fill behind an Undo or action button.
    pub soft: Rgb,
    /// The countdown's block over the paper: a pale tone the ink reads on.
    pub block: Rgb,
}

impl ToastSwatch {
    /// Interpolates every colour `k` of the way toward `to`.
    pub fn mix(self, to: Self, k: f64) -> Self {
        Self {
            mark: self.mark.mix(to.mark, k),
            fill: self.fill.mix(to.fill, k),
            listed: self.listed.mix(to.listed, k),
            edge: self.edge.mix(to.edge, k),
            soft: self.soft.mix(to.soft, k),
            block: self.block.mix(to.block, k),
        }
    }
}

/// The three semantic toast swatches for one appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToastPalette {
    /// Confirmations and neutral notices (green, inflected by the theme).
    pub info: ToastSwatch,
    /// Heads-ups and non-fatal limits (yellow-amber, inflected by the theme).
    pub warning: ToastSwatch,
    /// Failures and errors (red, inflected by the theme).
    pub error: ToastSwatch,
}

impl ToastPalette {
    /// Interpolates all three tones `k` of the way toward `to`.
    pub fn mix(self, to: Self, k: f64) -> Self {
        Self {
            info: self.info.mix(to.info, k),
            warning: self.warning.mix(to.warning, k),
            error: self.error.mix(to.error, k),
        }
    }
}

#[derive(Clone, Copy)]
struct Anchor {
    l: f64,
    c: f64,
    h: f64,
    pull: f64,
}

const fn anchor(l: f64, c: f64, h: f64, pull: f64) -> Anchor {
    Anchor { l, c, h, pull }
}

struct SurfaceMix {
    tint_l: f64,
    tint_c: f64,
    fill_k: f64,
    soft_k: f64,
    edge_k: f64,
}

/// How far the countdown block leans from the paper toward the tone's mark.
/// Smaller is paler; error leans a touch more so it still reads as urgent.
fn block_k(_mode: BaseMode, tone: usize) -> f64 {
    match tone {
        2 => 0.31,
        _ => 0.25,
    }
}

/// The countdown block: the tone's mark mixed into the paper in gamma
/// (sRGB-encoded) space — the same method T60 used, just with a smaller
/// `k`, so it reads as slightly paler while staying clearly tinted.
fn block_of(surface: Rgb, mark: Rgb, k: f64) -> Rgb {
    let g = |c: u8| f64::from(c) / 255.0;
    let u = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgb(
        u(g(surface.0) + (g(mark.0) - g(surface.0)) * k),
        u(g(surface.1) + (g(mark.1) - g(surface.1)) * k),
        u(g(surface.2) + (g(mark.2) - g(surface.2)) * k),
    )
}

/// Pulls `base_h` smoothly toward `theme_h` without flipping at 180°.
fn harmonise_hue(base_h: f64, theme_h: f64, max_pull: f64) -> f64 {
    let delta = (theme_h - base_h + 180.0).rem_euclid(360.0) - 180.0;
    let shift = max_pull * delta.to_radians().sin();
    (base_h + shift).rem_euclid(360.0)
}

fn anchors(mode: BaseMode) -> [Anchor; 3] {
    match mode {
        BaseMode::Light => [
            anchor(0.50, 0.108, 148.0, 12.0),
            anchor(0.56, 0.101, 76.0, 10.0),
            anchor(0.52, 0.136, 25.0, 10.0),
        ],
        BaseMode::Dark => [
            anchor(0.75, 0.108, 148.0, 12.0),
            anchor(0.81, 0.112, 80.0, 10.0),
            anchor(0.70, 0.13, 25.0, 10.0),
        ],
        BaseMode::Dim => [
            anchor(0.70, 0.087, 148.0, 12.0),
            anchor(0.75, 0.091, 80.0, 10.0),
            anchor(0.67, 0.105, 25.0, 10.0),
        ],
        BaseMode::Black => [
            anchor(0.76, 0.112, 148.0, 12.0),
            anchor(0.82, 0.115, 80.0, 10.0),
            anchor(0.71, 0.133, 25.0, 10.0),
        ],
    }
}

fn surface_mix(mode: BaseMode, surf_l: f64) -> SurfaceMix {
    match mode {
        BaseMode::Light => SurfaceMix {
            tint_l: (surf_l + 0.005).clamp(0.84, 0.92),
            tint_c: 0.085,
            fill_k: 0.38,
            soft_k: 0.11,
            edge_k: 0.48,
        },
        BaseMode::Dark => SurfaceMix {
            tint_l: (surf_l + 0.06).clamp(0.22, 0.30),
            tint_c: 0.075,
            fill_k: 0.40,
            soft_k: 0.15,
            edge_k: 0.52,
        },
        BaseMode::Dim => SurfaceMix {
            tint_l: (surf_l + 0.05).clamp(0.25, 0.33),
            tint_c: 0.060,
            fill_k: 0.38,
            soft_k: 0.14,
            edge_k: 0.50,
        },
        BaseMode::Black => SurfaceMix {
            tint_l: 0.25,
            tint_c: 0.080,
            fill_k: 0.48,
            soft_k: 0.16,
            edge_k: 0.54,
        },
    }
}

impl Appearance {
    /// Semantic toast swatches (green, amber, red) mixed with this theme.
    pub fn toast_palette(&self) -> ToastPalette {
        let t = self.ui_tokens();
        let marks = self.ui_marks();
        let theme_h = t.accent.to_oklch().h;
        let sm = surface_mix(self.base, t.surface.to_oklch().l);
        // Black holds no surface hue, so pull a hint from tinted accent_soft.
        let black_tint = if self.base == BaseMode::Black && self.has_tint() {
            0.18 * self.tint_amount()
        } else {
            0.0
        };
        let build = |Anchor { l, c, h, pull }, tone: usize| {
            let h = harmonise_hue(h, theme_h, pull);
            let raw = Oklch { l, c, h }.to_rgb_in_gamut();
            let wash = Oklch {
                l: sm.tint_l,
                c: sm.tint_c,
                h,
            }
            .to_rgb_in_gamut();
            let mut fill = t.surface.mix(wash, sm.fill_k);
            let mut listed = marks.card.mix(wash, sm.fill_k);
            if black_tint > 0.0 {
                fill = fill.mix(t.accent_soft, black_tint);
                listed = listed.mix(t.accent_soft, black_tint);
            }
            let soft = fill.mix(raw, sm.soft_k);
            let mark = legible(raw, &[fill, listed, t.surface, soft]);
            let edge = t.line.mix(mark, sm.edge_k);
            let block = block_of(t.surface, mark, block_k(self.base, tone));
            ToastSwatch {
                mark,
                fill,
                listed,
                edge,
                soft,
                block,
            }
        };
        let a = anchors(self.base);
        ToastPalette {
            info: build(a[0], 0),
            warning: build(a[1], 1),
            error: build(a[2], 2),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::MIN_CONTRAST;

    #[test]
    fn toast_marks_and_body_ink_read_across_every_mode_and_tint() {
        for base in [
            BaseMode::Light,
            BaseMode::Dark,
            BaseMode::Dim,
            BaseMode::Black,
        ] {
            for hue in (0..360).step_by(20) {
                for strength in [0, 20, 50, 100] {
                    let a = Appearance {
                        base,
                        tint_hue: hue,
                        tint_strength: strength,
                        ..Appearance::default()
                    };
                    let t = a.ui_tokens();
                    let p = a.toast_palette();
                    for (name, s) in [("info", p.info), ("warning", p.warning), ("error", p.error)]
                    {
                        let at = format!("{base:?} {hue}/{strength} {name}");
                        for ground in [s.fill, s.listed, t.surface, s.soft] {
                            let ratio = s.mark.contrast(ground);
                            assert!(ratio >= MIN_CONTRAST, "{at}: mark {ratio:.2}");
                        }
                        for ground in [s.fill, s.listed, s.block] {
                            let ratio = t.ink.contrast(ground);
                            assert!(ratio >= MIN_CONTRAST, "{at}: ink {ratio:.2}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn semantic_tones_stay_green_amber_red_and_shift_with_theme() {
        let sepia = Appearance {
            tint_hue: 34,
            tint_strength: 25,
            ..Appearance::default()
        };
        let blue = Appearance::default();
        let (ps, pb) = (sepia.toast_palette(), blue.toast_palette());
        for p in [ps, pb] {
            let g = p.info.mark.to_oklch().h;
            let y = p.warning.mark.to_oklch().h;
            let r = p.error.mark.to_oklch().h;
            assert!((125.0..=168.0).contains(&g), "green hue drifted: {g:.1}");
            assert!((60.0..=96.0).contains(&y), "amber hue drifted: {y:.1}");
            assert!((8.0..=42.0).contains(&r), "red hue drifted: {r:.1}");
        }
        // Warm sepia pulls green warmer than cool blue does.
        assert!(ps.info.mark.to_oklch().h < pb.info.mark.to_oklch().h - 10.0);
        assert_ne!(ps.info.fill, pb.info.fill);
        assert_ne!(ps.warning.edge, pb.warning.edge);
        assert_ne!(ps.error.fill, pb.error.fill);
    }

    #[test]
    fn palette_mix_interpolates_endpoints() {
        let a = Appearance::default().toast_palette();
        let b = Appearance {
            base: BaseMode::Dark,
            tint_hue: 104,
            tint_strength: 20,
            ..Appearance::default()
        }
        .toast_palette();
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        assert_ne!(a.mix(b, 0.5), a);
    }
}
