//! sRGB <-> OKLCH conversion, used to tint the UI palette without destroying it.
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
//!
//! The module lives in the shared kernel because BOTH format pipelines
//! compute in this space: the PDF tint emits OKLCH UI tokens, and the text
//! palette derives its page colours in OKLCH.

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

/// (L, C, H) out of a colour this reader emits: an `oklch(...)` literal or an
/// `#rrggbb` hex.
///
/// The inverse of [`oklch_css`], and the only place the literal's grammar is
/// written down. Untinted palettes emit hex and tinted ones emit oklch, so
/// anything that reads a palette back — [`crate::appearance::reflowable::palette`]'s
/// precomposed mixes, and the tests that hold every emitted number to account
/// — has to accept both.
pub fn parse_color(value: &str) -> Option<(f64, f64, f64)> {
    let v = value.trim();
    if let Some(inner) = v.strip_prefix("oklch(").and_then(|s| s.strip_suffix(')')) {
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>().ok());
        return Some((it.next().flatten()?, it.next().flatten()?, it.next().flatten()?));
    }
    hex_to_oklch(v)
}

/// Hue `from` rotated a fraction `t` of the way to `to`, the SHORT way round
/// the circle.
///
/// The trap this exists for: hue is an angle, so 350 -> 10 is a 20 degree step
/// forward and not a 340 degree sweep backwards through the whole spectrum. A
/// tint that took the long way turned a warm paper green on its way to red.
/// Both the UI-token tint and the text palette's accent ride it, so one slider
/// moves every colour the same direction.
pub fn hue_toward(from: f64, to: f64, t: f64) -> f64 {
    let mut delta = to - from;
    while delta > 180.0 {
        delta -= 360.0;
    }
    while delta < -180.0 {
        delta += 360.0;
    }
    (from + delta * t).rem_euclid(360.0)
}

/// A CSS `oklch(...)` literal, rounded so presets compare stably in tests.
#[inline]
pub fn oklch_css(l: f64, c: f64, h: f64) -> String {
    format!(
        "oklch({:.4} {:.4} {:.2})",
        l.clamp(0.0, 1.0),
        c.max(0.0),
        h.rem_euclid(360.0)
    )
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
    fn an_emitted_literal_reads_back_as_the_numbers_it_was_built_from() {
        // The round trip both pipelines depend on: tinted palettes emit oklch,
        // untinted ones emit hex, and one parser has to read either.
        let (l, c, h) = parse_color(&oklch_css(0.98, 0.08, 104.0)).unwrap();
        assert!((l - 0.98).abs() < 1e-9 && (c - 0.08).abs() < 1e-9 && (h - 104.0).abs() < 1e-9);
        assert!((parse_color("#ffffff").unwrap().0 - 1.0).abs() < 0.002);
        assert!(parse_color("oklch(nonsense)").is_none());
        assert!(parse_color("currentColor").is_none());
    }

    #[test]
    fn a_hue_walks_the_short_way_and_lands_where_it_is_told() {
        assert!((hue_toward(350.0, 10.0, 1.0) - 10.0).abs() < 1e-9);
        assert!((hue_toward(350.0, 10.0, 0.5) - 0.0).abs() < 1e-9);
        assert!((hue_toward(10.0, 350.0, 0.5) - 0.0).abs() < 1e-9);
        // Full strength is the target exactly, and no strength moves at all.
        assert!((hue_toward(104.0, 200.0, 1.0) - 200.0).abs() < 1e-9);
        assert!((hue_toward(104.0, 200.0, 0.0) - 104.0).abs() < 1e-9);
    }

    #[test]
    fn css_output_is_stable_and_clamped() {
        assert_eq!(oklch_css(0.5, 0.1, 138.5), "oklch(0.5000 0.1000 138.50)");
        // Out-of-range lightness must not emit an invalid colour.
        assert_eq!(oklch_css(1.7, -0.2, 10.0), "oklch(1.0000 0.0000 10.00)");
    }
}
