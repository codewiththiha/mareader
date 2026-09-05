//! The tint maths BOTH pipelines share: the sRGB->OKLCH hue mapping and
//! the per-token OKLCH shift that preserves each token's lightness.
//!
//! The PDF pipeline feeds these numbers into the CSS filter chain and the
//! UI-token overrides; the text pipeline writes them straight into its
//! page tokens. Keeping the mapping and the per-token chroma ceilings here
//! — and nowhere else — is what makes one slider drive both formats onto
//! the same hue at the same strength.

use crate::appearance::shared::oklch::{hex_to_oklch, hue_toward, oklch_css};

/// Map an sRGB hue angle (`tint_hue`, applied via `hue-rotate()`) to the
/// corresponding OKLCH hue angle (what the tokens are emitted in). The two
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
    hex_to_oklch(&hex).map(|(_, _, h)| h).unwrap_or(srgb_hue)
}

/// The chroma ceiling a token may reach at a 100% tint.
///
/// Large flat areas (paper, surface) need restraint — the strength that
/// looks right on a page is overwhelming across a whole window — while
/// accents are supposed to be saturated. Ink is deliberately near-zero:
/// text carries the reading contrast and a coloured ink on coloured paper
/// is what makes tinted themes feel murky; it picks up a whisper of the
/// hue and nothing more.
pub fn chroma_ceiling(token: &str) -> f64 {
    match token {
        "paper" => 0.055,
        "surface" => 0.070,
        "line" => 0.090,
        "ink" => 0.020,
        "muted" => 0.045,
        "accent" => 0.150,
        "accent-soft" => 0.110,
        _ => 0.055,
    }
}

/// Shift one token colour toward the tint at strength `t` (0..=1).
///
/// The tint preserves the token's OWN lightness — which encodes the
/// hierarchy: page brighter than chrome, chrome brighter than its borders —
/// and moves only hue (rotated the SHORT way toward `target_h`, so a hue
/// near 350 does not sweep the whole spectrum on its way to 10) and chroma
/// (lifted toward the token's ceiling). Because L never moves, contrast
/// ratios survive a 100% tint.
pub fn tinted_token(hex: &str, target_h: f64, t: f64, max_c: f64) -> Option<String> {
    let (l, c0, h0) = hex_to_oklch(hex)?;
    let h = hue_toward(h0, target_h, t);

    // Near-neutral bases (white paper, gray line) have a meaningless hue,
    // so blend from their own chroma up to the ceiling rather than
    // preserving a hue that was never really there.
    let c = c0 + (max_c - c0).max(0.0) * t;

    Some(oklch_css(l, c, h))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::fixture::lch;

    #[test]
    fn the_srgb_hue_maps_onto_the_oklch_circle_not_identity() {
        // COLOUR-SPACE TRAP: `hue-rotate()` works in sRGB, so `tint_hue` is an
        // sRGB angle. The tokens are emitted in OKLCH, whose hue circle is
        // rotated relative to sRGB — sRGB 34 (a warm tan) sits near 60 on the
        // OKLCH circle, NOT 34. Feeding the raw number straight in made the
        // page go warm tan while the chrome went pink at hue 34.
        let h = ui_hue_oklch(34.0);
        assert!(h > 40.0, "a warm tan must not be emitted as OKLCH 34 (pink)");
        assert!(h < 90.0, "sanity: tan should sit below the green corner");
    }

    #[test]
    fn tinting_preserves_lightness_exactly() {
        for strength in [10.0, 50.0, 90.0, 100.0] {
            let t = strength / 100.0;
            let target = ui_hue_oklch(104.0);
            for (hex, ceiling) in [
                ("#ffffff", 0.055),
                ("#f3f4f6", 0.070),
                ("#1f2937", 0.020),
            ] {
                let want = hex_to_oklch(hex).unwrap().0;
                let got = tinted_token(hex, target, t, ceiling).map(|v| lch(&v).0).unwrap();
                assert!((got - want).abs() < 0.001, "{hex} at {strength}%: L moved {want} -> {got}");
            }
        }
    }

    #[test]
    fn a_full_strength_tint_lands_on_the_requested_hue() {
        // Near-neutral bases rotate from a meaningless hue, so at 100% every
        // token must land ON the requested hue rather than near it.
        let target = ui_hue_oklch(104.0);
        for hex in ["#ffffff", "#2563eb", "#e5e7eb"] {
            let h = tinted_token(hex, target, 1.0, 0.150).map(|v| lch(&v).2).unwrap();
            let d = (h - target).abs().min(360.0 - (h - target).abs());
            assert!(d < 1.0, "{hex} hue {h} should be ~{target}");
        }
    }

    #[test]
    fn hue_rotation_takes_the_short_way_round_the_circle() {
        // A base at ~265deg tinted to 10deg must rotate forward through 300,
        // not sweep backwards through the whole spectrum. At half strength the
        // result should sit between the two, going the short way.
        let target = ui_hue_oklch(10.0);
        assert!(target < 90.0, "sanity: sRGB 10 maps low, got {target}");
        let v = tinted_token("#e5e7eb", target, 0.5, 0.090).unwrap();
        let h = lch(&v).2;
        assert!(
            (280.0..=360.0).contains(&h),
            "expected a short forward rotation through 300, got {h}"
        );
    }

    #[test]
    fn the_ceiling_table_keeps_the_ink_near_neutral_and_the_accent_loud() {
        assert!(chroma_ceiling("ink") < 0.03, "ink must stay almost neutral");
        assert!(chroma_ceiling("accent") > chroma_ceiling("paper"));
        assert_eq!(chroma_ceiling("paper"), chroma_ceiling("unknown-token"));
    }
}
