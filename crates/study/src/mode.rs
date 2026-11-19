//! Study modes, card sources and when each mode can run.

use serde::{Deserialize, Serialize};

/// How study mode teaches, or that it is off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StudyMode {
    #[default]
    Off,
    Proactive,
    Reactive,
}

impl StudyMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Proactive => "Proactive",
            Self::Reactive => "Reactive",
        }
    }

    /// Parses a settings or script name.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "proactive" => Some(Self::Proactive),
            "reactive" => Some(Self::Reactive),
            _ => None,
        }
    }
}

/// Who writes a deck's cards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CardSource {
    #[default]
    Auto,
    Ai,
    Dictionary,
}

/// What this machine has, for the capability matrix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StudyEnv {
    pub cefr: bool,
    pub ai: bool,
    pub dict: bool,
    /// CEFR levels and the tagger are English-only today.
    pub english: bool,
}

/// Whether one mode can run, and if not, why and what fixes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Avail {
    pub ok: bool,
    pub reason: Option<&'static str>,
    /// One of `cefr`, `ai` or `dict`, when a fix exists.
    pub fix: Option<&'static str>,
    pub fix_label: Option<&'static str>,
}

impl Avail {
    fn yes() -> Self {
        Self {
            ok: true,
            reason: None,
            fix: None,
            fix_label: None,
        }
    }

    fn no(reason: &'static str, fix: &'static str, fix_label: &'static str) -> Self {
        Self {
            ok: false,
            reason: Some(reason),
            fix: Some(fix),
            fix_label: Some(fix_label),
        }
    }
}

/// Both modes' availability on this machine, for this book.
pub fn availability(env: StudyEnv) -> (Avail, Avail) {
    if !env.english {
        let no = Avail::no(
            "Study mode needs an English book. Word levels are English-only.",
            "cefr",
            "Get word data…",
        );
        return (no.clone(), no);
    }
    if !env.cefr {
        let no = Avail::no(
            "Needs word levels. Download them in Settings › Vocabulary.",
            "cefr",
            "Get word data…",
        );
        return (no.clone(), no);
    }
    let proactive = if env.ai || env.dict {
        Avail::yes()
    } else {
        Avail::no(
            "Needs an AI model or a dictionary pack to write the cards.",
            "ai",
            "Set up AI…",
        )
    };
    let reactive = if env.ai {
        Avail::yes()
    } else {
        Avail::no(
            "Needs an AI model to check your answers. A dictionary can't judge.",
            "ai",
            "Set up AI…",
        )
    };
    (proactive, reactive)
}

/// Which source writes cards, if any can.
pub fn resolve_via(source: CardSource, env: StudyEnv) -> Option<&'static str> {
    match source {
        CardSource::Ai => env.ai.then_some("ai"),
        CardSource::Dictionary => env.dict.then_some("dict"),
        CardSource::Auto => {
            if env.ai {
                Some("ai")
            } else if env.dict {
                Some("dict")
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(cefr: bool, ai: bool, dict: bool) -> StudyEnv {
        StudyEnv {
            cefr,
            ai,
            dict,
            english: true,
        }
    }

    #[test]
    fn no_word_levels_blocks_both_modes() {
        let (p, r) = availability(env(false, true, true));
        assert!(!p.ok && !r.ok);
        assert_eq!(p.fix, Some("cefr"));
    }

    #[test]
    fn a_dictionary_only_machine_runs_proactive_not_reactive() {
        let (p, r) = availability(env(true, false, true));
        assert!(p.ok);
        assert!(!r.ok);
    }

    #[test]
    fn no_ai_and_no_dictionary_blocks_both() {
        let (p, r) = availability(env(true, false, false));
        assert!(!p.ok && !r.ok);
    }

    #[test]
    fn a_non_english_book_blocks_both() {
        let mut e = env(true, true, true);
        e.english = false;
        let (p, r) = availability(e);
        assert!(!p.ok && !r.ok);
    }

    #[test]
    fn auto_prefers_ai_and_falls_back_to_the_dictionary() {
        assert_eq!(
            resolve_via(CardSource::Auto, env(true, true, true)),
            Some("ai")
        );
        assert_eq!(
            resolve_via(CardSource::Auto, env(true, false, true)),
            Some("dict")
        );
        assert_eq!(resolve_via(CardSource::Ai, env(true, false, true)), None);
    }
}
