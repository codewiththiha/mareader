//! The dictionary's knobs: the hover, the languages, the overlay.

use serde::{Deserialize, Serialize};

use super::on_true;

/// The floating search window's remembered shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictOverlay {
    pub visible: bool,
    pub collapsed: bool,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Default for DictOverlay {
    fn default() -> Self {
        Self {
            visible: false,
            collapsed: true,
            x: 32.0,
            y: 32.0,
            w: 380.0,
            h: 460.0,
        }
    }
}

/// The dictionary system's persisted state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictSettings {
    /// Sense pops on red-word hover.
    #[serde(default = "on_true")]
    pub hover: bool,
    /// Pack ids in play; empty means every built pack.
    #[serde(default)]
    pub langs: Vec<String>,
    /// The floating window's last shape.
    #[serde(default)]
    pub overlay: DictOverlay,
}

impl Default for DictSettings {
    fn default() -> Self {
        Self {
            hover: true,
            langs: Vec::new(),
            overlay: DictOverlay::default(),
        }
    }
}
