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
    /// The panel's two languages. Off, the ask is English to any.
    #[serde(default)]
    pub pair: bool,
    /// The typed language; `None` reads it off the word itself.
    #[serde(default)]
    pub from: Option<String>,
    /// The answering language; `None` follows `default_lang`.
    #[serde(default)]
    pub to: Option<String>,
}

impl Default for DictSettings {
    fn default() -> Self {
        Self {
            hover: true,
            default_lang: None,
            langs: Vec::new(),
            pair: false,
            from: None,
            to: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_saved_before_the_pair_still_reads() {
        let old = r#"{"hover":false,"default_lang":"my","langs":["mcfnlp-en-my"]}"#;
        let got: DictSettings = serde_json::from_str(old).expect("an old row");
        assert!(!got.hover);
        assert_eq!(got.default_lang.as_deref(), Some("my"));
        assert_eq!(got.langs, vec!["mcfnlp-en-my".to_string()]);
        // The pair is off: the panel asks English to any, as it did.
        assert!(!got.pair);
        assert_eq!(got.from, None);
        assert_eq!(got.to, None);
    }

    #[test]
    fn the_pair_round_trips_with_the_rest() {
        let row = DictSettings {
            pair: true,
            from: None,
            to: Some("jp".to_string()),
            ..DictSettings::default()
        };
        let text = serde_json::to_string(&row).expect("a row");
        let got: DictSettings = serde_json::from_str(&text).expect("the row back");
        assert_eq!(got, row);
    }
}
