//! The appearance data model: base, texture and noise modes, and the
//! [`Appearance`] they compose.

use serde::{Deserialize, Serialize};

use super::base::base_tokens;

/// The structural half of a look: the filter family and blend
/// direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BaseMode {
    /// Paper-white UI, canvas untouched, textures darken (multiply).
    #[default]
    Light,
    /// Inverted canvas, textures lighten (screen).
    Dark,
    /// NOT inverted, just dimmed: keeps the document's real colours.
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

    pub const ALL: &'static [BaseMode] = &[Self::Light, Self::Dark, Self::Dim];

    pub fn all() -> &'static [BaseMode] {
        Self::ALL
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
    /// The CSS class a carrier takes for this mode.
    pub fn css_class(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Paper => Some("texture-paper"),
            Self::Lined => Some("texture-lined"),
            Self::Grid => Some("texture-grid"),
            Self::Dotted => Some("texture-dotted"),
            Self::Cross => Some("texture-cross"),
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

    pub const ALL: &'static [TextureMode] = &[
        Self::None,
        Self::Paper,
        Self::Lined,
        Self::Grid,
        Self::Dotted,
        Self::Cross,
    ];

    pub fn all() -> &'static [TextureMode] {
        Self::ALL
    }
}

/// Film grain: off, static, or animated, re-seeding per frame.
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

/// A complete look: what a preset stores and the DOM reflects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub base: BaseMode,
    /// Tint hue in degrees, 0..360.
    pub tint_hue: u16,
    /// Tint strength 0..=100; 0 short-circuits the pipeline.
    pub tint_strength: u8,
    pub texture: TextureMode,
    /// Texture opacity 0..=100.
    pub texture_opacity: u8,
    /// Texture scale as a PERCENTAGE of the natural pitch, 25..=400.
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

    /// The tint's effect amount, 0..=1, fed to both pipelines' curves.
    pub fn tint_amount(&self) -> f64 {
        (self.tint_strength as f64 / 50.0).min(1.0)
    }

    /// The exact hex (or oklch literal) the UI accent has now.
    pub fn accent_hex(&self) -> String {
        if let Some(value) = self.tinted_accent() {
            return value;
        }
        base_tokens(self.base).accent.to_string()
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn css_class_maps_every_mode_to_its_stylesheet_class() {
        // The off mode carries no class; each pattern mode names the
        // textures.css selector it activates.
        assert_eq!(TextureMode::None.css_class(), None);
        assert_eq!(TextureMode::Paper.css_class(), Some("texture-paper"));
        assert_eq!(TextureMode::Lined.css_class(), Some("texture-lined"));
        assert_eq!(TextureMode::Grid.css_class(), Some("texture-grid"));
        assert_eq!(TextureMode::Dotted.css_class(), Some("texture-dotted"));
        assert_eq!(TextureMode::Cross.css_class(), Some("texture-cross"));
    }
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

        let mut small = Appearance {
            texture_scale: 1,
            ..Default::default()
        };
        small.sanitize();
        assert_eq!(small.texture_scale, 25, "texture must stay legible");
    }

    #[test]
    fn accent_hex_fallback_and_tint() {
        let mut a = Appearance::default();
        assert_eq!(a.accent_hex(), "#2563eb");
        a.base = BaseMode::Dark;
        assert_eq!(a.accent_hex(), "#60a5fa");
        a.base = BaseMode::Dim;
        assert_eq!(a.accent_hex(), "#7a9bd4");

        // When tinted, overrides provide --color-accent
        a.tint_hue = 104;
        a.tint_strength = 50;
        assert!(a.accent_hex().starts_with("oklch("));
    }
}
