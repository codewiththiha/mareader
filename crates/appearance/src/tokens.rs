//! The seven UI colour tokens per base mode, and their tint.

use crate::color::{Oklch, Rgb};
use crate::model::{Appearance, BaseMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiTokens {
    pub paper: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
    pub surface: Rgb,
    pub line: Rgb,
    pub accent: Rgb,
    pub accent_soft: Rgb,
}

pub fn base_tokens(mode: BaseMode) -> UiTokens {
    match mode {
        BaseMode::Light => UiTokens {
            paper: Rgb(0xff, 0xff, 0xff),
            ink: Rgb(0x1f, 0x29, 0x37),
            muted: Rgb(0x6b, 0x72, 0x80),
            surface: Rgb(0xeb, 0xed, 0xef),
            line: Rgb(0xda, 0xdc, 0xe1),
            accent: Rgb(0x25, 0x63, 0xeb),
            accent_soft: Rgb(0xdb, 0xea, 0xfe),
        },
        BaseMode::Dark => UiTokens {
            paper: Rgb(0x13, 0x13, 0x16),
            ink: Rgb(0xe5, 0xe7, 0xeb),
            muted: Rgb(0x9c, 0xa3, 0xaf),
            surface: Rgb(0x16, 0x16, 0x1a),
            line: Rgb(0x2b, 0x2b, 0x31),
            accent: Rgb(0x60, 0xa5, 0xfa),
            accent_soft: Rgb(0x1d, 0x2b, 0x3a),
        },
        BaseMode::Dim => UiTokens {
            paper: Rgb(0x1a, 0x1c, 0x1f),
            ink: Rgb(0xc3, 0xc6, 0xcb),
            muted: Rgb(0x8b, 0x8f, 0x96),
            surface: Rgb(0x20, 0x23, 0x28),
            line: Rgb(0x2e, 0x32, 0x38),
            accent: Rgb(0x7a, 0x9b, 0xd4),
            accent_soft: Rgb(0x23, 0x2b, 0x36),
        },
        // Paper and chrome pitch black; ink a little soft to spare glare.
        BaseMode::Black => UiTokens {
            paper: Rgb(0x00, 0x00, 0x00),
            ink: Rgb(0xd4, 0xd4, 0xd8),
            muted: Rgb(0x8a, 0x8a, 0x93),
            surface: Rgb(0x00, 0x00, 0x00),
            line: Rgb(0x27, 0x27, 0x2a),
            accent: Rgb(0x60, 0xa5, 0xfa),
            accent_soft: Rgb(0x0c, 0x1e, 0x33),
        },
    }
}

/// Page tint hues are sRGB angles; UI tokens rotate on the OKLCH circle.
fn ui_hue_oklch(srgb_hue: f64) -> f64 {
    let h = srgb_hue.rem_euclid(360.0) / 60.0;
    let f = h.fract();
    let (r, g, b) = match h.floor() as u8 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    let byte = |v: f64| (v * 255.0).round() as u8;
    Rgb(byte(r), byte(g), byte(b)).to_oklch().h
}

/// How far a near-white token may darken to carry its tint.
const MAX_DARKEN: f64 = 0.06;

/// Takes the tint's hue outright and keeps lightness; chroma eases toward
/// `max_c`.
fn tinted(token: Rgb, target_h: f64, t: f64, max_c: f64) -> Rgb {
    let Oklch { l, c, .. } = token.to_oklch();
    let want = Oklch {
        l,
        c: c + (max_c - c) * t,
        h: target_h,
    };
    dimmed(want)
}

/// Fits a colour by darkening up to MAX_DARKEN, then by losing chroma.
fn dimmed(want: Oklch) -> Rgb {
    if want.in_gamut() {
        return want.to_rgb();
    }
    let l = want.l;
    // Near white, sRGB has no room for chroma: give up a little lightness.
    (1..=12)
        .map(|i| Oklch {
            l: l - MAX_DARKEN * f64::from(i) / 12.0,
            ..want
        })
        .find(|o| o.in_gamut())
        // Squeezed at full lightness, chroma would leave pure white.
        .unwrap_or(Oklch {
            l: l - MAX_DARKEN,
            ..want
        })
        .to_rgb_in_gamut()
}

/// A book page's colours: mareader's text palette, not the PDF filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPalette {
    pub paper: Rgb,
    pub ink: Rgb,
}

