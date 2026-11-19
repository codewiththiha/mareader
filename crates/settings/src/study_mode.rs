//! Study mode's choices and its AI task; see `docs/study-mode.md`.

use serde::{Deserialize, Serialize};

use crate::ai::{AiSettings, ModelPreset, PresetId, language_name};
use crate::study::CefrLevel;

/// How a batch size is chosen in study mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyBatchUnit {
    #[default]
    Pages,
    Chars,
}

/// Who writes study cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyCardSource {
    #[default]
    Auto,
    Ai,
    Dictionary,
}

/// Study mode's own choices; the mode itself is kept per book.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyModeSettings {
    pub unit: StudyBatchUnit,
    /// Pages per batch, 1 to 10.
    pub pages: u8,
    /// Character budget per batch.
    pub chars: u32,
    /// Decks prepared while reading, 0 to 3.
    pub ahead: u8,
    /// Study seconds per card, 2 to 20.
    pub sec_per_card: u8,
    /// Start reading waits for the timer.
    pub strict: bool,
    /// Shade hard words in proactive mode.
    pub hints: bool,
    /// Words per deck, 4 to 30.
    pub max_cards: u8,
    pub source: StudyCardSource,
    /// Ask after a correct recall before memorizing.
    pub ask_memorize: bool,
}

impl Default for StudyModeSettings {
    fn default() -> Self {
        Self {
            unit: StudyBatchUnit::Pages,
            pages: 5,
            chars: 2400,
            ahead: 1,
            sec_per_card: 6,
            strict: true,
            hints: false,
            max_cards: 12,
            source: StudyCardSource::Auto,
            ask_memorize: true,
        }
    }
}

impl StudyModeSettings {
    pub fn sanitize(&mut self) {
        self.pages = self.pages.clamp(1, 10);
        self.chars = self.chars.clamp(500, 8000);
        self.ahead = self.ahead.min(3);
        self.sec_per_card = self.sec_per_card.clamp(2, 20);
        self.max_cards = self.max_cards.clamp(4, 30);
    }
}

/// Study cards and recall: who answers, and the meaning's language.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyTask {
    /// None follows Explain's model.
    pub preset: Option<PresetId>,
    /// A tag from [`LANGUAGES`]; none writes in the book's language.
    pub language: Option<String>,
}

impl AiSettings {
    /// The model study mode asks: its own, else Explain's.
    pub fn study_preset(&self) -> Option<&ModelPreset> {
        let own = self.tasks.study.preset.and_then(|id| self.preset(id));
        own.or_else(|| self.explain_preset())
    }

    /// The study meaning's language as a name, for the prompt.
    pub fn study_language(&self) -> Option<&'static str> {
        self.tasks.study.language.as_deref().and_then(language_name)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CefrSettings {
    pub enabled: bool,
    pub level: CefrLevel,
    /// Clicking a highlighted word explains it.
    pub click_explain: bool,
}

impl Default for CefrSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            level: CefrLevel::B2,
            click_explain: true,
        }
    }
}
