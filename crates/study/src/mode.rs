//! Study mode: proactive (learn the words before reading them) or reactive
//! (read freely, and prove each word as you meet it). See
//! `docs/study-mode.md`.

use serde::{Deserialize, Serialize};

/// Which way study mode teaches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyMode {
    #[default]
    Off,
    /// Learn the words before reading their pages: a batch gates its pages.
    Proactive,
    /// Read freely, and recall each shaded word as it comes up.
    Reactive,
}

impl StudyMode {
    pub const ALL: [StudyMode; 3] = [Self::Off, Self::Proactive, Self::Reactive];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Proactive => "Proactive",
            Self::Reactive => "Reactive",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Off => "Read as usual",
            Self::Proactive => "Study the words of each batch before reading on",
            Self::Reactive => "Recall each shaded word as you meet it",
        }
    }

    /// Whether reading past a batch is locked until its words are studied.
    pub fn locks_pages(self) -> bool {
        matches!(self, Self::Proactive)
    }
}

/// Where a deck's cards come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardSource {
    /// The AI when a model resolves for the study task, else the dictionary.
    #[default]
    Auto,
    Ai,
    Dict,
}

impl CardSource {
    pub const ALL: [CardSource; 3] = [Self::Auto, Self::Ai, Self::Dict];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Ai => "AI",
            Self::Dict => "Dictionary",
        }
    }
}

/// All of study mode's settings, shared with Vocabulary's level and the
/// highlighters' reserved looks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StudyModeSettings {
    pub mode: StudyMode,
    pub unit: super::BatchUnit,
    pub pages: usize,
    pub chars: usize,
    /// Decks prepared while the reader reads (0 waits until the lock).
    pub ahead: usize,
    pub sec_per_card: u8,
    pub strict: bool,
    /// Shade hard words even while proactive is a test.
    pub hints: bool,
    pub max_cards: usize,
    pub source: CardSource,
    /// Ask before a correct answer becomes memorized.
    pub ask_memorize: bool,
}

impl StudyModeSettings {
    pub const AHEAD_MIN: usize = 0;
    pub const AHEAD_MAX: usize = 3;
    pub const AHEAD: usize = 1;

    pub const SEC_PER_CARD_MIN: u8 = 2;
    pub const SEC_PER_CARD_MAX: u8 = 20;
    pub const SEC_PER_CARD: u8 = 6;

    pub const MAX_CARDS_MIN: usize = super::MAX_CARDS_MIN;
    pub const MAX_CARDS_MAX: usize = super::MAX_CARDS_MAX;
    pub const MAX_CARDS: usize = super::DEFAULT_MAX_CARDS;

    pub fn sanitize(&mut self) {
        self.pages = self
            .pages
            .clamp(super::BatchCfg::PAGES_MIN, super::BatchCfg::PAGES_MAX);
        self.chars = self
            .chars
            .clamp(super::BatchCfg::CHARS_MIN, super::BatchCfg::CHARS_MAX);
        self.ahead = self.ahead.clamp(Self::AHEAD_MIN, Self::AHEAD_MAX);
        self.sec_per_card = self
            .sec_per_card
            .clamp(Self::SEC_PER_CARD_MIN, Self::SEC_PER_CARD_MAX);
        self.max_cards = self
            .max_cards
            .clamp(Self::MAX_CARDS_MIN, Self::MAX_CARDS_MAX);
    }
}

impl Default for StudyModeSettings {
    fn default() -> Self {
        Self {
            mode: StudyMode::default(),
            unit: super::BatchUnit::default(),
            pages: super::BatchCfg::PAGES,
            chars: super::BatchCfg::CHARS,
            ahead: Self::AHEAD,
            sec_per_card: Self::SEC_PER_CARD,
            strict: true,
            hints: false,
            max_cards: Self::MAX_CARDS,
            source: CardSource::default(),
            ask_memorize: true,
        }
    }
}
