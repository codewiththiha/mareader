//! Texture mode -> the two custom properties the textures stylesheet
//! resolves: the opacity dial and the user pitch multiplier. Shared by both
//! carriers on purpose — the PDF page's per-page pattern (`.pdf-page::before`)
//! and the reflowable scroller's background pattern resolve the same two, so
//! the dials feed both formats.
//!
//! The `texture-*` carrier class is NOT emitted here:
//! [`TextureMode::css_class`](crate::appearance::TextureMode::css_class)
//! owns that naming, and the class rides on the carrier element (the PDF page
//! host, the reflowable scroller) rather than on `<html>`. The bare mode word
//! — `as_str` — is what the `data-texture` attribute and the CSS
//! `&[class*="texture-"]` scan read.

use crate::appearance::Appearance;

/// `--texture-opacity` and `--texture-scale-user`, written on `<html>` once
/// per appearance change. The stylesheet multiplies the user pitch into the
/// page's own `--scale-factor`, so the pattern zooms with the document.
pub fn css_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    vec![
        (
            "--texture-opacity",
            format!("{:.3}", a.texture_opacity as f64 / 100.0),
        ),
        (
            "--texture-scale-user",
            format!("{:.3}", a.texture_scale as f64 / 100.0),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pair_is_emitted_as_unit_fractions() {
        let a = Appearance { texture_opacity: 90, texture_scale: 150, ..Default::default() };
        let vars = css_vars(&a);
        assert_eq!(vars[0], ("--texture-opacity", "0.900".to_string()));
        assert_eq!(vars[1], ("--texture-scale-user", "1.500".to_string()));
    }
}
