//! The one number the paginator needs from the reader's typography.

pub use reader_core::settings::typography::{
    FontChoice, SystemFont, TextColumnAlign, TextSettings, sanitize,
};

// This module's own needs: the family table `body_char_width` reads.
use reader_core::settings::typography::{TextFamily, builtin_fonts};

/// Average glyph advance of the body font: the estimate's per-char width.
pub fn body_char_width(settings: &TextSettings) -> f64 {
    match &settings.default_font {
        FontChoice::System(f) => f.avg_char_width(),
        FontChoice::BuiltIn(id) => builtin_fonts()
            .iter()
            .find(|f| f.id == id.as_str())
            .map(|f| match f.family {
                TextFamily::Monospace => 0.6,
                TextFamily::Serif => 0.5,
                TextFamily::SansSerif => 0.52,
            })
            .unwrap_or(0.5),
        FontChoice::Default => 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_width_estimates_stay_inside_the_sanity_band() {
        // The unknown/unmatched face falls back to the serif estimate.
        let mut s = TextSettings {
            default_font: FontChoice::BuiltIn("not-shipped-yet".into()),
            ..TextSettings::default()
        };
        assert_eq!(body_char_width(&s), 0.5);
        // Every bundled font lands in the readable band.
        for f in builtin_fonts() {
            s.default_font = FontChoice::BuiltIn(f.id.to_string());
            let w = body_char_width(&s);
            assert!(w > 0.45 && w <= 0.6, "{}: {w}", f.id);
        }
        s.default_font = FontChoice::Default;
        assert_eq!(body_char_width(&s), 0.5);
    }
}
