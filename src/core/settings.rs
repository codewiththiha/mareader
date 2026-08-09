//! Persisted user settings + texture mode enum.
//!
//! CONTRACT: field names below are the serde schema persisted to localStorage
//! under `pdfreader.settings.v1`. Do not rename fields.

use serde::{Deserialize, Serialize};

use crate::core::themes::{theme_by_id, THEMES};

pub const SETTINGS_KEY: &str = "pdfreader.settings.v1";

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme_id: String,
    pub texture: TextureMode,
    pub noise_enabled: bool,
    /// 0..=100
    pub noise_intensity: u8,
    pub default_zoom: f64,
    pub last_path: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme_id: "light".to_string(),
            texture: TextureMode::None,
            noise_enabled: false,
            noise_intensity: 25,
            default_zoom: 1.0,
            last_path: None,
        }
    }
}

/// Ensures a persisted `Settings` is internally valid (bad theme id / out-of-range
/// values after deserialization or manual edits).
pub fn sanitize(settings: &mut Settings) {
    if settings.theme_id.is_empty() || !THEMES.iter().any(|t| t.id == settings.theme_id) {
        settings.theme_id = theme_by_id("light").id.to_string();
    }
    settings.noise_intensity = settings.noise_intensity.min(100);
    settings.default_zoom = settings.default_zoom.clamp(0.25, 5.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings {
            theme_id: "sepia".to_string(),
            texture: TextureMode::Paper,
            noise_enabled: true,
            noise_intensity: 60,
            default_zoom: 1.25,
            last_path: Some("/tmp/a.pdf".to_string()),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);

        // sanitize must not change a valid value set
        sanitize(&mut s);
        assert_eq!(s.theme_id, "sepia");
        assert_eq!(s.noise_intensity, 60);
    }

    #[test]
    fn invalid_theme_is_reset() {
        let mut s = Settings {
            theme_id: "neon".to_string(),
            ..Default::default()
        };
        sanitize(&mut s);
        assert_eq!(s.theme_id, "light");
    }

    #[test]
    fn missing_fields_default() {
        // A JSON blob with unknown/extra fields and no defaults must still parse
        // thanks to #[serde(default)].
        let json = r#"{"theme_id":"dark"}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.theme_id, "dark");
        assert_eq!(s.noise_enabled, false);
    }
}
