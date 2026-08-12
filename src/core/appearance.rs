//! Appearance model: base mode, colour tint, texture, noise — and the maths
//! that turns them into CSS values.
//!
//! WHY THIS EXISTS. The old design hard-coded six themes as six CSS blocks,
//! each with its own hand-tuned `--canvas-filter`. Sepia and Green were just
//! "light, tinted brown" and "light, tinted green": the same structure with a
//! different hue. That does not generalise — a reader who wants a slightly
//! cooler paper has no way to ask for it, and every new tint means another
//! hand-written CSS block.
//!
//! So the tint is now COMPUTED. There are three base modes (Light / Dark /
//! Dim) that decide the structural family — is the canvas inverted, does the
//! grain screen or multiply — and on top of that a single {hue, strength}
//! tint that is applied by the same maths regardless of base. Sepia and Green
//! survive as PRESETS: `{base: Light, hue: 34°, strength: 45}` and
//! `{base: Light, hue: 96°, strength: 40}` reproduce them, and Night is
//! `{base: Dark, hue: 110°, strength: 35}`.
//!
//! CONTRACT: the six values below are the serde schema persisted inside
//! `pdfreader.settings.v1`. Do not rename them.

use serde::{Deserialize, Serialize};

/// The structural half of a look: what the canvas filter pipeline does, and
/// which direction the grain/texture blends go. A tint can be layered on any
/// of these; the tint never changes which family you are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BaseMode {
    /// Paper-white UI, canvas untouched, textures darken (multiply).
    Light,
    /// Inverted canvas, textures lighten (screen).
    Dark,
    /// NOT inverted — just dimmed. Keeps the document's real colours (figures,
    /// photos, syntax highlighting) instead of hue-rotating them, which is the
    /// reason to pick it over Dark. Grain uses soft-light so it neither
    /// crushes nor blows out the midtones.
    Dim,
}

impl Default for BaseMode {
    fn default() -> Self {
        Self::Light
    }
}

impl BaseMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Dim => "dim",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::Dim => "Dim",
        }
    }

    /// Drives `<html class="dark">`, which Tailwind's `dark:` variants and the
    /// texture blend direction both key off.
    pub fn is_dark(&self) -> bool {
        matches!(self, Self::Dark | Self::Dim)
    }


    pub fn all() -> [BaseMode; 3] {
        [Self::Light, Self::Dark, Self::Dim]
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureMode {
    None,
    Paper,
    Lined,
    Grid,
    Dotted,
    Cross,
}

impl Default for TextureMode {
    fn default() -> Self {
        Self::None
    }
}

impl TextureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Paper => "paper",
            Self::Lined => "lined",
            Self::Grid => "grid",
            Self::Dotted => "dotted",
            Self::Cross => "cross",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Paper => "Real paper",
            Self::Lined => "Lined",
            Self::Grid => "Grid",
            Self::Dotted => "Dotted",
            Self::Cross => "Cross",
        }
    }

    pub fn all() -> [TextureMode; 6] {
        [
            Self::None,
            Self::Paper,
            Self::Lined,
            Self::Grid,
            Self::Dotted,
            Self::Cross,
        ]
    }
}

/// Film grain: off, static, or animated.
///
/// Animated grain re-seeds the pattern every frame so it crawls like real film
/// or sensor noise instead of sitting there as a fixed dirt layer. It is a
/// separate MODE rather than a second boolean because "animated but disabled"
/// is not a state worth persisting, and a 3-way enum makes that unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseMode {
    Off,
    Static,
    Animated,
}

impl Default for NoiseMode {
    fn default() -> Self {
        Self::Off
    }
}

impl NoiseMode {

    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Static => "Static",
            Self::Animated => "Animated",
        }
    }

    pub fn all() -> [NoiseMode; 3] {
        [Self::Off, Self::Static, Self::Animated]
    }

    pub fn is_on(&self) -> bool {
        !matches!(self, Self::Off)
    }
}

/// A complete look. This is what a preset stores and what the DOM reflects.
///
/// Every field is independent on purpose: the tint does not silently change
/// the texture, the texture opacity does not touch the grain. The only
/// coupling is `base`, which selects the blend FAMILY for texture and grain —
/// and that coupling is required, because "multiply a light tint" is a no-op
/// and would render the grain invisible on dark paper.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub base: BaseMode,
    /// Tint hue in degrees, 0..360.
    pub tint_hue: u16,
    /// Tint strength 0..=100. 0 means "no tint at all" and short-circuits the
    /// whole colour pipeline, so a plain Light/Dark/Dim stays byte-identical to
    /// what it was before this feature existed.
    pub tint_strength: u8,
    pub texture: TextureMode,
    /// Texture opacity 0..=100.
    pub texture_opacity: u8,
    /// Texture scale as a PERCENTAGE of the natural pitch, 25..=400. Stored as
    /// an integer percent rather than a float so presets compare exactly.
    pub texture_scale: u16,
    pub noise: NoiseMode,
    /// Grain intensity 0..=100.
    pub noise_intensity: u8,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            base: BaseMode::Light,
            tint_hue: 34,
            tint_strength: 0,
            texture: TextureMode::None,
            texture_opacity: 90,
            texture_scale: 100,
            noise: NoiseMode::Off,
            noise_intensity: 25,
        }
    }
}

