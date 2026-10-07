//! The preset thumbnail in the TEXT palette: what a text page would
//! actually get.

use super::palette::TextPalette;
use crate::appearance::Appearance;
use crate::appearance::preview::ps_surface_tail;

impl Appearance {
    /// Inline `style` for a preset thumbnail in the text-page palette.
    pub fn text_preview_style(&self) -> String {
        let p = TextPalette::compute(self);
        let mut out = String::new();
        for (token, value) in [
            ("paper", &p.paper),
            ("ink", &p.ink),
            ("muted", &p.muted),
            ("surface", &p.surface),
            ("line", &p.line),
            ("accent", &p.accent),
            ("accent-soft", &p.accent_soft),
        ] {
            out.push_str(&format!("--ps-color-{token}:{value};"));
        }
        // The strokes key off the page's OWN paper, not the chrome base.
        out.push_str(&ps_surface_tail(self, p.paper_l < 0.5));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::BaseMode;
    use crate::appearance::fixture::tinted;

    #[test]
    fn the_text_swatch_carries_the_text_palette() {
        let a = tinted(BaseMode::Light, 104, 50);
        let style = a.text_preview_style();
        let p = TextPalette::compute(&a);
        assert!(
            style.contains(&format!("--ps-color-paper:{};", p.paper)),
            "{style}"
        );
        assert!(
            style.contains(&format!("--ps-color-ink:{};", p.ink)),
            "{style}"
        );
        // Private namespace only, like the PDF swatch.
        for root_mutated in ["--canvas-filter:", "--color-paper:", "--tx-paper:"] {
            assert!(
                !style.contains(root_mutated),
                "{root_mutated} leaked: {style}"
            );
        }
    }

    #[test]
    fn the_texture_strokes_follow_the_paper_not_the_chrome() {
        // Dim TEXT pages sit on medium-dark paper: the dark stroke family.
        let a = tinted(BaseMode::Dim, 0, 0);
        let style = a.text_preview_style();
        assert!(style.contains("--ps-texture-blend:screen"), "{style}");

        let dark = tinted(BaseMode::Dark, 0, 0);
        assert!(
            dark.text_preview_style()
                .contains("--ps-texture-blend:screen")
        );

        let light = tinted(BaseMode::Light, 0, 0);
        assert!(
            light
                .text_preview_style()
                .contains("--ps-texture-blend:multiply")
        );
    }
}
