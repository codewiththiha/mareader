//! Study mode's switch, dials and when each mode can run.

use serde::{Deserialize, Serialize};

/// How a batch's size is chosen: whole pages, or a character budget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchUnit {
    #[default]
    Pages,
    Chars,
}

/// Which study switch the reader is on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Off,
    /// Words first: a deck gates each next batch.
    Proactive,
    /// Read free; recall a shaded word when you meet it.
    Reactive,
}

/// How long deck prep and judging feel, in the prototype's demo speeds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Speed {
    #[default]
    Normal,
    Fast,
    Slow,
    Fail,
}

/// Who writes the cards: AI, the offline pack, or whichever answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    #[default]
    Auto,
    Ai,
    Dict,
}

/// What this machine has for the capability matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Env {
    pub cefr: bool,
    pub ai: bool,
    pub dict: bool,
    pub speed: Speed,
}

impl Default for Env {
    fn default() -> Self {
        Self {
            cefr: true,
            ai: true,
            dict: true,
            speed: Speed::Normal,
        }
    }
}

/// A one-click fix for an unavailable mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fix {
    Cefr,
    Ai,
    Dict,
}

/// Why a mode can't run, and the fix that makes it able to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Avail {
    pub ok: bool,
    pub reason: Option<String>,
    pub fix: Option<Fix>,
}

impl Avail {
    fn yes() -> Self {
        Self {
            ok: true,
            reason: None,
            fix: None,
        }
    }

    fn no(reason: &str, fix: Fix) -> Self {
        Self {
            ok: false,
            reason: Some(reason.to_owned()),
            fix: Some(fix),
        }
    }
}

/// The study dials, as settings hold them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cfg {
    pub unit: BatchUnit,
    /// Pages per batch in [`BatchUnit::Pages`], 1–10.
    pub pages: usize,
    /// Character budget in [`BatchUnit::Chars`].
    pub chars: usize,
    /// Decks prepared while you read, 0–3.
    pub ahead: usize,
    /// Study seconds added per card in the deck.
    pub sec_per_card: u32,
    /// Start reading waits for the timer when true.
    pub strict: bool,
    /// Proactive: keep CEFR shading on while reading.
    pub hints: bool,
    /// Words above this band (1 A1 … 6 C2) are hard.
    pub level: u8,
    /// Cards shown per deck, 4–30.
    pub max_cards: usize,
    /// Reactive: ask before marking memorized after a correct answer.
    pub ask_memorize: bool,
    pub source: Source,
}

impl Default for Cfg {
    fn default() -> Self {
        Self {
            unit: BatchUnit::Pages,
            pages: 3,
            chars: 2_400,
            ahead: 1,
            sec_per_card: 6,
            strict: true,
            hints: false,
            level: 3,
            max_cards: 12,
            ask_memorize: true,
            source: Source::Auto,
        }
    }
}

impl Cfg {
    /// Clamps every dial to the range the UI offers.
    pub fn sanitize(&mut self) {
        self.pages = self.pages.clamp(1, 10);
        self.chars = self.chars.clamp(500, 8_000);
        self.ahead = self.ahead.min(3);
        self.sec_per_card = self.sec_per_card.clamp(2, 20);
        self.level = self.level.clamp(1, 6);
        self.max_cards = self.max_cards.clamp(4, 30);
    }
}

/// Proactive first, then Reactive: both need word levels.
pub fn availability(env: &Env) -> (Avail, Avail) {
    let no_levels = Avail::no(
        "Needs word levels. Download them in Settings › Vocabulary.",
        Fix::Cefr,
    );
    let proactive = if !env.cefr {
        no_levels.clone()
    } else if env.ai || env.dict {
        Avail::yes()
    } else {
        Avail::no(
            "Needs an AI model or a dictionary pack to write the cards.",
            Fix::Ai,
        )
    };
    let reactive = if !env.cefr {
        no_levels
    } else if env.ai {
        Avail::yes()
    } else {
        Avail::no(
            "Needs an AI model to check your answers. A dictionary can't judge.",
            Fix::Ai,
        )
    };
    (proactive, reactive)
}

/// Which source answers under [`Source::Auto`], or none if neither can.
pub fn resolve_via(source: Source, env: &Env) -> Option<Source> {
    match source {
        Source::Ai => env.ai.then_some(Source::Ai),
        Source::Dict => env.dict.then_some(Source::Dict),
        Source::Auto => {
            if env.ai {
                Some(Source::Ai)
            } else if env.dict {
                Some(Source::Dict)
            } else {
                None
            }
        }
    }
}

/// Milliseconds a demo speed pretends a call takes.
pub fn speed_ms(speed: Speed) -> u64 {
    match speed {
        Speed::Fast => 1_200,
        Speed::Normal => 3_600,
        Speed::Slow => 16_000,
        Speed::Fail => 2_600,
    }
}

/// The key a memorized word is stored under: lemma and class.
pub fn word_key(lemma: &str, pos: &str) -> String {
    format!(
        "{}|{}",
        lemma.trim().to_lowercase(),
        pos.trim().to_lowercase()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_gate_both_modes() {
        let env = Env {
            cefr: false,
            ..Env::default()
        };
        let (p, r) = availability(&env);
        assert!(!p.ok && !r.ok);
        assert_eq!(p.fix, Some(Fix::Cefr));
    }

    #[test]
    fn dict_alone_can_write_cards_but_not_judge() {
        let env = Env {
            ai: false,
            dict: true,
            ..Env::default()
        };
        let (p, r) = availability(&env);
        assert!(p.ok);
        assert!(!r.ok);
        assert_eq!(r.fix, Some(Fix::Ai));
    }

    #[test]
    fn auto_prefers_ai_then_dictionary() {
        let both = Env::default();
        assert_eq!(resolve_via(Source::Auto, &both), Some(Source::Ai));
        let dict_only = Env {
            ai: false,
            ..Env::default()
        };
        assert_eq!(resolve_via(Source::Auto, &dict_only), Some(Source::Dict));
        let none = Env {
            ai: false,
            dict: false,
            ..Env::default()
        };
        assert_eq!(resolve_via(Source::Auto, &none), None);
    }

    #[test]
    fn dials_clamp_to_their_ranges() {
        let mut cfg = Cfg {
            pages: 99,
            chars: 10,
            ahead: 9,
            sec_per_card: 0,
            level: 99,
            max_cards: 1,
            ..Cfg::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.pages, 10);
        assert_eq!(cfg.chars, 500);
        assert_eq!(cfg.ahead, 3);
        assert_eq!(cfg.sec_per_card, 2);
        assert_eq!(cfg.level, 6);
        assert_eq!(cfg.max_cards, 4);
    }
}
