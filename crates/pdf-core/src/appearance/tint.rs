//! The colour maths: the sRGB→OKLCH hue mapping, the canvas filter pipeline,
//! the blend family, and the tinted UI-token overrides.
//!
//! A translucent colour wash would muddy the text, so the tint is a
//! `sepia() saturate() hue-rotate()` chain; the UI tokens are emitted in
//! OKLCH with each token's OWN lightness preserved (which encodes the
//! hierarchy: page brighter than chrome, chrome brighter than its borders) —
//! only hue and chroma move, so contrast ratios survive a 100% tint.

use crate::appearance::{Appearance, BaseMode};

/// Map an sRGB hue angle (`tint_hue`, applied via `hue-rotate()`) to the
/// corresponding OKLCH hue angle (what the UI tokens are emitted in). The two
/// circles are rotated relative to each other, so converting a fully saturated
/// sRGB colour at the requested angle recovers the right OKLCH hue and one
/// slider drives both consistently.
pub fn ui_hue_oklch(srgb_hue: f64) -> f64 {
    let h = srgb_hue.rem_euclid(360.0) / 60.0;
    let i = h.floor() as i32;
    let f = h - h.floor();
    // hsl(H 100% 50%) -> rgb, without a colour library.
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8
    );
    crate::oklch::hex_to_oklch(&hex)
        .map(|(_, _, h)| h)
        .unwrap_or(srgb_hue)
}