impl Appearance {
    pub fn sanitize(&mut self) {
        self.tint_hue %= 360;
        self.tint_strength = self.tint_strength.min(100);
        self.texture_opacity = self.texture_opacity.min(100);
        self.texture_scale = self.texture_scale.clamp(25, 400);
        self.noise_intensity = self.noise_intensity.min(100);
    }

    /// True when the tint should actually be applied.
    pub fn has_tint(&self) -> bool {
        self.tint_strength > 0
    }

    /// The CSS `filter` pipeline for the page canvas.
    ///
    /// THE PROBLEM this solves: you cannot just paint a translucent coloured
    /// rectangle over the page. That washes out the black text along with the
    /// paper, which is exactly the "muddy, low-contrast" look that makes tinted
    /// reading modes unpleasant. The old Sepia/Green blocks avoided it with a
    /// `sepia() saturate() hue-rotate()` chain, which works because `sepia()`
    /// collapses everything to a single warm hue band FIRST — near-black stays
    /// near-black (it has almost no luminance to tint), while the light paper
    /// takes the colour. Rotating that band then lands the tint on any hue.
    ///
    /// So this generalises the chain the two hand-written themes already used,
    /// rather than inventing a new mechanism:
    ///   sepia(t)              — collapse to a warm band, t scaled by strength
    ///   saturate(1 + t*k)     — put back the chroma sepia() flattens
    ///   hue-rotate(h - 34°)   — 34° is sepia()'s own output hue, so the
    ///                           rotation is measured FROM it and `tint_hue`
    ///                           means the same angle on every base
    ///
    /// On Dark the invert+180° rotation runs FIRST (that is what makes the page
    /// dark), and the tint chain is appended after it, so the tint is applied
    /// to the already-inverted image and lands on the visible paper colour
    /// rather than on the pre-inversion white.
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

    /// The seven UI colour tokens, tinted to match the page.
    ///
    /// The UI has to follow the page, or a tinted document sits in a stark
    /// white frame and the tint reads as a bug. Each token is mixed toward the
    /// tint hue in OKLCH, which keeps perceived LIGHTNESS constant while only
    /// chroma and hue move — so contrast ratios survive the tint and text does
    /// not become unreadable at high strength. `color-mix` does this in the
    /// browser, so there is no colour-space maths to maintain in Rust.
    ///
    /// UI mixing is deliberately gentler than the canvas tint (the `* 0.5`
    /// below, and less again for ink): chrome is a large flat area, and the
    /// strength that looks right on paper is overwhelming across a whole
    /// window.
    pub fn ui_overrides(&self) -> Vec<(&'static str, String)> {
        if !self.has_tint() {
            return Vec::new();
        }
        let t = self.tint_strength as f64 / 100.0;
        let hue = self.tint_hue;
        // A vivid anchor at the requested hue, specified in sRGB/HSL.
        //
        // COLOUR-SPACE TRAP: the anchor must live in the SAME hue space as
        // `hue-rotate()`, which is sRGB. An earlier version used
        // `oklch(0.72 0.13 H)` here, and the two disagreed badly — OKLCH hue 34
        // is pink while sRGB 34 is the warm tan sepia actually wants, so the
        // page went warm-brown while the UI went pink at the same setting.
        // Only the HUE comes from this anchor; the MIX is still done in oklch
        // (below), which is what keeps perceived lightness — and therefore text
        // contrast — stable as the tint strengthens.
        let tint = format!("hsl({hue} 60% 55%)");

        let pct = |f: f64| -> String { format!("{:.1}%", (t * f * 100.0).clamp(0.0, 100.0)) };

        vec![
            ("--color-paper", format!("color-mix(in oklch, var(--base-paper), {tint} {})", pct(0.50))),
            ("--color-surface", format!("color-mix(in oklch, var(--base-surface), {tint} {})", pct(0.50))),
            ("--color-line", format!("color-mix(in oklch, var(--base-line), {tint} {})", pct(0.55))),
            // Ink barely moves: it carries the text contrast.
            ("--color-ink", format!("color-mix(in oklch, var(--base-ink), {tint} {})", pct(0.18))),
            ("--color-muted", format!("color-mix(in oklch, var(--base-muted), {tint} {})", pct(0.30))),
            ("--color-accent", format!("color-mix(in oklch, var(--base-accent), {tint} {})", pct(0.35))),
            ("--color-accent-soft", format!("color-mix(in oklch, var(--base-accent-soft), {tint} {})", pct(0.45))),
        ]
    }