impl Appearance {
    /// Paper and ink lightness per mode, chroma growing with the tint.
    pub fn text_palette(&self) -> TextPalette {
        let t = self.tint_amount();
        let h = ui_hue_oklch(f64::from(self.tint_hue));
        // Lightness, then the chroma a full tint reaches, as mareader's.
        let ((paper_l, paper_c), (ink_l, ink_c)) = match self.base {
            BaseMode::Light => ((0.98, 0.08), (0.15, 0.03)),
            BaseMode::Dark => ((0.24, 0.10), (0.92, 0.04)),
            BaseMode::Dim => ((0.22, 0.08), (0.12, 0.03)),
            // Black paper holds no hue, so the tint colours the ink instead.
            BaseMode::Black => ((0.0, 0.0), (0.86, 0.07)),
        };
        // Capped at what sRGB holds, so every step of strength still shows.
        let room = |l: f64| Oklch { l, c: 0.0, h }.max_chroma();
        let darkest = (paper_l - MAX_DARKEN).max(0.0);
        let cap = f64::min(paper_c, room(paper_l).max(room(darkest)));
        let paper = dimmed(Oklch {
            l: paper_l,
            c: cap * t,
            h,
        });
        let ink = Oklch {
            l: ink_l,
            c: ink_c * t,
            h,
        };
        TextPalette {
            paper,
            ink: ink.to_rgb_in_gamut(),
        }
    }

    /// The UI palette for this look; untinted bases come back unchanged.
    pub fn ui_tokens(&self) -> UiTokens {
        let (raw, dark) = (self.raw_tokens(), self.base.is_dark());
        // Light picks are a ground for small text; dark ones hold none.
        let picked = marks(&raw, dark).selected;
        readable(
            raw,
            if dark {
                &[]
            } else {
                std::slice::from_ref(&picked)
            },
        )
    }

    /// The fills that mark rows in this look: card, hover and pick.
    pub fn ui_marks(&self) -> Marks {
        marks(&self.raw_tokens(), self.base.is_dark())
    }

    /// The palette before small text is floored to read.
    fn raw_tokens(&self) -> UiTokens {
        let base = base_tokens(self.base);
        if !self.has_tint() {
            return base;
        }
        let t = self.tint_amount();
        let h = ui_hue_oklch(f64::from(self.tint_hue));
        // Black stays black: its paper and chrome take no tint.
        let black = self.base == BaseMode::Black;
        let ground = |token: Rgb, max_c| match black {
            true => token,
            false => tinted(token, h, t, max_c),
        };
        // Chrome stays off the book's page, and lines off the chrome.
        let surface = match black {
            true => base.surface,
            false => apart(
                ground(base.surface, 0.070),
                self.text_palette().paper,
                SURFACE_GAP,
            ),
        };
        let line = tinted(base.line, h, t, 0.090);
        // Light lines sit under the chrome, even where a tint darkens it.
        let line = match self.base.is_dark() {
            true => apart(line, surface, LINE_GAP),
            false => below(line, surface, LINE_GAP),
        };
        UiTokens {
            paper: ground(base.paper, 0.055),
            ink: tinted(base.ink, h, t, 0.020),
            muted: tinted(base.muted, h, t, 0.045),
            surface,
            line,
            accent: tinted(base.accent, h, t, 0.150),
            accent_soft: tinted(base.accent_soft, h, t, 0.110),
        }
    }
}

/// `token` at least `gap` of lightness under `from`.
fn below(token: Rgb, from: Rgb, gap: f64) -> Rgb {
    let mut lch = token.to_oklch();
    let target = from.to_oklch().l - gap;
    if lch.l <= target {
        return token;
    }
    lch.l = target.max(0.0);
    lch.to_rgb_in_gamut()
}

/// Fills that mark rows: a card's wash, a hovered row, a picked one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marks {
    pub card: Rgb,
    pub hover: Rgb,
    pub selected: Rgb,
}

/// How far, in OKLab, a pick stands from the grounds it sits on.
pub const PICK_GAP: f64 = 0.075;
/// And from a hovered row beside it.
pub const PICK_HOVER_GAP: f64 = 0.045;

