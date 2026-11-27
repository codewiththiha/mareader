//! Study mode's choices: batch size, the cards' time, the lock and recall.

use mareader_study::mode::{MAX_PAGES, Mode, Source, Unit};
use serde::{Deserialize, Serialize};

use crate::{AiSettings, language_name};

/// How study mode sizes batches, times decks and asks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyModeSettings {
    /// The mode the menu turns on; whether it is on is each pane's own.
    pub mode: Mode,
    /// Words (the default) or pages; the old characters mode reads as words.
    pub unit: Unit,
    /// Pages per batch in pages mode, 1 to [`MAX_PAGES`].
    pub pages: u8,
    /// Decks prepared ahead while reading; 0 asks at the gate.
    pub ahead: u8,
    /// A deck closes for good once its time is spent.
    pub limit: bool,
    /// Seconds each card adds to a deck's time, with `limit` on: a deck
    /// opening with 12 cards at 25 has 5 minutes.
    pub card_seconds: u8,
    /// The pages before the batches are paled and locked.
    pub lock: bool,
    /// Proactive keeps the CEFR shading on.
    pub hints: bool,
    /// Words per deck: words mode breaks its batches near it, and a deck
    /// over 1.5 times it comes in parts.
    pub max_cards: u8,
    pub source: Source,
    /// Reactive asks before adding a recalled word to memorized.
    pub ask_memorize: bool,
}

impl StudyModeSettings {
    pub const PAGES: u8 = 3;
    pub const AHEAD: u8 = 1;
    pub const AHEAD_MAX: u8 = 3;
    pub const CARD_SECONDS: u8 = 25;
    pub const CARD_SECONDS_MIN: u8 = 5;
    pub const CARD_SECONDS_MAX: u8 = 120;
    pub const MAX_CARDS: u8 = 12;
    pub const CARDS_MIN: u8 = 4;
    pub const CARDS_MAX: u8 = 50;

    pub fn sanitize(&mut self) {
        let most = u8::try_from(MAX_PAGES).unwrap_or(u8::MAX);
        self.pages = self.pages.clamp(1, most);
        self.ahead = self.ahead.min(Self::AHEAD_MAX);
        let (lo, hi) = (Self::CARD_SECONDS_MIN, Self::CARD_SECONDS_MAX);
        self.card_seconds = self.card_seconds.clamp(lo, hi);
        self.max_cards = self.max_cards.clamp(Self::CARDS_MIN, Self::CARDS_MAX);
    }
}

impl Default for StudyModeSettings {
    fn default() -> Self {
        Self {
            mode: Mode::Proactive,
            unit: Unit::Words,
            pages: Self::PAGES,
            ahead: Self::AHEAD,
            limit: true,
            card_seconds: Self::CARD_SECONDS,
            lock: false,
            hints: false,
            max_cards: Self::MAX_CARDS,
            source: Source::Auto,
            ask_memorize: true,
        }
    }
}

/// Study's AI tasks, cards and judging: their language.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyTask {
    /// The meaning's language by tag; none follows the dictionary default.
    pub language: Option<String>,
}

impl AiSettings {
    /// The cards' language as a name; unresolved defaults use English.
    pub fn study_language(&self) -> Option<&'static str> {
        let own = self.tasks.study.language.as_deref();
        own.and_then(language_name).or(Some("English"))
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
    use crate::{Provider, TaskId};

    #[test]
    fn study_mode_values_are_pulled_into_range() {
        let mut s = StudyModeSettings {
            pages: 0,
            ahead: 9,
            card_seconds: 200,
            max_cards: 1,
            ..StudyModeSettings::default()
        };
        s.sanitize();
        assert_eq!((s.pages, s.ahead), (1, 3));
        assert_eq!((s.card_seconds, s.max_cards), (120, 4));
        let mut big = StudyModeSettings {
            pages: 40,
            max_cards: 90,
            ..StudyModeSettings::default()
        };
        big.sanitize();
        assert_eq!((big.pages, big.max_cards), (10, 50));
    }

    #[test]
    fn the_study_tasks_follow_explain_until_they_have_their_own() {
        let mut ai = AiSettings::default();
        let explain = ai.add_preset(Provider::Ollama, "llama3");
        let own = ai.add_preset(Provider::Ollama, "qwen3");
        ai.tasks.choose(TaskId::Explain, Some(explain));
        ai.tasks.explain.language = Some("my".into());
        let id = |ai: &AiSettings, t| ai.model(t).map(|p| p.id);
        assert_eq!(id(&ai, TaskId::StudyCards), Some(explain));
        assert_eq!(ai.study_language(), Some("English"));
        ai.tasks.choose(TaskId::StudyJudge, Some(own));
        ai.tasks.study.language = Some("fr".into());
        assert_eq!(id(&ai, TaskId::StudyJudge), Some(own));
        assert_eq!(id(&ai, TaskId::StudyCards), Some(explain));
        assert_eq!(ai.study_language(), Some("French"));
        ai.remove_preset(own);
        assert_eq!(ai.tasks.choice(TaskId::StudyJudge), None);
        ai.tasks.study.language = Some("zz".into());
        ai.sanitize();
        assert_eq!(ai.tasks.study.language, None);
    }

    #[test]
    fn old_settings_without_study_mode_read_as_defaults() {
        let s: StudyModeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s, StudyModeSettings::default());
        let old = r#"{"unit":"chars","chars":900}"#;
        let s: StudyModeSettings = serde_json::from_str(old).unwrap();
        assert_eq!(s.unit, Unit::Words, "characters mode reads as words");
        let s: StudyModeSettings = serde_json::from_str(r#"{"unit":"pages"}"#).unwrap();
        assert_eq!(s.unit, Unit::Pages);
        let old = r#"{"sec_per_card":6,"strict":true,"minutes":9}"#;
        let s: StudyModeSettings = serde_json::from_str(old).unwrap();
        assert!(s.limit && !s.lock, "the old timers' keys are let go");
        assert_eq!(s.card_seconds, StudyModeSettings::CARD_SECONDS);
    }

    #[test]
    fn study_recall_keeps_two_looks_no_pen_takes() {
        let mut h = crate::HighlightSettings {
            underline: mareader_appearance::Rgb(0x2e, 0xa0, 0x4f),
            ..crate::HighlightSettings::default()
        };
        h.missed = h.solved;
        h.sanitize();
        assert_ne!(h.missed, h.solved);
        assert!(!h.allowed(crate::Pen::Underline, h.solved.color));
        assert!(h.allowed(crate::Pen::Underline, h.underline));
        assert!(!h.colors(crate::Pen::Underline).contains(&h.missed.color));
    }
}
