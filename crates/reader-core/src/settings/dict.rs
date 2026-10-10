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
    /// The card's language: a pack's target. `None` lets the first
    /// built pack answer.
    pub default_lang: Option<String>,
    /// Pack ids the sidebar's own search asks; empty follows
    /// `default_lang`.
    #[serde(default)]
    pub langs: Vec<String>,
}

impl Default for DictSettings {
    fn default() -> Self {
        Self {
            hover: true,
            default_lang: None,
            langs: Vec::new(),
        }
    }
}