/// A light look's pick in its accent's hue, deepened until it stands out.
fn marks(t: &UiTokens, dark: bool) -> Marks {
    if dark {
        let card = t.surface.mix(t.line, 0.5).mix(t.accent_soft, 0.25);
        let selected = t.accent_soft.mix(t.accent, 0.3);
        return Marks {
            card,
            hover: t.line,
            selected,
        };
    }
    let card = t.paper.mix(t.accent_soft, 0.18);
    // Hover stays a quiet step, so it never outshines the pick.
    let hover = t.surface.mix(t.line, 0.55);
    let soft = t.accent_soft.to_oklch();
    let h = if soft.c < 0.01 {
        t.accent.to_oklch().h
    } else {
        soft.h
    };
    let grounds = [t.surface, card, t.paper];
    let clear = |c: Rgb| {
        let ground = grounds.iter().map(|g| c.distance(*g)).fold(1.0, f64::min);
        ground >= PICK_GAP && c.distance(hover) >= PICK_HOVER_GAP
    };
    // Warm and violet hues hold little chroma this pale, so it also darkens.
    let mut selected = t.accent_soft;
    for i in 0..80 {
        let l = soft.l - 0.004 * f64::from(i);
        let full = Oklch { l, c: 0.0, h }.max_chroma();
        let c = (full * 0.92).min(0.13).max(soft.c);
        selected = Oklch { l, c, h }.to_rgb_in_gamut();
        if clear(selected) {
            break;
        }
    }
    Marks {
        card,
        hover,
        selected,
    }
}

/// Lightness kept between the chrome and a book's page.
pub const SURFACE_GAP: f64 = 0.035;
/// Lightness kept between a separator and the chrome it divides.
pub const LINE_GAP: f64 = 0.05;

/// `token` moved off `from` by at least `gap` of lightness, on its own side.
fn apart(token: Rgb, from: Rgb, gap: f64) -> Rgb {
    let mut lch = token.to_oklch();
    let ground = from.to_oklch().l;
    let up = lch.l >= ground;
    let target = if up { ground + gap } else { ground - gap };
    if (up && lch.l >= target) || (!up && lch.l <= target) {
        return token;
    }
    lch.l = target.clamp(0.0, 1.0);
    lch.to_rgb_in_gamut()
}

/// WCAG AA for the small text muted and accent ink are used for.
pub const MIN_CONTRAST: f64 = 4.5;

/// Muted and accent ink moved just far enough to read on every ground.
fn readable(t: UiTokens, picks: &[Rgb]) -> UiTokens {
    let mut grounds = vec![t.paper, t.surface, t.accent_soft];
    grounds.extend_from_slice(picks);
    UiTokens {
        muted: legible(t.muted, &grounds),
        accent: legible(t.accent, &grounds),
        ..t
    }
}

