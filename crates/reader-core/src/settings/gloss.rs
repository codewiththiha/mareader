//! The AI word card's user-facing settings types.
//!
//! These ride on the persisted `Settings` struct (in `reader_core::settings`)
//! as FLAT `gloss_*` fields — the field names are the serde schema saved to
//! localStorage, so they stay flat and are not nested behind a new `ai`
//! block (that would silently drop every install's saved values on load).
//! This crate owns the types; the persisted struct owns the storage.

use serde::{Deserialize, Serialize};

/// The highlighter's default fill opacity for a new install (or a blob that
/// predates the knob).
pub fn default_gloss_opacity() -> f64 {
    0.4
}

/// The default custom highlighter colour — the hex the old fixed `violet`
/// swatch had, so installs predating the colour picker land on the look they
/// had.
pub fn default_custom_gloss() -> String {
    "#a58af0".into()
}

/// `#` + six ASCII hex digits — the shape the custom colour picker emits.
pub fn is_hex6(s: &str) -> bool {
    let b = s.as_bytes();
    s.len() == 7 && b[0] == b'#' && b[1..].iter().all(|c| c.is_ascii_hexdigit())
}

/// The highlighter colour of gloss marks: a fixed palette or a custom hex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlossColor {
    #[default]
    Accent,
    Red,
    Yellow,
    Green,
    Blue,
    /// Old saved `"violet"` becomes Custom (default hex = old violet).
    #[serde(alias = "violet")]
    Custom,
}

impl GlossColor {
    pub const ALL: &'static [GlossColor] = &[
        Self::Accent,
        Self::Red,
        Self::Yellow,
        Self::Green,
        Self::Blue,
        Self::Custom,
    ];

    pub fn all() -> &'static [GlossColor] {
        Self::ALL
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Accent => "Auto",
            Self::Red => "Red",
            Self::Yellow => "Yellow",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Custom => "Custom",
        }
    }

    /// `None` = follow the live accent tint.
    pub fn resolve(&self, custom: &str) -> Option<String> {
        match self {
            Self::Accent => None,
            Self::Red => Some("#e56b64".into()),
            Self::Yellow => Some("#e8c449".into()),
            Self::Green => Some("#6fd58c".into()),
            Self::Blue => Some("#6ba3f5".into()),
            Self::Custom => Some(custom.to_string()),
        }
    }
}

/// How much air the AI word card carries: the padding, line heights and
/// section gaps of the gloss card's body. Compact is the default because a
/// definition is scanned, not read like a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GlossDensity {
    #[default]
    Compact,
    Comfortable,
}

impl GlossDensity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Comfortable => "Comfortable",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GlossDensity;

    #[test]
    fn gloss_density_uses_the_persisted_snake_case_schema() {
        assert_eq!(
            serde_json::to_string(&GlossDensity::Compact).unwrap(),
            "\"compact\""
        );
        assert_eq!(
            serde_json::from_str::<GlossDensity>("\"comfortable\"").unwrap(),
            GlossDensity::Comfortable
        );
    }
}