    /// The base palette for a mode, as `(token, value)` pairs.
    ///
    /// Duplicated from the `:root[data-base=...]` blocks in input.css on
    /// purpose, and ONLY for preset thumbnails: a swatch has to show a look
    /// that is not the currently applied one, so it cannot inherit the live
    /// tokens from `<html>` — it has to carry its own. Keep the two in sync;
    /// `base_palettes_match_the_stylesheet` guards the shape, and a thumbnail
    /// being slightly off is cosmetic rather than load-bearing.
    fn base_palette(&self) -> [(&'static str, &'static str); 7] {
        match self.base {
            BaseMode::Light => [
                ("--base-paper", "#ffffff"),
                ("--base-ink", "#1f2937"),
                ("--base-muted", "#6b7280"),
                ("--base-surface", "#f3f4f6"),
                ("--base-line", "#e5e7eb"),
                ("--base-accent", "#2563eb"),
                ("--base-accent-soft", "#dbeafe"),
            ],
            BaseMode::Dark => [
                ("--base-paper", "#131316"),
                ("--base-ink", "#e5e7eb"),
                ("--base-muted", "#9ca3af"),
                ("--base-surface", "#1a1a1e"),
                ("--base-line", "#2b2b31"),
                ("--base-accent", "#60a5fa"),
                ("--base-accent-soft", "#1d2b3a"),
            ],
            BaseMode::Dim => [
                ("--base-paper", "#1a1c1f"),
                ("--base-ink", "#c3c6cb"),
                ("--base-muted", "#8b8f96"),
                ("--base-surface", "#202328"),
                ("--base-line", "#2e3238"),
                ("--base-accent", "#7a9bd4"),
                ("--base-accent-soft", "#232b36"),
            ],
        }
    }

    /// Inline `style` for a preset thumbnail, so the swatch renders in its own
    /// look rather than the one currently applied.
    ///
    /// Custom properties inherit, so setting the base palette + the tint
    /// overrides on the swatch root makes everything inside it resolve against
    /// THAT appearance. The same `--canvas-filter` / `--texture-*` variables
    /// the real page uses are set too, which is what makes the thumbnail an
    /// actual preview instead of an approximation drawn by separate code.
    pub fn preview_style(&self) -> String {
        let mut out = String::new();
        for (k, v) in self.base_palette() {
            out.push_str(&format!("{k}:{v};"));
        }
        // Aliases first, then let any tint override them.
        for (alias, base) in [
            ("--color-paper", "--base-paper"),
            ("--color-ink", "--base-ink"),
            ("--color-muted", "--base-muted"),
            ("--color-surface", "--base-surface"),
            ("--color-line", "--base-line"),
            ("--color-accent", "--base-accent"),
            ("--color-accent-soft", "--base-accent-soft"),
        ] {
            out.push_str(&format!("{alias}:var({base});"));
        }
        for (k, v) in self.ui_overrides() {
            out.push_str(&format!("{k}:{v};"));
        }
        out.push_str(&format!("--canvas-filter:{};", self.canvas_filter()));
        out.push_str(&format!("--canvas-blend:{};", self.canvas_blend()));
        out.push_str(&format!(
            "--texture-opacity:{:.3};",
            self.texture_opacity as f64 / 100.0
        ));
        // Thumbnails are ~1/12 of a page; at the true pitch a 26px rule grid
        // would be a solid block. Scale the pitch down with the swatch so the
        // PATTERN is recognisable, which is what the thumbnail is for.
        out.push_str(&format!(
            "--texture-scale-user:{:.3};",
            (self.texture_scale as f64 / 100.0) * 0.34
        ));
        out.push_str(&format!(
            "--noise-opacity:{:.3};",
            self.noise_intensity as f64 / 100.0
        ));
        // The swatch carries its own base palette, so it must carry the
        // matching grain blend too — inheriting the live one would render a
        // dark preset's grain with the light rule (and vice versa), i.e.
        // invisibly. Same family split as the textures.
        out.push_str(&format!(
            "--noise-blend:{};",
            if self.base.is_dark() { "screen" } else { "multiply" }
        ));
        out
    }

