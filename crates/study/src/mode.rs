//! Study mode's rules: batches, decks, keys, recall and what each mode needs.
//!
//! Proactive mode shows a batch's hard words as cards before its pages
//! unlock; Reactive mode asks the reader to recall a shaded word's meaning.
//! Everything here is pure: the engine feeds it pages, levels and answers.
//! See `docs/study.md` section Study mode.

mod batches;
mod cards;
mod deck;
mod judge;
mod keys;
mod recall;

use serde::{Deserialize, Serialize};

pub use crate::class::{Classes, Pos};
pub use batches::{
    Batch, LIGHT_CHARS, MAX_PAGES, Size, Unit, gate, plan, plan_from, plan_words, retypeset,
};
pub use cards::{
    Answer, CardJob, CardWriter, PER_REQUEST, PER_REQUEST_MAX, Practice, SLOW_SECS, Wanted,
    Written, chunks, practice_back, request_chunks,
};
pub use deck::{Allowance, Card, Deck, MOST_CARDS, Sighting, also_as, build};
pub use judge::{Attempt, Judge, JudgeJob, Judged, Judgement, practice_verdict};
pub use keys::{Key, bases, lemma, proper};
pub use recall::{Done, MAX_TRIES, Outcome, Phase, Recall, Verdict};

/// The two ways to study while reading.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Words first, then the pages.
    #[default]
    Proactive,
    /// Read freely; recall shaded words when asked.
    Reactive,
}

impl Mode {
    pub const ALL: [Self; 2] = [Self::Proactive, Self::Reactive];

    pub fn name(self) -> &'static str {
        match self {
            Self::Proactive => "proactive",
            Self::Reactive => "reactive",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Proactive => "Proactive",
            Self::Reactive => "Reactive",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name.trim())
    }
}

/// Where cards come from, as the reader picks it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The AI when there is one, else a dictionary.
    #[default]
    Auto,
    Ai,
    Dictionary,
}

/// Who writes a deck's cards this time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Via {
    Ai,
    Dictionary,
}

/// What the machine and the book have, as the modes need it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Have {
    /// Word levels are downloaded.
    pub cefr: bool,
    /// A model is chosen for study.
    pub ai: bool,
    /// A dictionary pack answers from English.
    pub dict: bool,
    /// The book reads as English, or says no language.
    pub english: bool,
}

/// Why a mode can't run, and what fixes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missing {
    Levels,
    English,
    /// Proactive needs an AI or a dictionary to write cards.
    Cards,
    /// Reactive needs an AI to judge answers.
    Judge,
}

impl Missing {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Levels => "Needs word levels. Download them in Settings › Vocabulary.",
            Self::English => "Study mode reads English books only.",
            Self::Cards => "Needs an AI model or a dictionary pack to write the cards.",
            Self::Judge => "Needs an AI model to check your answers. A dictionary can't judge.",
        }
    }

    /// The fix button's words; none when nothing in the app fixes it.
    pub fn fix(self) -> Option<&'static str> {
        match self {
            Self::Levels => Some("Get word data…"),
            Self::English => None,
            Self::Cards | Self::Judge => Some("Set up AI…"),
        }
    }
}

/// Whether `mode` can run with what there is.
pub fn check(mode: Mode, have: Have) -> Result<(), Missing> {
    if !have.cefr {
        return Err(Missing::Levels);
    }
    if !have.english {
        return Err(Missing::English);
    }
    match mode {
        Mode::Proactive if !(have.ai || have.dict) => Err(Missing::Cards),
        Mode::Reactive if !have.ai => Err(Missing::Judge),
        Mode::Proactive | Mode::Reactive => Ok(()),
    }
}

/// Who writes the cards: `source` when both could, else whoever is there.
pub fn via(source: Source, have: Have) -> Option<Via> {
    match source {
        Source::Dictionary if have.dict => Some(Via::Dictionary),
        _ if have.ai => Some(Via::Ai),
        _ => have.dict.then_some(Via::Dictionary),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Have = Have {
        cefr: true,
        ai: true,
        dict: true,
        english: true,
    };

    #[test]
    fn each_mode_says_what_it_lacks_levels_first() {
        assert_eq!(check(Mode::Proactive, ALL), Ok(()));
        let bare = Have {
            cefr: false,
            ..Have::default()
        };
        assert_eq!(check(Mode::Reactive, bare), Err(Missing::Levels));
        let french = Have {
            english: false,
            ..ALL
        };
        assert_eq!(check(Mode::Proactive, french), Err(Missing::English));
        let dict_only = Have { ai: false, ..ALL };
        assert_eq!(check(Mode::Proactive, dict_only), Ok(()));
        assert_eq!(check(Mode::Reactive, dict_only), Err(Missing::Judge));
        let neither = Have {
            dict: false,
            ..dict_only
        };
        assert_eq!(check(Mode::Proactive, neither), Err(Missing::Cards));
        assert_eq!(Missing::English.fix(), None);
    }

    #[test]
    fn auto_prefers_the_ai_and_a_pick_counts_only_with_both() {
        let dict_only = Have { ai: false, ..ALL };
        let ai_only = Have { dict: false, ..ALL };
        assert_eq!(via(Source::Auto, ALL), Some(Via::Ai));
        assert_eq!(via(Source::Auto, dict_only), Some(Via::Dictionary));
        assert_eq!(via(Source::Dictionary, ALL), Some(Via::Dictionary));
        assert_eq!(via(Source::Ai, dict_only), Some(Via::Dictionary));
        assert_eq!(via(Source::Dictionary, ai_only), Some(Via::Ai));
        let neither = Have {
            ai: false,
            ..ai_only
        };
        assert_eq!(via(Source::Auto, neither), None);
        assert_eq!(Mode::parse("reactive"), Some(Mode::Reactive));
        assert_eq!(Mode::parse("off"), None);
    }
}
