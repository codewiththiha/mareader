//! sRGB -> OKLCH conversion, used to tint the UI palette without destroying it.
//!
//! WHY THIS EXISTS. The first tint implementation used CSS
//! `color-mix(in oklch, <base>, <tint> N%)`. Mixing *toward* a mid-lightness
//! colour drags every token toward THAT lightness, so in light mode paper
//! (L=1.00), surface (0.97) and line (0.93) all landed at ~0.88 — they
//! converged. The page, the sidebar, the toolbar and the thumbnail cards
//! became one flat slab of colour with no edges between them.
//!
//! Dark mode hid the problem: its bases start low, so pulling them up left
//! them dark enough to still read as chrome.
//!
//! The fix is to keep each token's OWN lightness — which is what encodes the
//! visual hierarchy — and only move hue and chroma. That cannot be expressed
//! with `color-mix`, and CSS relative-colour syntax would push the work into
//! the browser where it cannot be unit-tested, so the conversion happens here
//! and the result is emitted as an `oklch(L C H)` literal.

/// sRGB gamma -> linear.
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Parse `#rrggbb` into linear-light RGB in 0..=1.
fn hex_to_linear(hex: &str) -> Option<(f64, f64, f64)> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|b| b as f64 / 255.0);
    Some((
        srgb_to_linear(v(0)?),
        srgb_to_linear(v(2)?),
        srgb_to_linear(v(4)?),
    ))
}

/// Lightness, chroma and hue (degrees) of an `#rrggbb` colour in OKLCH.
///
/// Standard Björn Ottosson matrices: linear sRGB -> LMS -> cube root -> OKLab.
#[inline]
pub fn hex_to_oklch(hex: &str) -> Option<(f64, f64, f64)> {
    let (r, g, b) = hex_to_linear(hex)?;

    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;

    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());

    let ll = 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_;
    let aa = 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_;
    let bb = 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_;

    let chroma = (aa * aa + bb * bb).sqrt();
    let mut hue = bb.atan2(aa).to_degrees();
    if hue < 0.0 {
        hue += 360.0;
    }
    Some((ll, chroma, hue))
}

/// A CSS `oklch(...)` literal, rounded so presets compare stably in tests.
#[inline]
pub fn oklch_css(l: f64, c: f64, h: f64) -> String {
    format!("oklch({:.4} {:.4} {:.2})", l.clamp(0.0, 1.0), c.max(0.0), h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_of(hex: &str) -> f64 {
        hex_to_oklch(hex).unwrap().0
    }

    #[test]
    fn known_colours_convert_to_the_expected_oklch() {
        let (l, c, _) = hex_to_oklch("#ffffff").unwrap();
        assert!((l - 1.0).abs() < 0.002, "white L={l}");
        assert!(c < 0.002, "white must be achromatic, c={c}");

        let (l, c, _) = hex_to_oklch("#000000").unwrap();
        assert!(l < 0.002, "black L={l}");
        assert!(c < 0.002);

        // Pure sRGB red is a well-known OKLCH landmark: L≈0.628, C≈0.258, h≈29.2
        let (l, c, h) = hex_to_oklch("#ff0000").unwrap();
        assert!((l - 0.628).abs() < 0.01, "red L={l}");
        assert!((c - 0.258).abs() < 0.01, "red C={c}");
        assert!((h - 29.23).abs() < 1.0, "red h={h}");
    }

    #[test]
    fn the_light_palette_has_the_lightness_ladder_the_ui_depends_on() {
        // This ordering IS the visual hierarchy: page brighter than chrome,
        // chrome brighter than its borders. The tint must preserve it — losing
        // it is exactly the bug this module exists to fix.
        let paper = l_of("#ffffff");
        let surface = l_of("#f3f4f6");
        let line = l_of("#e5e7eb");
        assert!(paper > surface, "{paper} > {surface}");
        assert!(surface > line, "{surface} > {line}");
    }

    #[test]
    fn the_dark_palette_has_the_same_ladder_inverted() {
        let paper = l_of("#131316");
        let surface = l_of("#1a1a1e");
        let line = l_of("#2b2b31");
        assert!(paper < surface, "dark paper must be the DARKEST");
        assert!(surface < line);
    }

    #[test]
    fn malformed_input_is_rejected_rather_than_panicking() {
        assert!(hex_to_oklch("#fff").is_none());
        assert!(hex_to_oklch("nonsense").is_none());
        assert!(hex_to_oklch("#gggggg").is_none());
        assert!(hex_to_oklch("").is_none());
    }

    #[test]
    fn css_output_is_stable_and_clamped() {
        assert_eq!(oklch_css(0.5, 0.1, 138.5), "oklch(0.5000 0.1000 138.50)");
        // Out-of-range lightness must not emit an invalid colour.
        assert_eq!(oklch_css(1.7, -0.2, 10.0), "oklch(1.0000 0.0000 10.00)");
    }
}
