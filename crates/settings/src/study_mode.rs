//! Study mode's choices: batch size, the time floor, cards and recall.

use mareader_study::mode::{MAX_PAGES, Mode, Source, Unit};
use serde::{Deserialize, Serialize};

use crate::{AiSettings, ModelPreset, PresetId, language_name};

/// How study mode sizes batches, times decks and asks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyModeSettings {
    /// The mode the menu turns on; whether it is on is each pane's own.
    pub mode: Mode,
    pub unit: Unit,
    /// Pages per batch, 1 to [`MAX_PAGES`].
    pub pages: u8,
    /// Characters per batch in characters mode.
    pub chars: u32,
    /// Decks prepared ahead while reading; 0 asks at the gate.
    pub ahead: u8,
    pub sec_per_card: u8,
    /// Start waits for the time floor.
    pub strict: bool,
    /// Proactive keeps the CEFR shading on.
    pub hints: bool,
    pub max_cards: u8,
    pub source: Source,
    /// Reactive asks before adding a recalled word to memorized.
    pub ask_memorize: bool,
}

impl StudyModeSettings {
    pub const PAGES: u8 = 3;
    pub const CHARS: u32 = 2400;
    pub const CHARS_MIN: u32 = 300;
    pub const CHARS_MAX: u32 = 20_000;
    pub const AHEAD: u8 = 1;
    pub const AHEAD_MAX: u8 = 3;
    pub const SEC_PER_CARD: u8 = 6;
    pub const SEC_MIN: u8 = 2;
    pub const SEC_MAX: u8 = 20;
    pub const MAX_CARDS: u8 = 12;
    pub const CARDS_MIN: u8 = 4;
    pub const CARDS_MAX: u8 = 30;

    pub fn sanitize(&mut self) {
        let most = u8::try_from(MAX_PAGES).unwrap_or(u8::MAX);
        self.pages = self.pages.clamp(1, most);
        self.chars = self.chars.clamp(Self::CHARS_MIN, Self::CHARS_MAX);
        self.ahead = self.ahead.min(Self::AHEAD_MAX);
        self.sec_per_card = self.sec_per_card.clamp(Self::SEC_MIN, Self::SEC_MAX);
        self.max_cards = self.max_cards.clamp(Self::CARDS_MIN, Self::CARDS_MAX);
    }
}

impl Default for StudyModeSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Proactive,
            unit: Unit::Pages,
            pages: Self::PAGES,
            chars: Self::CHARS,
            ahead: Self::AHEAD,
            sec_per_card: Self::SEC_PER_CARD,
            strict: true,
            hints: false,
            max_cards: Self::MAX_CARDS,
            source: Source::Auto,
            ask_memorize: true,
        }
    }
}

/// The study AI task: cards and judging, with their own model and language.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyTask {
    /// None follows Explain's model.
    pub preset: Option<PresetId>,
    /// The meaning's language by tag; none follows Explain's.
    pub language: Option<String>,
}

impl AiSettings {
    /// The model study mode asks: its own, else Explain's.
    pub fn study_preset(&self) -> Option<&ModelPreset> {
        let own = self.tasks.study.preset.and_then(|id| self.preset(id));
        own.or_else(|| self.explain_preset())
    }

    /// The cards' language as a name: study's own, else Explain's.
    pub fn study_language(&self) -> Option<&'static str> {
        let own = self.tasks.study.language.as_deref();
        own.and_then(language_name)
            .or_else(|| self.explain_language())
    }
}

/// Forgets a language tag no AI task knows.
pub(crate) fn drop_unknown(language: &mut Option<String>) {
    if language.as_deref().and_then(language_name).is_none() {
        *language = None;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::Provider;

    #[test]
    fn study_mode_values_are_pulled_into_range() {
        let mut s = StudyModeSettings {
            pages: 0,
            chars: 5,
            ahead: 9,
            sec_per_card: 99,
            max_cards: 1,
            ..StudyModeSettings::default()
        };
        s.sanitize();
        let min = StudyModeSettings::CHARS_MIN;
        assert_eq!((s.pages, s.chars, s.ahead), (1, min, 3));
        assert_eq!((s.sec_per_card, s.max_cards), (20, 4));
        let mut big = StudyModeSettings {
            pages: 40,
            ..StudyModeSettings::default()
        };
        big.sanitize();
        assert_eq!(big.pages, 10);
    }

    #[test]
    fn the_study_task_follows_explain_until_it_has_its_own() {
        let mut ai = AiSettings::default();
        let explain = ai.add_preset(Provider::Ollama, "llama3");
        let own = ai.add_preset(Provider::Ollama, "qwen3");
        ai.tasks.explain.preset = Some(explain);
        ai.tasks.explain.language = Some("my".into());
        assert_eq!(ai.study_preset().map(|p| p.id), Some(explain));
        assert_eq!(ai.study_language(), Some("Burmese"));
        ai.tasks.study.preset = Some(own);
        ai.tasks.study.language = Some("fr".into());
        assert_eq!(ai.study_preset().map(|p| p.id), Some(own));
        assert_eq!(ai.study_language(), Some("French"));
        ai.remove_preset(own);
        assert_eq!(ai.tasks.study.preset, None);
        ai.tasks.study.language = Some("zz".into());
        ai.sanitize();
        assert_eq!(ai.tasks.study.language, None);
    }

    #[test]
    fn old_settings_without_study_mode_read_as_defaults() {
        let s: StudyModeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, StudyModeSettings::default());
        let s: StudyModeSettings = serde_json::from_str(r#"{"unit":"chars"}"#).unwrap();
        assert_eq!(s.unit, Unit::Chars);
    }
}
