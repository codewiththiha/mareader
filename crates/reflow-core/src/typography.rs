//! The reader's typography as the layout maths need it.
//!
//! The settings themselves (`reader_core::settings::typography`) are the
//! persisted schema, and the bridge from that schema to the interface (a
//! font choice becoming a CSS stack, the whole setting becoming the scale-1
//! custom properties) lives with it in the same crate — presentation of a
//! settings blob, not page layout. What stays here is the one number the
//! PAGINATOR reaches in for ([`body_char_width`]), which is why the estimate
//! and the rendered text can never drift apart on the font.
//!
//! The schema types are re-exported so a component that reads a knob and
//! paints it imports from one crate.

pub use reader_core::settings::typography::{FontChoice, TextFamily, TextSettings, builtin_fonts};

pub use reader_core::settings::typography::{
    BuiltInFont, DEFAULT_FONT_SIZE, DEFAULT_INK_CONTRAST, DEFAULT_LINE_HEIGHT,
    DEFAULT_PARAGRAPH_MARGIN, SystemFont, TextColumnAlign, sanitize,
};

/// Average glyph advance (fraction of the font size) for the body font —
/// the pagination estimate's per-character width.
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