impl Appearance {
    pub fn canvas_filter(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        match self.base {
            BaseMode::Light => {}
            BaseMode::Dark => {
                parts.push("invert(0.92)".into());
                parts.push("hue-rotate(180deg)".into());
                parts.push("saturate(0.85)".into());
                parts.push("brightness(1.02)".into());
            }
            BaseMode::Dim => {
                parts.push("brightness(0.8)".into());
                parts.push("saturate(0.75)".into());
                parts.push("contrast(0.9)".into());
            }
        }

        if self.has_tint() {
            let t = self.tint_strength as f64 / 100.0;
            // Cap sepia at 0.55: past that the collapse starts eating real
            // colour in figures and photographs, and the page reads as a
            // duotone print rather than tinted paper.
            let sep = (t * 0.55).clamp(0.0, 0.55);
            // Sepia flattens chroma; give it back proportionally so a strong
            // tint reads as saturated rather than merely beige.
            let sat = 1.0 + t * 0.6;
            // sepia() lands around 34deg (a warm brown). Measure the requested
            // hue from there so tint_hue is an absolute target, not an offset.
            let rot = (self.tint_hue as f64) - 34.0;
            parts.push(format!("sepia({sep:.3})"));
            parts.push(format!("saturate({sat:.3})"));
            parts.push(format!("hue-rotate({rot:.1}deg)"));
        }

        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(" ")
        }
    }

    /// Blend mode for the canvas against the page background.
    ///
    /// `multiply` keeps light themes paper-like. Inverted canvases need
    /// `screen` (multiply can only darken, so it would crush the near-white
    /// inverted text back into the dark page and destroy readability).
    /// Dim is not inverted but is darkened, and soft-light preserves its
    /// midtones where multiply would double up the darkening.
    pub fn canvas_blend(&self) -> &'static str {
        match self.base {
            BaseMode::Light => "multiply",
            BaseMode::Dark => "screen",
            BaseMode::Dim => "soft-light",
        }
    }

    /// The seven UI colour tokens, tinted to match the page.
    ///
    /// The tint preserves each token's OWN lightness (which encodes the
    /// hierarchy: page brighter than chrome, chrome brighter than its borders)
    /// and moves only hue (rotated toward the tint hue by strength) and chroma
    /// (base + a strength-scaled amount, capped per token). Because L never
    /// moves, contrast ratios survive a 100% tint.
    pub fn ui_overrides(&self) -> Vec<(&'static str, String)> {
        if !self.has_tint() {
            return Vec::new();
        }
        let t = self.tint_strength as f64 / 100.0;
        // tint_hue is an sRGB angle; the tokens are emitted in OKLCH.
        let target_h = ui_hue_oklch(self.tint_hue as f64);

        // Per-token chroma ceiling at full strength. Large flat areas (paper,
        // surface) need restraint — the strength that looks right on a page
        // is overwhelming across a whole window — while accents are supposed
        // to be saturated.
        //
        // Ink is deliberately near-zero: text carries the reading contrast and
        // a coloured ink on coloured paper is what makes tinted themes feel
        // murky. It picks up a whisper of the hue and nothing more.
        let tokens: [(&'static str, &'static str, f64); 7] = [
            ("--color-paper", "--base-paper", 0.055),
            ("--color-surface", "--base-surface", 0.070),
            ("--color-line", "--base-line", 0.090),
            ("--color-ink", "--base-ink", 0.020),
            ("--color-muted", "--base-muted", 0.045),
            ("--color-accent", "--base-accent", 0.150),
            ("--color-accent-soft", "--base-accent-soft", 0.110),
        ];

        let palette = self.base_palette();
        let mut out = Vec::with_capacity(tokens.len());
        for (name, base_var, max_c) in tokens {
            let Some(hex) = palette.iter().find(|(k, _)| *k == base_var).map(|(_, v)| *v) else {
                continue;
            };
            let Some((l, c0, h0)) = crate::oklch::hex_to_oklch(hex) else {
                continue;
            };

            // Rotate the SHORT way around the circle, so a hue near 350 does
            // not sweep the entire spectrum on its way to 10.
            let mut delta = target_h - h0;
            while delta > 180.0 {
                delta -= 360.0;
            }
            while delta < -180.0 {
                delta += 360.0;
            }
            let h = (h0 + delta * t).rem_euclid(360.0);

            // Near-neutral bases (white paper, gray line) have a meaningless
            // hue, so blend from their own chroma up to the ceiling rather
            // than preserving a hue that was never really there.
            let c = c0 + (max_c - c0).max(0.0) * t;

            out.push((name, crate::oklch::oklch_css(l, c, h)));
        }
        out
    }

    /// The base palette for a mode, as `(token, value)` pairs.
    ///
    /// Mirrors the `:root[data-base=...]` blocks in input.css; only used by
    /// preset thumbnails, which must carry their own look rather than inherit
    /// the live tokens. Keep the two in sync.
    pub(crate) fn base_palette(&self) -> [(&'static str, &'static str); 7] {
        match self.base {
            BaseMode::Light => [
                ("--base-paper", "#ffffff"),
                ("--base-ink", "#1f2937"),
                ("--base-muted", "#6b7280"),
                ("--base-surface", "#f3f4f6"),
                ("--base-line", "#e5e7eb"),
                ("--base-accent", "#2563eb"),
                ("--base-accent-soft", "#dbeafe"),
            ],
            BaseMode::Dark => [
                ("--base-paper", "#131316"),
                ("--base-ink", "#e5e7eb"),
                ("--base-muted", "#9ca3af"),
                ("--base-surface", "#1a1a1e"),
                ("--base-line", "#2b2b31"),
                ("--base-accent", "#60a5fa"),
                ("--base-accent-soft", "#1d2b3a"),
            ],
            BaseMode::Dim => [
                ("--base-paper", "#1a1c1f"),
                ("--base-ink", "#c3c6cb"),
                ("--base-muted", "#8b8f96"),
                ("--base-surface", "#202328"),
                ("--base-line", "#2e3238"),
                ("--base-accent", "#7a9bd4"),
                ("--base-accent-soft", "#232b36"),
            ],
        }
    }

}
#[cfg(test)]
mod tests {
    use crate::appearance::{Appearance, BaseMode};
    use crate::appearance::ui_hue_oklch;
    fn tinted(base: BaseMode, hue: u16, strength: u8) -> Appearance {
        Appearance { base, tint_hue: hue, tint_strength: strength, ..Default::default() }
    }

    #[test]
    fn no_tint_leaves_the_base_filters_untouched() {
        // A plain Light page must have NO filter at all — an identity filter
        // chain still forces a compositing layer and can shift colours through
        // rounding, so "no tint" has to mean literally none.
        assert_eq!(tinted(BaseMode::Light, 34, 0).canvas_filter(), "none");

        // Dark and Dim keep exactly the pipelines the old hand-written CSS had.
        let dark = tinted(BaseMode::Dark, 34, 0).canvas_filter();
        assert!(dark.starts_with("invert(0.92)"), "{dark}");
        assert!(!dark.contains("sepia"), "untinted dark must not colourise: {dark}");

        let dim = tinted(BaseMode::Dim, 34, 0).canvas_filter();
        assert_eq!(dim, "brightness(0.8) saturate(0.75) contrast(0.9)");
        assert!(!dim.contains("invert"), "Dim must preserve document colours");
    }

