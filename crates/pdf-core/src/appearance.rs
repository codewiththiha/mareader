//! Appearance model: base mode, colour tint, texture, noise — and the maths
//! that turns them into CSS values.
//!
//! The old six hand-written themes were the same structure with a different
//! hue, so the tint is now COMPUTED: three base modes (Light / Dark / Dim)
//! decide the structural family, and a single {hue, strength} tint is applied
//! by the same maths on top. Sepia / Green / Night survive as presets.
//!
//! CONTRACT: the field names below are the serde schema persisted inside
//! `pdfreader.settings.v1`. Do not rename them.

use serde::{Deserialize, Serialize};

/// Map an sRGB hue angle (`tint_hue`, applied via `hue-rotate()`) to the
/// corresponding OKLCH hue angle (what the UI tokens are emitted in). The two
/// circles are rotated relative to each other, so converting a fully saturated
/// sRGB colour at the requested angle recovers the right OKLCH hue and one
/// slider drives both consistently.
pub fn ui_hue_oklch(srgb_hue: f64) -> f64 {
    let h = srgb_hue.rem_euclid(360.0) / 60.0;
    let i = h.floor() as i32;
    let f = h - h.floor();
    // hsl(H 100% 50%) -> rgb, without a colour library.
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    let hex = format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8
    );
    crate::oklch::hex_to_oklch(&hex)
        .map(|(_, _, h)| h)
        .unwrap_or(srgb_hue)
}