    /// Class list for a preset thumbnail's page element.
    pub fn preview_class(&self) -> String {
        let mut c = String::from("preset-page");
        if self.texture != TextureMode::None {
            c.push_str(&format!(" texture-{}", self.texture.as_str()));
        }
        match self.noise {
            NoiseMode::Off => {}
            NoiseMode::Static => c.push_str(" preset-noise"),
            NoiseMode::Animated => c.push_str(" preset-noise preset-noise-animated"),
        }
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tinted(base: BaseMode, hue: u16, strength: u8) -> Appearance {
        Appearance { base, tint_hue: hue, tint_strength: strength, ..Default::default() }
    }

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
    fn ui_tokens_are_only_overridden_when_a_tint_is_active() {
        assert!(tinted(BaseMode::Light, 34, 0).ui_overrides().is_empty());
        let o = tinted(BaseMode::Light, 34, 50).ui_overrides();
        assert_eq!(o.len(), 7, "all seven tokens must move together");
        // Ink must move LESS than paper or the text loses contrast.
        let pctof = |name: &str| -> f64 {
            let v = &o.iter().find(|(k, _)| *k == name).unwrap().1;
            let i = v.rfind(' ').unwrap();
            v[i + 1..].trim_end_matches("%)").parse().unwrap()
        };
        assert!(pctof("--color-ink") < pctof("--color-paper"));
    }

    #[test]
    fn sanitize_clamps_every_range() {
        let mut a = Appearance {
            tint_hue: 725,
            tint_strength: 200,
            texture_opacity: 240,
            texture_scale: 5000,
            noise_intensity: 199,
            ..Default::default()
        };
        a.sanitize();
        assert_eq!(a.tint_hue, 5); // 725 % 360
        assert_eq!(a.tint_strength, 100);
        assert_eq!(a.texture_opacity, 100);
        assert_eq!(a.texture_scale, 400);
        assert_eq!(a.noise_intensity, 100);

        let mut small = Appearance { texture_scale: 1, ..Default::default() };
        small.sanitize();
        assert_eq!(small.texture_scale, 25, "texture must stay legible");
    }

    #[test]
    fn the_ui_tint_anchor_lives_in_the_same_hue_space_as_hue_rotate() {
        // REGRESSION: the anchor was oklch(0.72 0.13 H) while the canvas tint
        // uses hue-rotate(), which is sRGB. The same `tint_hue` then meant two
        // different colours — at 34 the page went warm tan and the UI went
        // pink. Both must be driven from an sRGB hue.
        let o = tinted(BaseMode::Light, 34, 50).ui_overrides();
        let paper = &o.iter().find(|(k, _)| *k == "--color-paper").unwrap().1;
        assert!(paper.contains("hsl(34"), "UI anchor must be sRGB-hued: {paper}");
        assert!(!paper.contains("oklch(0.72"), "stale oklch anchor: {paper}");
        // The MIX still happens in oklch: that is what holds perceived
        // lightness (and text contrast) steady as the tint strengthens.
        assert!(paper.contains("in oklch"), "mix must stay perceptual: {paper}");
    }

    #[test]
    fn a_preview_carries_its_own_look_not_the_live_one() {
        // The whole point of a thumbnail: it must render as ITS appearance
        // while a different one is applied to the document.
        let p = tinted(BaseMode::Dark, 110, 40).preview_style();
        assert!(p.contains("--base-paper:#131316"), "{p}");
        assert!(p.contains("--canvas-filter:invert"), "{p}");
        assert!(p.contains("--color-paper:color-mix"), "tint must reach the swatch");

        // An untinted preview still pins the palette, or it would inherit the
        // live (possibly tinted) tokens and show the wrong colour.
        let plain = tinted(BaseMode::Light, 34, 0).preview_style();
        assert!(plain.contains("--color-paper:var(--base-paper)"), "{plain}");
        assert!(plain.contains("--canvas-filter:none"), "{plain}");
    }

    #[test]
    fn preview_shrinks_the_texture_pitch_so_the_pattern_is_visible() {
        // At true pitch a 26px rule grid on a ~40px swatch is a solid block.
        let a = Appearance { texture: TextureMode::Lined, texture_scale: 100, ..Default::default() };
        let s = a.preview_style();
        let i = s.find("--texture-scale-user:").unwrap() + 21;
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

    #[test]
    fn dim_is_dark_for_the_ui_but_does_not_invert_the_page() {
        assert!(BaseMode::Dim.is_dark(), "Dim needs the dark UI palette");
        // Dim must keep the document's own colours — that is the reason to
        // pick it over Dark.
        assert!(!tinted(BaseMode::Dim, 0, 0).canvas_filter().contains("invert"));
        assert!(tinted(BaseMode::Dark, 0, 0).canvas_filter().contains("invert"));
    }
}
