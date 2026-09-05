//! The preset-thumbnail preview: an inline `style` + class list that render
//! a swatch in ITS OWN look rather than the one currently applied.
//!
//! Every variable the swatch consumes is emitted under a private `--ps-*`
//! namespace (WKWebView's custom-property invalidation is name-based, not
//! scope-based); the `.preset-canvas` itself uses solid colours, no CSS
//! filter/blend, so it has zero GPU compositing layers to lose during a
//! slider drag.

use crate::appearance::{Appearance, NoiseMode};

impl Appearance {
    /// Inline `style` for a preset thumbnail, so the swatch renders in its own
    /// look rather than the one currently applied.
    ///
    /// **PRIVATE NAMESPACE (`--ps-*`)**. Every variable the swatch consumes is
    /// emitted under a `--ps-*` name that is NEVER written on `<html>`. This is
    /// the fix for the "preset text bars vanish during a tint drag" bug:
    /// WKWebView's custom-property invalidation is NAME-BASED, not scope-based.
    /// `paint_appearance_now()` rewrites `--canvas-filter`, `--canvas-blend`,
    /// the seven `--color-*` tokens, `--texture-*`, `--noise-*` on `<html>`
    /// once per frame during a slider drag — and every declaration in the
    /// document that consumes those NAMES gets invalidated, including the
    /// ones inside swatches that shadow the names inline. The swatch is
    /// repainted against a mid-rebuild backdrop (the live page canvases are
    /// being swapped raw↔baked, `--color-paper` under the popover is moving),
    /// and the `.preset-canvas`'s `filter` + `mix-blend-mode` layer samples a
    /// wrong backdrop: the dark `#24303f` "text" bars get multiplied toward
    /// the *live* paper colour and collapse into it (light preset) or screen
    /// into the backdrop (dark preset). The three bars vanish for the whole
    /// drag, only reappearing on hover (which forces a correct repaint).
    ///
    /// By renaming every consumed variable to `--ps-*`, the per-frame root
    /// writes invalidate NOTHING inside the swatch — the swatch is simply never
    /// repainted during the drag, so its blend can never sample a wrong
    /// backdrop. The `contain: layout paint` on `.preset-swatch` (see
    /// input.css) is a second isolation layer: even if a descendant did
    /// somehow depend on a root name, the repaint would be caged to the
    /// swatch's own subtree and couldn't sample the popover's moving backdrop.
    pub fn preview_style(&self) -> String {
        let mut out = String::new();

        // 1. Base palette in --ps-* namespace (never written on <html>).
        //    --base-paper -> --ps-paper, etc.
        for (k, v) in self.base_palette() {
            let ps_name = format!("--ps-{}", k.trim_start_matches("--base-"));
            out.push_str(&format!("{ps_name}:{v};"));
        }

        // 2. Aliases: --ps-color-* defaults to --ps-* (the base palette).
        //    If a tint is active, step 3 overrides these with the tinted
        //    value; otherwise the alias resolves to the base.
        for token in ["paper", "ink", "muted", "surface", "line", "accent", "accent-soft"] {
            out.push_str(&format!("--ps-color-{token}:var(--ps-{token});"));
        }

        // 3. Tinted UI token overrides (if any) in --ps-color-* namespace.
        //    ui_overrides() emits --color-* names; rename to --ps-color-*.
        for (k, v) in self.ui_overrides() {
            let ps_name = format!("--ps-color-{}", k.trim_start_matches("--color-"));
            out.push_str(&format!("{ps_name}:{v};"));
        }

        // 4. Texture and noise scale/opacity, the grain blend and the
        //    stroke family — the shared swatch tail, driven by the BASE's
        //    darkness here (the PDF preview's paper IS the base mode's).
        out.push_str(&ps_surface_tail(self, self.base.is_dark()));
        out
    }

    /// Class list for a preset thumbnail's page element.
    pub fn preview_class(&self) -> String {
        let mut c = String::from("preset-page");
        if let Some(class) = self.texture.css_class() {
            c.push(' ');
            c.push_str(class);
        }
        match self.noise {
            NoiseMode::Off => {}
            NoiseMode::Static => c.push_str(" preset-noise"),
            NoiseMode::Animated => c.push_str(" preset-noise preset-noise-animated"),
        }
        c
    }
}

