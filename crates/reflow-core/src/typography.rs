//! Resolving the reader's typography into what the browser paints.
//!
//! The settings themselves (`reader_core::settings::typography`) are the
//! persisted schema; this module is the one bridge from that schema to the
//! interface: a font choice becomes a CSS font stack, and the whole setting
//! becomes the scale-1 custom properties the stylesheet reads. Pagination
//! reaches in for one number only ([`body_char_width`]), which is why the
//! estimate and the rendered text can never drift apart on the font.
//!
//! The schema types are re-exported so a component that reads a knob and
//! paints it imports from one crate.

pub use reader_core::settings::typography::{
    TextFamily, TextSettings, builtin_fonts, FontChoice,
};

pub use reader_core::settings::typography::{
    BuiltInFont, DEFAULT_FONT_SIZE, DEFAULT_INK_CONTRAST, DEFAULT_LINE_HEIGHT,
    DEFAULT_PARAGRAPH_MARGIN, SystemFont, TextColumnAlign, sanitize,
};

/// The serif stack the body falls back to when no default font is chosen:
/// the classic book-reading faces, in availability order.
const SERIF_STACK: &str =
    "Charter, \"Bitstream Charter\", \"Iowan Old Style\", Georgia, \"Times New Roman\", serif";
/// The sans family's natural stack.
const SANS_STACK: &str =
    "ui-sans, -apple-system, \"Segoe UI\", Helvetica, Arial, sans-serif";
/// The monospace family's natural stack.
const MONO_STACK: &str =
    "ui-mono, Menlo, Consolas, \"Liberation Mono\", \"Courier New\", monospace";

/// The natural stack of a family — what a `Default` family slot resolves to.
fn family_default_stack(family: TextFamily) -> &'static str {
    match family {
        TextFamily::Serif => SERIF_STACK,
        TextFamily::SansSerif => SANS_STACK,
        TextFamily::Monospace => MONO_STACK,
    }
}

/// Resolve one font choice to a CSS stack.
///
/// * `Default` in a FAMILY slot resolves to that family's natural stack.
/// * `Default` in the BODY slot (`family == None`) reads in the SERIF
///   face — whatever the Serif slot currently resolves to, not the bare
///   constant — so the Serif picker is what shapes default body text, and
///   the Default picker is the override that takes body away from it.
/// * A bundled font resolves to its own stack; a bundled id the build does
///   not (yet) ship falls back the same way as `Default`, so a saved choice
///   never renders nothing.
fn resolve_stack(settings: &TextSettings, choice: &FontChoice, family: Option<TextFamily>) -> String {
    match choice {
        FontChoice::Default => match family {
            Some(family) => family_default_stack(family).to_string(),
            None => family_stack(settings, TextFamily::Serif),
        },
        FontChoice::System(f) => f.stack(),
        FontChoice::BuiltIn(id) => builtin_fonts()
            .iter()
            .find(|f| f.id == id.as_str())
            .map(|f| f.stack.to_string())
            .unwrap_or_else(|| {
                family
                    .map(family_default_stack)
                    .unwrap_or(SERIF_STACK)
                    .to_string()
            }),
    }
}

/// The stack body text renders in: the Default picker's choice, or the
/// Serif slot when that choice is `Default` (see [`resolve_stack`]).
fn body_stack(settings: &TextSettings) -> String {
    resolve_stack(settings, &settings.default_font, None)
}

/// The stack a family renders in, honouring its override slot.
fn family_stack(settings: &TextSettings, family: TextFamily) -> String {
    let choice = match family {
        TextFamily::Serif => &settings.serif_font,
        TextFamily::SansSerif => &settings.sans_font,
        TextFamily::Monospace => &settings.mono_font,
    };
    resolve_stack(settings, choice, Some(family))
}

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