    #[test]
    fn the_tint_chain_is_appended_after_the_base_not_before() {
        // Order is load-bearing: on Dark the invert must run FIRST so the tint
        // lands on the visible (already inverted) paper.
        let f = tinted(BaseMode::Dark, 200, 60).canvas_filter();
        let inv = f.find("invert").expect("invert present");
        let sep = f.find("sepia").expect("sepia present");
        assert!(inv < sep, "tint must come after the inversion: {f}");
    }

    #[test]
    fn hue_is_absolute_measured_from_sepias_own_output() {
        // sepia() outputs ~34deg. Asking for 34 must therefore rotate by zero,
        // which is what makes `tint_hue` mean the same angle on every base.
        let f = tinted(BaseMode::Light, 34, 50).canvas_filter();
        assert!(f.contains("hue-rotate(0.0deg)"), "{f}");

        // And a request 90deg away rotates by exactly 90.
        let f = tinted(BaseMode::Light, 124, 50).canvas_filter();
        assert!(f.contains("hue-rotate(90.0deg)"), "{f}");
    }

    #[test]
    fn sepia_is_capped_so_photographs_do_not_become_duotone() {
        let f = tinted(BaseMode::Light, 34, 100).canvas_filter();
        // Full strength must still cap at 0.55, not 1.0.
        assert!(f.contains("sepia(0.550)"), "{f}");
    }

    #[test]
    fn strength_scales_the_tint_monotonically() {
        let weak = tinted(BaseMode::Light, 34, 20).canvas_filter();
        let strong = tinted(BaseMode::Light, 34, 80).canvas_filter();
        let grab = |s: &str| -> f64 {
            let i = s.find("sepia(").unwrap() + 6;
            s[i..].split(')').next().unwrap().parse().unwrap()
        };
        assert!(grab(&weak) < grab(&strong));
    }

    #[test]
    fn blend_families_match_the_base() {
        // multiply on a dark canvas would crush the inverted text away.
        assert_eq!(tinted(BaseMode::Light, 0, 0).canvas_blend(), "multiply");
        assert_eq!(tinted(BaseMode::Dark, 0, 0).canvas_blend(), "screen");
        assert_eq!(tinted(BaseMode::Dim, 0, 0).canvas_blend(), "soft-light");
    }

    #[test]
    fn ui_tokens_are_only_overridden_when_a_tint_is_active() {
        assert!(tinted(BaseMode::Light, 34, 0).ui_overrides().is_empty());
        let o = tinted(BaseMode::Light, 34, 50).ui_overrides();
        assert_eq!(o.len(), 7, "all seven tokens must move together");
    }

