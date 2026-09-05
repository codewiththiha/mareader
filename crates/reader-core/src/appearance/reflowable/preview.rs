//! The preset thumbnail in the TEXT palette — what a text/Markdown page
//! will actually get when the preset is applied.
//!
//! The PDF swatch previews the raster pipeline (base palette + tinted
//! UI-token overrides); this one previews [`TextPalette`], so a preset
//! read while a text document is open shows the page the reader would
//! paint for it — bright light paper in Light mode, dark in Dark, grey
//! with dark ink in Dim. Same `--ps-*` private namespace, same
//! texture/noise tail as the PDF swatch (see `appearance::preview`); only
//! the colour tokens differ.

use crate::appearance::preview::ps_surface_tail;
use crate::appearance::Appearance;
use super::palette::TextPalette;

impl Appearance {
    /// Inline `style` for a preset thumbnail, rendered with the text-page
    /// palette instead of the PDF token path. Everything else — the
    /// `--ps-*` namespace discipline, the texture and noise vars — is the
    /// shared swatch tail.
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
        // The texture strokes key off the page's OWN paper, not the chrome
        // base: a dim TEXT page is medium-dark paper, so it takes the dark
        // family like the Dark palette does.
        out.push_str(&ps_surface_tail(self, p.paper_l < 0.5));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::fixture::tinted;
    use crate::appearance::BaseMode;

    #[test]
    fn the_text_swatch_carries_the_text_palette() {
        let a = tinted(BaseMode::Light, 104, 50);
        let style = a.text_preview_style();
        let p = TextPalette::compute(&a);
        assert!(style.contains(&format!("--ps-color-paper:{};", p.paper)), "{style}");
        assert!(style.contains(&format!("--ps-color-ink:{};", p.ink)), "{style}");
        // Private namespace only, like the PDF swatch: no root-mutated
        // names may leak in, or WKWebView repaints the swatch every frame
        // of a slider drag (see appearance::preview).
        for root_mutated in ["--canvas-filter:", "--color-paper:", "--tx-paper:"] {
            assert!(!style.contains(root_mutated), "{root_mutated} leaked: {style}");
        }
    }

    #[test]
    fn the_texture_strokes_follow_the_paper_not_the_chrome() {
        // Dim TEXT pages sit on medium-dark paper (L 0.40), so their
        // strokes take the dark family, same as the Dark palette's.
        let a = tinted(BaseMode::Dim, 0, 0);
        let style = a.text_preview_style();
        assert!(style.contains("--ps-texture-blend:screen"), "{style}");

        let dark = tinted(BaseMode::Dark, 0, 0);
        assert!(dark.text_preview_style().contains("--ps-texture-blend:screen"));

        let light = tinted(BaseMode::Light, 0, 0);
        assert!(light.text_preview_style().contains("--ps-texture-blend:multiply"));
    }
}
