//! The dictionary on the wire: a pack's row, one ranked entry.

use serde::{Deserialize, Serialize};

/// One download's middle, as download-core draws it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProgressMirror {
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub speed: Option<f64>,
    pub eta_secs: Option<u64>,
    pub message: Option<String>,
}

/// A pack's row: what it is and where it stands.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackMirror {
    pub id: String,
    pub label: String,
    pub source: String,
    pub target: String,
    pub rows: u64,
    pub built: bool,
    pub progress: Option<ProgressMirror>,
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    /// Why a phase is what it is: a failure's own words.
    pub message: Option<String>,
}

impl PackMirror {
    /// The wire phase, with the fields as a fallback for old rows.
    pub fn phase(&self) -> &str {
        if !self.phase.is_empty() {
            return &self.phase;
        }
        if self.built {
            return "ready";
        }
        match self.progress.as_ref().map(|p| p.phase.as_str()) {
            Some("downloading") | Some("preparing") | Some("retrying") | Some("verifying") => {
                "downloading"
            }
            Some("paused") => "paused",
            Some("failed") => "failed",
            _ => "absent",
        }
    }

    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let progress = self.progress.as_ref()?;
        let total = progress.total?;
        (total > 0).then_some(((progress.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }
}

/// One ranked entry on the wire to a card or a search list.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryMirror {
    pub word: String,
    pub pos_raw: Option<String>,
    /// Canon tags as snake_case words: `verb`, `noun`, ...
    pub tags: Vec<String>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
    pub pack: String,
    /// The bridge word, when two packs carried the reader here.
    pub via: Option<String>,
    /// `exact` | `prefix` | `substring` | `fuzzy`.
    pub word_match: String,
}