    /// (L, C, H) of a token in an override set.
    fn lch(o: &[(&'static str, String)], name: &str) -> (f64, f64, f64) {
        let v = &o.iter().find(|(k, _)| *k == name).unwrap().1;
        let inner = v.trim_start_matches("oklch(").trim_end_matches(')');
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>().unwrap());
        (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
    }

    #[test]
    fn the_tint_preserves_each_tokens_lightness_exactly() {
        // THE BUG THIS PREVENTS: mixing toward the tint colour dragged paper,
        // surface and line to a common lightness, so page/sidebar/toolbar/
        // thumbnails merged into one flat slab. Lightness must never move.
        for strength in [10u8, 50, 90, 100] {
            let o = tinted(BaseMode::Light, 104, strength).ui_overrides();
            for (token, base_hex) in [
                ("--color-paper", "#ffffff"),
                ("--color-surface", "#f3f4f6"),
                ("--color-line", "#e5e7eb"),
                ("--color-ink", "#1f2937"),
            ] {
                let want = crate::oklch::hex_to_oklch(base_hex).unwrap().0;
                let got = lch(&o, token).0;
                assert!(
                    (got - want).abs() < 0.001,
                    "{token} at {strength}%: L moved {want} -> {got}"
                );
            }
        }
    }

    #[test]
    fn the_lightness_ladder_survives_a_full_strength_tint() {
        // Page brighter than chrome, chrome brighter than its borders. If this
        // collapses the UI loses all its edges.
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let paper = lch(&o, "--color-paper").0;
        let surface = lch(&o, "--color-surface").0;
        let line = lch(&o, "--color-line").0;
        assert!(paper > surface + 0.01, "paper {paper} vs surface {surface}");
        assert!(surface > line + 0.01, "surface {surface} vs line {line}");

        // ...and inverted in dark mode.
        let d = tinted(BaseMode::Dark, 104, 100).ui_overrides();
        let dpaper = lch(&d, "--color-paper").0;
        let dsurface = lch(&d, "--color-surface").0;
        assert!(dpaper < dsurface, "dark paper must stay the darkest");
    }

    #[test]
    fn the_accent_actually_follows_the_tint_hue() {
        // REGRESSION: the accent used to stay blue on a green tint, because
        // mixing a saturated blue 31% toward green barely moves its hue. At
        // full strength every token must land ON the requested hue.
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let want = ui_hue_oklch(104.0);
        for token in ["--color-paper", "--color-accent", "--color-accent-soft", "--color-line"] {
            let h = lch(&o, token).2;
            let d = (h - want).abs().min(360.0 - (h - want).abs());
            assert!(d < 1.0, "{token} hue {h} should be ~{want}");
        }
    }

    #[test]
    fn hue_rotation_takes_the_short_way_round_the_circle() {
        // A base at ~265deg tinted to 10deg must rotate forward through 300,
        // not sweep backwards through the whole spectrum. At half strength the
        // result should sit between the two, going the short way.
        let o = tinted(BaseMode::Light, 10, 50).ui_overrides();
        let h = lch(&o, "--color-line").2; // base line hue ≈ 265
        let target = ui_hue_oklch(10.0);
        assert!(target < 90.0, "sanity: sRGB 10 maps low, got {target}");
        assert!(
            (280.0..=360.0).contains(&h),
            "expected a short forward rotation through 300, got {h}"
        );
    }

    #[test]
    fn ink_stays_almost_neutral_so_text_does_not_go_murky() {
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let ink_c = lch(&o, "--color-ink").1;
        let paper_c = lch(&o, "--color-paper").1;
        let accent_c = lch(&o, "--color-accent").1;
        assert!(ink_c < 0.03, "ink chroma {ink_c} too colourful");
        assert!(accent_c > paper_c, "the accent must be the most saturated");
    }

    #[test]
    fn chroma_rises_with_strength(){
        let weak = tinted(BaseMode::Light, 104, 20).ui_overrides();
        let strong = tinted(BaseMode::Light, 104, 90).ui_overrides();
        assert!(lch(&weak, "--color-paper").1 < lch(&strong, "--color-paper").1);
    }

    #[test]
    fn the_ui_hue_matches_the_hue_the_page_filter_produces() {
        // COLOUR-SPACE TRAP: `hue-rotate()` works in sRGB, so `tint_hue` is an
        // sRGB angle. The UI tokens are emitted in OKLCH, whose hue circle is
        // rotated relative to sRGB — feeding the raw number straight in made
        // the page go warm tan while the chrome went pink at hue 34.
        // `ui_hue_oklch` maps between them, so both land on the same colour.
        let o = tinted(BaseMode::Light, 34, 100).ui_overrides();
        let h = o
            .iter()
            .find(|(k, _)| *k == "--color-paper")
            .map(|(_, v)| {
                let inner = v.trim_start_matches("oklch(").trim_end_matches(')');
                inner.split_whitespace().nth(2).unwrap().parse::<f64>().unwrap()
            })
            .unwrap();
        // sRGB 34deg (a warm tan) sits near 60deg on the OKLCH circle, NOT 34.
        let want = crate::appearance::ui_hue_oklch(34.0);
        assert!((h - want).abs() < 1.0, "paper hue {h} should be {want}");
        assert!(h > 40.0, "a warm tan must not be emitted as OKLCH 34 (pink)");
    }

    #[test]
    fn dim_is_dark_for_the_ui_but_does_not_invert_the_page() {
        assert!(BaseMode::Dim.is_dark(), "Dim needs the dark UI palette");
        // Dim must keep the document's own colours — that is the reason to
        // pick it over Dark.
        assert!(!tinted(BaseMode::Dim, 0, 0).canvas_filter().contains("invert"));
        assert!(tinted(BaseMode::Dark, 0, 0).canvas_filter().contains("invert"));
    }
}
