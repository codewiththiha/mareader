//! The raster filter pipeline: the CSS filter chain that turns the
//! always-light PDF raster into the reader's base mode and tint, and the
//! blend mode that composites it over the page backdrop.
//!
//! A translucent colour wash would muddy the text, so the tint is a
//! `sepia() saturate() hue-rotate()` chain; the UI tokens ride along in
//! OKLCH (see [`super::tint`]), emitted with each token's OWN lightness
//! preserved — only hue and chroma move, so contrast ratios survive a
//! 100% tint.

use crate::appearance::{Appearance, BaseMode};

impl Appearance {
    pub fn canvas_filter(&self) -> String {
        let mut parts: Vec<String> = Vec::new();

        match self.base {
            BaseMode::Light => {}
            BaseMode::Dark => {
                parts.push("invert(0.92)".into());
                parts.push("hue-rotate(180deg)".into());
                parts.push("saturate(0.85)".into());
                parts.push("brightness(1.02)".into());
            }
            BaseMode::Dim => {
                parts.push("brightness(0.8)".into());
                parts.push("saturate(0.75)".into());
                parts.push("contrast(0.9)".into());
            }
        }

        if self.has_tint() {
            let t = self.tint_strength as f64 / 100.0;
            // Cap sepia at 0.55: past that the collapse starts eating real
            // colour in figures and photographs, and the page reads as a
            // duotone print rather than tinted paper.
            let sep = (t * 0.55).clamp(0.0, 0.55);
            // Sepia flattens chroma; give it back proportionally so a strong
            // tint reads as saturated rather than merely beige.
            let sat = 1.0 + t * 0.6;
            // sepia() lands around 34deg (a warm brown). Measure the requested
            // hue from there so tint_hue is an absolute target, not an offset.
            let rot = (self.tint_hue as f64) - 34.0;
            parts.push(format!("sepia({sep:.3})"));
            parts.push(format!("saturate({sat:.3})"));
            parts.push(format!("hue-rotate({rot:.1}deg)"));
        }

        if parts.is_empty() {
            "none".to_string()
        } else {
            parts.join(" ")
        }
    }

    /// Blend mode for the canvas against the page background.
    ///
    /// `multiply` keeps light themes paper-like. Inverted canvases need
    /// `screen` (multiply can only darken, so it would crush the near-white
    /// inverted text back into the dark page and destroy readability).
    /// Dim is not inverted but is darkened, and soft-light preserves its
    /// midtones where multiply would double up the darkening.
    pub fn canvas_blend(&self) -> &'static str {
        match self.base {
            BaseMode::Light => "multiply",
            BaseMode::Dark => "screen",
            BaseMode::Dim => "soft-light",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::appearance::fixture::tinted;
    use crate::appearance::BaseMode;

    #[test]
    fn no_tint_leaves_the_base_filters_untouched() {
        // A plain Light page must have NO filter at all — an identity filter
        // chain still forces a compositing layer and can shift colours through
        // rounding, so "no tint" has to mean literally none.
        assert_eq!(tinted(BaseMode::Light, 34, 0).canvas_filter(), "none");

        // Dark and Dim keep exactly the pipelines the old hand-written CSS had.
        let dark = tinted(BaseMode::Dark, 34, 0).canvas_filter();
        assert!(dark.starts_with("invert(0.92)"), "{dark}");
        assert!(!dark.contains("sepia"), "untinted dark must not colourise: {dark}");

        let dim = tinted(BaseMode::Dim, 34, 0).canvas_filter();
        assert_eq!(dim, "brightness(0.8) saturate(0.75) contrast(0.9)");
        assert!(!dim.contains("invert"), "Dim must preserve document colours");
    }

    #[test]
    fn the_tint_chain_is_appended_after_the_base_not_before() {
        // Order is load-bearing: on Dark the invert must run FIRST so the tint
        // lands on the visible (already inverted) paper.
        let f = tinted(BaseMode::Dark, 200, 60).canvas_filter();
        let inv = f.find("invert").expect("invert present");
        let sep = f.find("sepia").expect("sepia present");
        assert!(inv < sep, "tint must come after the inversion: {f}");
    }

    #[test]
    fn hue_is_absolute_measured_from_sepias_own_output() {
        // sepia() outputs ~34deg. Asking for 34 must therefore rotate by zero,
        // which is what makes `tint_hue` mean the same angle on every base.
        let f = tinted(BaseMode::Light, 34, 50).canvas_filter();
        assert!(f.contains("hue-rotate(0.0deg)"), "{f}");

        // And a request 90deg away rotates by exactly 90.
        let f = tinted(BaseMode::Light, 124, 50).canvas_filter();
        assert!(f.contains("hue-rotate(90.0deg)"), "{f}");
    }

    #[test]
    fn sepia_is_capped_so_photographs_do_not_become_duotone() {
        let f = tinted(BaseMode::Light, 34, 100).canvas_filter();
        // Full strength must still cap at 0.55, not 1.0.
        assert!(f.contains("sepia(0.550)"), "{f}");
    }

    #[test]
    fn strength_scales_the_tint_monotonically() {
        let weak = tinted(BaseMode::Light, 34, 20).canvas_filter();
        let strong = tinted(BaseMode::Light, 34, 80).canvas_filter();
        let grab = |s: &str| -> f64 {
            let i = s.find("sepia(").unwrap() + 6;
            s[i..].split(')').next().unwrap().parse().unwrap()
        };
        assert!(grab(&weak) < grab(&strong));
    }

    #[test]
    fn blend_families_match_the_base() {
        // multiply on a dark canvas would crush the inverted text away.
        assert_eq!(tinted(BaseMode::Light, 0, 0).canvas_blend(), "multiply");
        assert_eq!(tinted(BaseMode::Dark, 0, 0).canvas_blend(), "screen");
        assert_eq!(tinted(BaseMode::Dim, 0, 0).canvas_blend(), "soft-light");
    }

    #[test]
    fn dim_is_dark_for_the_ui_but_does_not_invert_the_page() {
        assert!(BaseMode::Dim.is_dark(), "Dim needs the dark UI palette");
        // Dim must keep the document's own colours — that is the reason to
        // pick it over Dark.
        assert!(!tinted(BaseMode::Dim, 0, 0).canvas_filter().contains("invert"));
        assert!(tinted(BaseMode::Dark, 0, 0).canvas_filter().contains("invert"));
    }
}