/// The structural half of a look: what the canvas filter pipeline does, and
/// which direction the grain/texture blends go. A tint can be layered on any
/// of these; the tint never changes which family you are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BaseMode {
    /// Paper-white UI, canvas untouched, textures darken (multiply).
    #[default]
    Light,
    /// Inverted canvas, textures lighten (screen).
    Dark,
    /// NOT inverted — just dimmed. Keeps the document's real colours (figures,
    /// photos, syntax highlighting) instead of hue-rotating them, which is the
    /// reason to pick it over Dark. Grain uses soft-light so it neither
    /// crushes nor blows out the midtones.
    Dim,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureMode {
    #[default]
    None,
    Paper,
    Lined,
    Grid,
    Dotted,
    Cross,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoiseMode {
    #[default]
    Off,
    Static,
    Animated,
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
    /// A translucent colour wash would muddy the text, so the tint is a
    /// `sepia() saturate() hue-rotate()` chain: sepia collapses everything to
    /// a warm band (near-black keeps its luminance, paper takes the colour),
    /// saturate puts back the flattened chroma, and the rotation measures from
    /// sepia's own ~34° output so `tint_hue` is an absolute angle on every
    /// base. On Dark the invert runs first so the tint lands on the visible
    /// (already inverted) paper.
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
    /// The tint preserves each token's OWN lightness (which encodes the
    /// hierarchy: page brighter than chrome, chrome brighter than its borders)
    /// and moves only hue (rotated toward the tint hue by strength) and chroma
    /// (base + a strength-scaled amount, capped per token). Because L never
    /// moves, contrast ratios survive a 100% tint.
    pub fn ui_overrides(&self) -> Vec<(&'static str, String)> {
        if !self.has_tint() {
            return Vec::new();
        }
        let t = self.tint_strength as f64 / 100.0;
        // tint_hue is an sRGB angle; the tokens are emitted in OKLCH.
        let target_h = ui_hue_oklch(self.tint_hue as f64);

        // Per-token chroma ceiling at full strength. Large flat areas (paper,
        // surface) need restraint — the strength that looks right on a page
        // is overwhelming across a whole window — while accents are supposed
        // to be saturated.
        //
        // Ink is deliberately near-zero: text carries the reading contrast and
        // a coloured ink on coloured paper is what makes tinted themes feel
        // murky. It picks up a whisper of the hue and nothing more.
        let tokens: [(&'static str, &'static str, f64); 7] = [
            ("--color-paper", "--base-paper", 0.055),
            ("--color-surface", "--base-surface", 0.070),
            ("--color-line", "--base-line", 0.090),
            ("--color-ink", "--base-ink", 0.020),
            ("--color-muted", "--base-muted", 0.045),
            ("--color-accent", "--base-accent", 0.150),
            ("--color-accent-soft", "--base-accent-soft", 0.110),
        ];

        let palette = self.base_palette();
        let mut out = Vec::with_capacity(tokens.len());
        for (name, base_var, max_c) in tokens {
            let Some(hex) = palette.iter().find(|(k, _)| *k == base_var).map(|(_, v)| *v) else {
                continue;
            };
            let Some((l, c0, h0)) = crate::oklch::hex_to_oklch(hex) else {
                continue;
            };

            // Rotate the SHORT way around the circle, so a hue near 350 does
            // not sweep the entire spectrum on its way to 10.
            let mut delta = target_h - h0;
            while delta > 180.0 {
                delta -= 360.0;
            }
            while delta < -180.0 {
                delta += 360.0;
            }
            let h = (h0 + delta * t).rem_euclid(360.0);

            // Near-neutral bases (white paper, gray line) have a meaningless
            // hue, so blend from their own chroma up to the ceiling rather
            // than preserving a hue that was never really there.
            let c = c0 + (max_c - c0).max(0.0) * t;

            out.push((name, crate::oklch::oklch_css(l, c, h)));
        }
        out
    }

    /// The base palette for a mode, as `(token, value)` pairs.
    ///
    /// Mirrors the `:root[data-base=...]` blocks in input.css; only used by
    /// preset thumbnails, which must carry their own look rather than inherit
    /// the live tokens. Keep the two in sync.
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

        // 4. Texture and noise scale/opacity. No --ps-filter / --ps-blend
        //    are emitted — the .preset-canvas no longer uses CSS filter or
        //    mix-blend-mode (those caused GPU compositor bugs on Dark/Dim/
        //    Sepia/Green themes without Noise during slider drags). Instead,
        //    the swatch uses solid colours: --ps-color-paper for the page
        //    backdrop and --ps-color-ink for the "text" bars. This makes the
        //    swatch immune to compositor layer-loss bugs because it has no
        //    GPU compositing layers to lose.
        out.push_str(&format!(
            "--ps-tex-opacity:{:.3};",
            self.texture_opacity as f64 / 100.0
        ));
        // Thumbnails are ~1/12 of a page; at the true pitch a 26px rule grid
        // would be a solid block. Scale the pitch down with the swatch so the
        // PATTERN is recognisable, which is what the thumbnail is for.
        out.push_str(&format!(
            "--ps-tex-scale:{:.3};",
            (self.texture_scale as f64 / 100.0) * 0.34
        ));
        out.push_str(&format!(
            "--ps-noise-opacity:{:.3};",
            self.noise_intensity as f64 / 100.0
        ));
        // The swatch carries its own base palette, so it must carry the
        // matching grain blend too — inheriting the live one would render a
        // dark preset's grain with the light rule (and vice versa), i.e.
        // invisibly. Same family split as the textures.
        out.push_str(&format!(
            "--ps-noise-blend:{};",
            if self.base.is_dark() { "screen" } else { "multiply" }
        ));

        // 5. Texture stroke colours and blend direction in --ps-* namespace.
        //    These are keyed off `:root.dark` in input.css (inherited from
        //    <html>); without the .dark class on the swatch itself, they would
        //    inherit the LIVE theme's values. When the live theme and the
        //    preset's base disagree (e.g. reader on dark live theme, browsing
        //    a LIGHT preset swatch), the light preset's texture strokes would
        //    inherit the DARK values (white strokes, screen blend) — and on a
        //    light canvas those are near-invisible, making the "text" bars
        //    disappear under the texture overlay.
        if self.base.is_dark() {
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
    }

    /// (L, C, H) of a token in an override set.
    fn lch(o: &[(&'static str, String)], name: &str) -> (f64, f64, f64) {
        let v = &o.iter().find(|(k, _)| *k == name).unwrap().1;
        let inner = v.trim_start_matches("oklch(").trim_end_matches(')');
        let mut it = inner.split_whitespace().map(|x| x.parse::<f64>().unwrap());
        (it.next().unwrap(), it.next().unwrap(), it.next().unwrap())
    }

    #[test]
    fn the_tint_preserves_each_tokens_lightness_exactly() {
        // THE BUG THIS PREVENTS: mixing toward the tint colour dragged paper,
        // surface and line to a common lightness, so page/sidebar/toolbar/
        // thumbnails merged into one flat slab. Lightness must never move.
        for strength in [10u8, 50, 90, 100] {
            let o = tinted(BaseMode::Light, 104, strength).ui_overrides();
            for (token, base_hex) in [
                ("--color-paper", "#ffffff"),
                ("--color-surface", "#f3f4f6"),
                ("--color-line", "#e5e7eb"),
                ("--color-ink", "#1f2937"),
            ] {
                let want = crate::oklch::hex_to_oklch(base_hex).unwrap().0;
                let got = lch(&o, token).0;
                assert!(
                    (got - want).abs() < 0.001,
                    "{token} at {strength}%: L moved {want} -> {got}"
                );
            }
        }
    }

    #[test]
    fn the_lightness_ladder_survives_a_full_strength_tint() {
        // Page brighter than chrome, chrome brighter than its borders. If this
        // collapses the UI loses all its edges.
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let paper = lch(&o, "--color-paper").0;
        let surface = lch(&o, "--color-surface").0;
        let line = lch(&o, "--color-line").0;
        assert!(paper > surface + 0.01, "paper {paper} vs surface {surface}");
        assert!(surface > line + 0.01, "surface {surface} vs line {line}");

        // ...and inverted in dark mode.
        let d = tinted(BaseMode::Dark, 104, 100).ui_overrides();
        let dpaper = lch(&d, "--color-paper").0;
        let dsurface = lch(&d, "--color-surface").0;
        assert!(dpaper < dsurface, "dark paper must stay the darkest");
    }

    #[test]
    fn the_accent_actually_follows_the_tint_hue() {
        // REGRESSION: the accent used to stay blue on a green tint, because
        // mixing a saturated blue 31% toward green barely moves its hue. At
        // full strength every token must land ON the requested hue.
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let want = ui_hue_oklch(104.0);
        for token in ["--color-paper", "--color-accent", "--color-accent-soft", "--color-line"] {
            let h = lch(&o, token).2;
            let d = (h - want).abs().min(360.0 - (h - want).abs());
            assert!(d < 1.0, "{token} hue {h} should be ~{want}");
        }
    }

    #[test]
    fn hue_rotation_takes_the_short_way_round_the_circle() {
        // A base at ~265deg tinted to 10deg must rotate forward through 300,
        // not sweep backwards through the whole spectrum. At half strength the
        // result should sit between the two, going the short way.
        let o = tinted(BaseMode::Light, 10, 50).ui_overrides();
        let h = lch(&o, "--color-line").2; // base line hue ≈ 265
        let target = ui_hue_oklch(10.0);
        assert!(target < 90.0, "sanity: sRGB 10 maps low, got {target}");
        assert!(
            (280.0..=360.0).contains(&h),
            "expected a short forward rotation through 300, got {h}"
        );
    }

    #[test]
    fn ink_stays_almost_neutral_so_text_does_not_go_murky() {
        let o = tinted(BaseMode::Light, 104, 100).ui_overrides();
        let ink_c = lch(&o, "--color-ink").1;
        let paper_c = lch(&o, "--color-paper").1;
        let accent_c = lch(&o, "--color-accent").1;
        assert!(ink_c < 0.03, "ink chroma {ink_c} too colourful");
        assert!(accent_c > paper_c, "the accent must be the most saturated");
    }

    #[test]
    fn chroma_rises_with_strength(){
        let weak = tinted(BaseMode::Light, 104, 20).ui_overrides();
        let strong = tinted(BaseMode::Light, 104, 90).ui_overrides();
        assert!(lch(&weak, "--color-paper").1 < lch(&strong, "--color-paper").1);
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
    fn the_ui_hue_matches_the_hue_the_page_filter_produces() {
        // COLOUR-SPACE TRAP: `hue-rotate()` works in sRGB, so `tint_hue` is an
        // sRGB angle. The UI tokens are emitted in OKLCH, whose hue circle is
        // rotated relative to sRGB — feeding the raw number straight in made
        // the page go warm tan while the chrome went pink at hue 34.
        // `ui_hue_oklch` maps between them, so both land on the same colour.
        let o = tinted(BaseMode::Light, 34, 100).ui_overrides();
        let h = o
            .iter()
            .find(|(k, _)| *k == "--color-paper")
            .map(|(_, v)| {
                let inner = v.trim_start_matches("oklch(").trim_end_matches(')');
                inner.split_whitespace().nth(2).unwrap().parse::<f64>().unwrap()
            })
            .unwrap();
        // sRGB 34deg (a warm tan) sits near 60deg on the OKLCH circle, NOT 34.
        let want = crate::appearance::ui_hue_oklch(34.0);
        assert!((h - want).abs() < 1.0, "paper hue {h} should be {want}");
        assert!(h > 40.0, "a warm tan must not be emitted as OKLCH 34 (pink)");
    }

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

    #[test]
    fn dim_is_dark_for_the_ui_but_does_not_invert_the_page() {
        assert!(BaseMode::Dim.is_dark(), "Dim needs the dark UI palette");
        // Dim must keep the document's own colours — that is the reason to
        // pick it over Dark.
        assert!(!tinted(BaseMode::Dim, 0, 0).canvas_filter().contains("invert"));
        assert!(tinted(BaseMode::Dark, 0, 0).canvas_filter().contains("invert"));
    }
}