/// The swatch tail every preview shares: texture and noise scale/opacity,
/// the grain blend and the texture stroke family, all in the `--ps-*`
/// namespace.
///
/// No --ps-filter / --ps-blend are emitted — the .preset-canvas does not
/// use CSS filter or mix-blend-mode (those caused GPU compositor bugs on
/// Dark/Dim themes without Noise during slider drags). Instead the swatch
/// uses solid colours: --ps-color-paper for the page backdrop and
/// --ps-color-ink for the "text" bars, so it has no GPU compositing
/// layers to lose.
///
/// `dark_paper` picks the stroke family. The PDF preview keys it off the
/// base mode (its paper IS the base mode's); the text preview keys it off
/// the palette's own paper lightness, because a dim TEXT page sits on
/// light-grey paper while its chrome is dark. The swatch carries its own
/// look, so it must carry the matching grain blend and strokes too —
/// inheriting the live theme's would render a dark preset's grain with
/// the light rule (and vice versa), i.e. invisibly.
pub(crate) fn ps_surface_tail(a: &Appearance, dark_paper: bool) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "--ps-tex-opacity:{:.3};",
        a.texture_opacity as f64 / 100.0
    ));
    // Thumbnails are ~1/12 of a page; at the true pitch a 26px rule grid
    // would be a solid block. Scale the pitch down with the swatch so the
    // PATTERN is recognisable, which is what the thumbnail is for.
    out.push_str(&format!(
        "--ps-tex-scale:{:.3};",
        (a.texture_scale as f64 / 100.0) * 0.34
    ));
    out.push_str(&format!(
        "--ps-noise-opacity:{:.3};",
        a.noise_intensity as f64 / 100.0
    ));
    out.push_str(&format!(
        "--ps-noise-blend:{};",
        if dark_paper { "screen" } else { "multiply" }
    ));
    if dark_paper {
        out.push_str("--ps-texture-line:rgba(255,255,255,0.22);");
        out.push_str("--ps-texture-strong:rgba(255,255,255,0.36);");
        out.push_str("--ps-texture-paper:rgba(255,255,255,0.08);");
        out.push_str("--ps-texture-blend:screen;");
    } else {
        out.push_str("--ps-texture-line:rgba(15,23,42,0.16);");
        out.push_str("--ps-texture-strong:rgba(15,23,42,0.26);");
        out.push_str("--ps-texture-paper:rgba(15,23,42,0.05);");
        out.push_str("--ps-texture-blend:multiply;");
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::appearance::fixture::tinted;
    use crate::appearance::{Appearance, BaseMode, NoiseMode, TextureMode};

    #[test]
    fn a_preview_carries_its_own_look_not_the_live_one() {
        // The whole point of a thumbnail: it must render as ITS appearance
        // while a different one is applied to the document.
        let p = tinted(BaseMode::Dark, 110, 40).preview_style();
        assert!(p.contains("--ps-paper:#131316"), "{p}");
        assert!(p.contains("--ps-color-paper:oklch("), "tint must reach the swatch");

        // An untinted preview still pins the palette, or it would inherit the
        // live (possibly tinted) tokens and show the wrong colour.
        let plain = tinted(BaseMode::Light, 34, 0).preview_style();
        assert!(plain.contains("--ps-color-paper:var(--ps-paper)"), "{plain}");

        // The swatch must consume ONLY --ps-* names — no root-mutated names
        // (--canvas-filter, --color-*, --texture-*, --noise-*) — or WKWebView's
        // name-based custom-property invalidation would repaint the swatch
        // every frame during a slider drag, against a mid-rebuild backdrop,
        // making the "text" bars vanish. Also, --ps-filter / --ps-blend are
        // no longer emitted (the .preset-canvas uses solid colours, not CSS
        // filter/blend) so the swatch has zero GPU compositing layers.
        for root_mutated in [
            "--canvas-filter:", "--canvas-blend:",
            "--color-paper:", "--color-ink:", "--color-line:",
            "--texture-opacity:", "--texture-scale-user:",
            "--texture-line:", "--texture-paper:", "--texture-blend:",
            "--noise-opacity:", "--noise-blend:",
        ] {
            assert!(
                !p.contains(root_mutated),
                "preview_style() must not emit root-mutated name {root_mutated} (would trigger name-based invalidation during drag): {p}"
            );
            assert!(
                !plain.contains(root_mutated),
                "preview_style() must not emit root-mutated name {root_mutated} (would trigger name-based invalidation during drag): {plain}"
            );
        }
    }

    #[test]
    fn preview_shrinks_the_texture_pitch_so_the_pattern_is_visible() {
        // At true pitch a 26px rule grid on a ~40px swatch is a solid block.
        let a = Appearance { texture: TextureMode::Lined, texture_scale: 100, ..Default::default() };
        let s = a.preview_style();
        let i = s.find("--ps-tex-scale:").unwrap() + "--ps-tex-scale:".len();
        let v: f64 = s[i..].split(';').next().unwrap().parse().unwrap();
        assert!(v < 0.5, "preview pitch must be reduced, got {v}");
    }

    #[test]
    fn preview_class_reflects_texture_and_grain() {
        let a = Appearance {
            texture: TextureMode::Grid,
            noise: NoiseMode::Animated,
            ..Default::default()
        };
        let c = a.preview_class();
        assert!(c.contains("texture-grid"), "{c}");
        assert!(c.contains("preset-noise-animated"), "{c}");

        let plain = Appearance::default().preview_class();
        assert!(!plain.contains("texture-"), "{plain}");
        assert!(!plain.contains("noise"), "{plain}");
    }
}
