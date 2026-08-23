//! The appearance data model: the three structural modes (base / texture /
//! noise) and the [`Appearance`] look they compose into.
//!
//! The old six hand-written themes were the same structure with a different
//! hue, so the tint is now COMPUTED: three base modes (Light / Dark / Dim)
//! decide the structural family, and a single {hue, strength} tint is applied
//! by the same maths on top. Sepia / Green / Night survive as presets.
//!
//! CONTRACT: the field names below are the serde schema persisted inside
//! `pdfreader.settings.v1`. Do not rename them.

use serde::{Deserialize, Serialize};

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

}
#[cfg(test)]
mod tests {
    use super::*;

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
}