/// The settings as CSS custom properties, all at SCALE 1 — the page applies
/// its own `--ts` multiplier on top, so a zoom never repaints these.
///
/// The page-side contract: `--tx-font-size`, `--tx-line-height`,
/// `--tx-para-margin`, `--tx-word-spacing`, `--tx-letter-spacing`,
/// `--tx-text-indent`, `--tx-text-align`, `--tx-hyphens`, `--tx-font-body`,
/// `--tx-font-sans`, `--tx-font-mono`.
///
/// The ink dial is deliberately NOT here: it is resolved in Rust by the
/// appearance pipeline (reader-core's `appearance::reflowable`), which mixes the
/// palette ink toward the paper itself and paints a flat `--tx-ink` — the
/// stylesheet never mixes live. Column alignment is also NOT here — it
/// positions a container (a class on the stream column), not a value any
/// rule of the type itself resolves through.
pub fn css_variables(settings: &TextSettings) -> Vec<(&'static str, String)> {
    vec![
        ("--tx-font-size", format!("{}px", format_px(settings.font_size))),
        ("--tx-line-height", format!("{:.3}", settings.line_height)),
        ("--tx-para-margin", format!("{}em", format_em(settings.paragraph_margin))),
        ("--tx-word-spacing", format!("{}px", format_px(settings.word_spacing))),
        ("--tx-letter-spacing", format!("{}em", format_em(settings.letter_spacing))),
        ("--tx-text-indent", format!("{}em", format_em(settings.text_indent))),
        (
            "--tx-text-align",
            if settings.justify { "justify" } else { "start" }.to_string(),
        ),
        (
            "--tx-hyphens",
            if settings.hyphenation { "auto" } else { "none" }.to_string(),
        ),
        ("--tx-font-weight", settings.font_weight.to_string()),
        ("--tx-font-body", body_stack(settings)),
        ("--tx-font-sans", family_stack(settings, TextFamily::SansSerif)),
        ("--tx-font-mono", family_stack(settings, TextFamily::Monospace)),
    ]
}

/// Trim a px value to at most 2 decimals, without trailing zeros the CSS
/// does not need (`17`, `16.5`, `16.25`).
fn format_px(v: f64) -> String {
    format_scaled(v, 2)
}

fn format_em(v: f64) -> String {
    format_scaled(v, 3)
}

fn format_scaled(v: f64, decimals: u32) -> String {
    let factor = 10f64.powi(decimals as i32);
    let scaled = (v * factor).round() / factor;
    if scaled == scaled.trunc() {
        format!("{}", scaled as i64)
    } else {
        format!("{scaled}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stacks_resolve_through_every_slot() {
        let mut s = TextSettings::default();
        // Body default: the serif reading stack.
        assert_eq!(body_stack(&s), SERIF_STACK);
        // Body default FOLLOWS the Serif slot...
        s.serif_font = FontChoice::System(SystemFont::Georgia);
        assert_eq!(body_stack(&s), "Georgia, serif");
        // ...unless the body itself picks a face, which wins outright.
        s.default_font = FontChoice::System(SystemFont::Verdana);
        assert_eq!(body_stack(&s), "Verdana, sans-serif");
        s.default_font = FontChoice::Default;
        s.serif_font = FontChoice::Default;
        // Family slots: Default keeps the natural stack...
        assert_eq!(family_stack(&s, TextFamily::SansSerif), SANS_STACK);
        assert_eq!(family_stack(&s, TextFamily::Monospace), MONO_STACK);
        // ...an override replaces it.
        s.mono_font = FontChoice::System(SystemFont::Consolas);
        assert_eq!(family_stack(&s, TextFamily::Monospace), "Consolas, monospace");
        // An unshipped bundled font falls back to the natural stack.
        s.serif_font = FontChoice::BuiltIn("not-shipped-yet".into());
        assert_eq!(family_stack(&s, TextFamily::Serif), SERIF_STACK);
    }

    #[test]
    fn css_variables_carry_the_full_contract() {
        let mut s = TextSettings::default();
        s.justify = true;
        s.hyphenation = true;
        s.font_size = 18.0;
        let vars: Vec<String> = css_variables(&s).into_iter().map(|(k, v)| format!("{k}:{v}")).collect();
        let joined = vars.join(";");
        assert!(joined.contains("--tx-font-size:18px"), "{joined}");
        assert!(joined.contains("--tx-text-align:justify"), "{joined}");
        assert!(joined.contains("--tx-hyphens:auto"), "{joined}");
        assert!(joined.contains("--tx-line-height:1.7"), "{joined}");
        assert!(joined.contains("--tx-font-weight:400"), "{joined}");
        assert!(joined.contains("--tx-font-body:"), "{joined}");
        assert!(joined.contains("--tx-font-sans:"), "{joined}");
        assert!(joined.contains("--tx-font-mono:"), "{joined}");
        // The ink dial is NOT part of this contract: it resolves in Rust
        // (appearance::reflowable) and paints as a flat --tx-ink.
        assert!(!joined.contains("--tx-ink-contrast"), "{joined}");
    }

    #[test]
    fn px_formatting_drops_unneeded_decimals() {
        assert_eq!(format_px(17.0), "17");
        assert_eq!(format_px(16.5), "16.5");
        assert_eq!(format_px(-0.5), "-0.5");
        assert_eq!(format_em(1.0), "1");
    }

}