/// `ink` darkened on light grounds, lightened on dark, until it reads.
pub fn legible(ink: Rgb, grounds: &[Rgb]) -> Rgb {
    let worst = |c: Rgb| grounds.iter().map(|g| c.contrast(*g)).fold(21.0, f64::min);
    let light = grounds.iter().map(|g| g.luminance()).sum::<f64>() / grounds.len().max(1) as f64;
    let step = if light > 0.18 { -0.005 } else { 0.005 };
    let mut lch = ink.to_oklch();
    let mut out = ink;
    // At most 0.4 of lightness: black or white reads on any ground here.
    for _ in 0..80 {
        if worst(out) >= MIN_CONTRAST {
            break;
        }
        lch.l = (lch.l + step).clamp(0.0, 1.0);
        out = lch.to_rgb_in_gamut();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_ui_text_reads_on_every_ground_in_every_look() {
        for base in BaseMode::ALL {
            for (hue, strength) in [(0, 0), (30, 35), (40, 100), (120, 60), (220, 100)] {
                let a = Appearance {
                    base,
                    tint_hue: hue,
                    tint_strength: strength,
                    ..Appearance::default()
                };
                let t = a.ui_tokens();
                for ground in [t.paper, t.surface, t.accent_soft] {
                    for (name, ink) in [("muted", t.muted), ("accent", t.accent)] {
                        let c = ink.contrast(ground);
                        assert!(
                            c >= MIN_CONTRAST,
                            "{base:?} {hue}/{strength} {name}: {c:.2}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn readable_ink_stays_put() {
        let dark = base_tokens(BaseMode::Dark);
        assert_eq!(
            readable(dark, &[]).muted,
            dark.muted,
            "dark muted already reads"
        );
    }

    #[test]
    fn a_stronger_tint_never_whitens_light_paper() {
        let mut last = 0.0;
        for strength in [10, 20, 35, 50, 100] {
            let a = Appearance {
                tint_hue: 4,
                tint_strength: strength,
                ..Appearance::default()
            };
            let c = a.ui_tokens().paper.to_oklch().c;
            assert!(
                c > 0.005 && c >= last - 1e-3,
                "strength {strength}: chroma {c}"
            );
            last = c;
        }
    }

    #[test]
    fn untinted_tokens_are_the_css_values() {
        let a = Appearance::default();
        let base = base_tokens(BaseMode::Light);
        // All but the floored small-text inks, which only darken.
        assert_eq!(a.ui_tokens(), readable(base, &[a.ui_marks().selected]));
        assert_eq!(
            (a.ui_tokens().paper, a.ui_tokens().line),
            (base.paper, base.line)
        );
        assert!(a.ui_tokens().muted.luminance() < base.muted.luminance());
        assert_eq!(base_tokens(BaseMode::Dark).accent.to_hex(), "#60a5fa");
        assert_eq!(base_tokens(BaseMode::Dim).paper.to_hex(), "#1a1c1f");
    }

    #[test]
    fn a_tint_keeps_lightness_and_moves_hue() {
        let a = Appearance {
            tint_hue: 104,
            tint_strength: 50,
            ..Default::default()
        };
        // Accent-soft: tinted, but never moved by the contrast floor.
        let before = base_tokens(BaseMode::Light).accent_soft.to_oklch();
        let after = a.ui_tokens().accent_soft.to_oklch();
        assert!((before.l - after.l).abs() < 0.02, "{before:?} → {after:?}");
        assert!((before.h - after.h).abs() > 20.0, "{before:?} → {after:?}");
    }

    #[test]
    fn sepia_stays_in_its_own_hue() {
        let sepia = Appearance {
            tint_hue: 34,
            tint_strength: 23,
            ..Default::default()
        };
        let target = ui_hue_oklch(34.0);
        let t = sepia.ui_tokens();
        for token in [t.paper, t.surface, t.line, t.accent, t.accent_soft] {
            let Oklch { c, h, .. } = token.to_oklch();
            let off = (h - target + 180.0).rem_euclid(360.0) - 180.0;
            let hex = token.to_hex();
            assert!(c > 0.008, "{hex} lost its tint");
            assert!(off.abs() < 12.0, "{hex} is {h:.0}°, sepia is {target:.0}°");
        }
    }

    #[test]
    fn every_step_of_strength_deepens_the_book_paper() {
        // Black's paper stays black; its own test follows.
        for base in [BaseMode::Light, BaseMode::Dark, BaseMode::Dim] {
            for hue in [14, 34, 104, 160, 200, 240, 290] {
                let chroma = |strength| {
                    let a = Appearance {
                        base,
                        tint_hue: hue,
                        tint_strength: strength,
                        ..Appearance::default()
                    };
                    a.text_palette().paper.to_oklch().c
                };
                let mut last = chroma(0);
                for strength in [10, 20, 30, 40, 50] {
                    let c = chroma(strength);
                    let at = format!("{base:?} hue {hue} at {strength}");
                    assert!(c > last + 0.003, "{at}: {c:.4} after {last:.4}");
                    last = c;
                }
                assert!(last > 0.03, "{base:?} hue {hue} tops out at {last:.4}");
            }
        }
    }

    #[test]
    fn black_stays_black_and_its_tint_goes_to_the_ink() {
        let black = Rgb(0, 0, 0);
        let mut last = -1.0;
        for strength in [0, 10, 25, 50] {
            let a = Appearance {
                base: BaseMode::Black,
                tint_hue: 34,
                tint_strength: strength,
                ..Appearance::default()
            };
            let (ui, book) = (a.ui_tokens(), a.text_palette());
            assert_eq!((ui.paper, ui.surface, book.paper), (black, black, black));
            let ink = book.ink.to_oklch();
            assert!((ink.l - 0.86).abs() < 0.02, "{strength}: ink {ink:?}");
            assert!(ink.c > last + 0.005 || strength == 0, "{strength}: {ink:?}");
            last = ink.c;
        }
    }

    #[test]
    fn chrome_stays_off_the_book_page_and_lines_off_the_chrome() {
        let l = |c: Rgb| c.to_oklch().l;
        for base in [BaseMode::Light, BaseMode::Dark, BaseMode::Dim] {
            for (hue, strength) in [(0, 0), (30, 35), (34, 50), (104, 50), (160, 40), (220, 50)] {
                let a = Appearance {
                    base,
                    tint_hue: hue,
                    tint_strength: strength,
                    ..Appearance::default()
                };
                let (ui, book) = (a.ui_tokens(), a.text_palette());
                let at = format!("{base:?} {hue}/{strength}");
                // Bytes round lightness by a few thousandths.
                let page = (l(ui.surface) - l(book.paper)).abs();
                assert!(page >= SURFACE_GAP - 0.006, "{at}: surface {page:.3}");
                let line = (l(ui.line) - l(ui.surface)).abs();
                assert!(line >= LINE_GAP - 0.006, "{at}: line {line:.3}");
            }
        }
    }

    #[test]
    fn a_light_pick_stands_out_and_hover_stays_quieter() {
        for hue in (0..360).step_by(6) {
            for strength in [0, 5, 10, 18, 25, 35, 50, 100] {
                let a = Appearance {
                    tint_hue: hue,
                    tint_strength: strength,
                    ..Appearance::default()
                };
                let (t, m) = (a.ui_tokens(), a.ui_marks());
                let at = format!("{hue}/{strength}");
                for g in [t.surface, m.card, t.paper] {
                    let d = m.selected.distance(g);
                    assert!(d >= PICK_GAP - 0.002, "{at}: pick {d:.3} off a ground");
                }
                let d = m.selected.distance(m.hover);
                assert!(d >= PICK_HOVER_GAP - 0.002, "{at}: pick {d:.3} off hover");
                let l = |c: Rgb| c.to_oklch().l;
                assert!(l(t.line) < l(t.surface), "{at}: line over the chrome");
                assert!(l(m.hover) < l(t.surface), "{at}: hover over the chrome");
                let (soft, pick) = (t.accent_soft.to_oklch(), m.selected.to_oklch());
                let off = (pick.h - soft.h + 180.0).rem_euclid(360.0) - 180.0;
                assert!(soft.c < 0.01 || off.abs() < 15.0, "{at}: pick left its hue");
                for ink in [t.ink, t.accent] {
                    let c = ink.contrast(m.selected);
                    assert!(c >= MIN_CONTRAST, "{at}: {c:.2} on the pick");
                }
            }
        }
    }

    #[test]
    fn srgb_red_maps_near_oklch_29() {
        assert!((ui_hue_oklch(0.0) - 29.2).abs() < 1.0);
        assert!((ui_hue_oklch(360.0) - ui_hue_oklch(0.0)).abs() < 1e-9);
    }

    #[test]
    fn book_pages_keep_mareader_s_paper_and_ink_lightness() {
        for (base, paper, ink) in [
            (BaseMode::Light, 0.98, 0.15),
            (BaseMode::Dark, 0.24, 0.92),
            (BaseMode::Dim, 0.22, 0.12),
        ] {
            for strength in [0, 40, 100] {
                let a = Appearance {
                    base,
                    tint_hue: 200,
                    tint_strength: strength,
                    ..Appearance::default()
                };
                let p = a.text_palette();
                let (pl, il) = (p.paper.to_oklch().l, p.ink.to_oklch().l);
                // Paper may darken a little to carry its tint, never lighten.
                let room = (paper - MAX_DARKEN - 0.02)..(paper + 0.02);
                assert!(room.contains(&pl), "{base:?} {strength}: paper {pl}");
                assert!((il - ink).abs() < 0.02, "{base:?} {strength}: ink {il}");
                let chroma = p.paper.to_oklch().c;
                assert_eq!(
                    strength == 0,
                    chroma < 0.005,
                    "{base:?} {strength}: {chroma}"
                );
            }
        }
    }
}
