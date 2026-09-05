//! The UI-token side of the PDF tint: the seven `--color-*` overrides the
//! filter pipeline rides along with.
//!
//! The tint preserves each token's OWN lightness (which encodes the
//! hierarchy: page brighter than chrome, chrome brighter than its borders)
//! and moves only hue (rotated toward the tint hue by strength) and chroma
//! (base + a strength-scaled amount, capped per token). Because L never
//! moves, contrast ratios survive a 100% tint. The rotation and the ceiling
//! table live in the shared kernel — the text palette reuses both so the
//! two formats tint to the same hue at the same strength.

use crate::appearance::shared::tint::{chroma_ceiling, tinted_token, ui_hue_oklch};
use crate::appearance::Appearance;

impl Appearance {
    /// Just the accent token, without building the seven-entry override list.
    pub fn tinted_accent(&self) -> Option<String> {
        if !self.has_tint() {
            return None;
        }
        let t = self.tint_strength as f64 / 100.0;
        let target_h = ui_hue_oklch(self.tint_hue as f64);
        let hex = self
            .base_palette()
            .into_iter()
            .find(|(k, _)| *k == "--base-accent")
            .map(|(_, v)| v)?;
        tinted_token(hex, target_h, t, chroma_ceiling("accent"))
    }

    /// The seven UI colour tokens, tinted to match the page.
    ///
    /// Emitted as `--color-*` pairs; empty when no tint is active. The
    /// per-token chroma ceilings live in the shared kernel so the text
    /// palette hits the same numbers.
    pub fn ui_overrides(&self) -> Vec<(&'static str, String)> {
        if !self.has_tint() {
            return Vec::new();
        }
        let t = self.tint_strength as f64 / 100.0;
        // tint_hue is an sRGB angle; the tokens are emitted in OKLCH.
        let target_h = ui_hue_oklch(self.tint_hue as f64);

        let tokens: [(&'static str, &'static str, f64); 7] = [
            ("--color-paper", "--base-paper", chroma_ceiling("paper")),
            ("--color-surface", "--base-surface", chroma_ceiling("surface")),
            ("--color-line", "--base-line", chroma_ceiling("line")),
            ("--color-ink", "--base-ink", chroma_ceiling("ink")),
            ("--color-muted", "--base-muted", chroma_ceiling("muted")),
            ("--color-accent", "--base-accent", chroma_ceiling("accent")),
            ("--color-accent-soft", "--base-accent-soft", chroma_ceiling("accent-soft")),
        ];

        let palette = self.base_palette();
        let mut out = Vec::with_capacity(tokens.len());
        for (name, base_var, max_c) in tokens {
            let Some(hex) = palette.iter().find(|(k, _)| *k == base_var).map(|(_, v)| *v) else {
                continue;
            };
            let Some(value) = tinted_token(hex, target_h, t, max_c) else {
                continue;
            };
            out.push((name, value));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::appearance::fixture::{lch, tinted};
    use crate::appearance::shared::oklch::hex_to_oklch;
    use crate::appearance::shared::tint::ui_hue_oklch;
    use crate::appearance::BaseMode;

    /// (L, C, H) of one token in an override set.
    fn token_lch(o: &[(&'static str, String)], name: &str) -> (f64, f64, f64) {
        lch(&o.iter().find(|(k, _)| *k == name).unwrap().1)
    }

    #[test]
    fn ui_tokens_are_only_overridden_when_a_tint_is_active() {
        assert!(tinted(BaseMode::Light, 34, 0).ui_overrides().is_empty());
        let o = tinted(BaseMode::Light, 34, 50).ui_overrides();
        assert_eq!(o.len(), 7, "all seven tokens must move together");
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
                let want = hex_to_oklch(base_hex).unwrap().0;
                let got = token_lch(&o, token).0;
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
        let paper = token_lch(&o, "--color-paper").0;
        let surface = token_lch(&o, "--color-surface").0;
        let line = token_lch(&o, "--color-line").0;
        assert!(paper > surface + 0.01, "paper {paper} vs surface {surface}");
        assert!(surface > line + 0.01, "surface {surface} vs line {line}");

        // ...and inverted in dark mode.
        let d = tinted(BaseMode::Dark, 104, 100).ui_overrides();
        let dpaper = token_lch(&d, "--color-paper").0;
        let dsurface = token_lch(&d, "--color-surface").0;
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
            let h = token_lch(&o, token).2;
            let d = (h - want).abs().min(360.0 - (h - want).abs());
            assert!(d < 1.0, "{token} hue {h} should be ~{want}");
        }
    }

    #[test]
    fn ink_stays_almost_neutral_so_text_does_not_go_murky() {
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let ink_c = token_lch(&o, "--color-ink").1;
        let paper_c = token_lch(&o, "--color-paper").1;
        let accent_c = token_lch(&o, "--color-accent").1;
        assert!(ink_c < 0.03, "ink chroma {ink_c} too colourful");
        assert!(accent_c > paper_c, "the accent must be the most saturated");
    }

    #[test]
    fn chroma_rises_with_strength() {
        let weak = tinted(BaseMode::Light, 104, 20).ui_overrides();
        let strong = tinted(BaseMode::Light, 104, 90).ui_overrides();
        assert!(token_lch(&weak, "--color-paper").1 < token_lch(&strong, "--color-paper").1);
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
            .map(|(_, v)| lch(v).2)
            .unwrap();
        // sRGB 34deg (a warm tan) sits near 60deg on the OKLCH circle, NOT 34.
        let want = ui_hue_oklch(34.0);
        assert!((h - want).abs() < 1.0, "paper hue {h} should be {want}");
        assert!(h > 40.0, "a warm tan must not be emitted as OKLCH 34 (pink)");
    }
}
