//! The dictionary's knobs: the hover and the languages.

use serde::{Deserialize, Serialize};

use super::on_true;

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
}

impl Default for DictSettings {
    fn default() -> Self {
        Self {
            hover: true,
            langs: Vec::new(),
        }
    }
}
