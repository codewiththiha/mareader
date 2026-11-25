//! Who explains words: a contract, and the answerers every build has.

use crate::answer::WordInfo;
use crate::class::Pos;

/// Explains a word in context; runs on a worker, so it may block.
pub trait Explainer: Send + Sync {
    /// The answer; `partial` sees it fill in, when the answerer streams.
    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure>;

    /// Who answers, as the card's small print names them; blank for none.
    fn by(&self) -> String {
        String::new()
    }

    /// The answer and who gave it, which may differ from `by` beforehand;
    /// the tagger's class goes with the word as a guess, where one heeds it.
    fn explain_by(
        &self,
        (word, context, _hint): (&str, &str, Option<Pos>),
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<(WordInfo, String), Failure> {
        let by = self.by();
        self.explain(word, context, partial).map(|info| (info, by))
    }

    /// Answers on a worker even where asks run inline, as a slow answerer must.
    fn waits(&self) -> bool {
        false
    }
}

/// Why no answer came; the card shows [`Why::message`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Why {
    /// No AI is set up, or the chosen one can't run here.
    NotHere,
    /// The model is there but not ready yet.
    NotReady,
    /// The model declined the text.
    Refused,
    TimedOut,
    Busy,
    /// An answer came that couldn't be read.
    BadAnswer,
    /// The provider needs an API key and has none.
    NoKey,
    /// Every key was turned away.
    KeysRejected,
    /// The provider couldn't be reached.
    Offline,
    /// The provider turned the request down, as for an unknown model.
    Setup,
    /// The model can't work in the text's language.
    Language,
    /// The system keychain holding the keys is locked.
    Locked,
    /// The model already runs as many tasks as it's allowed.
    InUse,
}

impl Why {
    pub fn message(self) -> &'static str {
        match self {
            Self::NotHere => "No model is set for the AI Dictionary. Choose one in Settings › AI.",
            Self::NotReady => "The model isn't ready yet. Try again in a moment.",
            Self::Refused => "The model declined to look up this word.",
            Self::TimedOut => "The answer took too long.",
            Self::Busy => "The model is busy. Try again in a moment.",
            Self::BadAnswer => "The answer couldn't be read.",
            Self::NoKey => "This provider needs an API key. Add one in Settings › AI models.",
            Self::KeysRejected => {
                "Every API key was turned away. Check them in Settings › AI models."
            }
            Self::Offline => "The AI couldn't be reached. Check the connection or the server.",
            Self::Setup => {
                "The AI turned the request down. Check the model in Settings › AI models."
            }
            Self::Language => "The model can't define words in this language.",
            Self::Locked => "The keychain holding the API keys is locked.",
            Self::InUse => "This model is busy with another task. Try again when it's done.",
        }
    }

    /// Whether asking again, unchanged, may help.
    pub fn retry(self) -> bool {
        !matches!(
            self,
            Self::NotHere
                | Self::Refused
                | Self::NoKey
                | Self::KeysRejected
                | Self::Setup
                | Self::Language
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub why: Why,
    /// What the provider said, for the card's small print.
    pub detail: Option<String>,
    /// The provider already asked again; waiting more won't help.
    pub retried: bool,
}

impl Failure {
    /// Small print longer than this is cut.
    pub const DETAIL_CHARS: usize = 240;

    /// `why`, with the provider's words cut to [`Self::DETAIL_CHARS`].
    pub fn detailed(why: Why, detail: impl Into<String>) -> Self {
        let detail: String = detail.into();
        let detail = detail.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut cut: String = detail.chars().take(Self::DETAIL_CHARS).collect();
        if cut.len() < detail.len() {
            cut.push('…');
        }
        Self {
            why,
            detail: Some(cut).filter(|d| !d.is_empty()),
            retried: false,
        }
    }

    /// The reader's words: why, then the small print. A missing model's
    /// small print, from `no_model()`, is the whole of it.
    pub fn said(&self) -> String {
        let why = self.why.message();
        match &self.detail {
            Some(detail) if self.why == Why::NotHere => detail.clone(),
            Some(detail) => format!("{why} ({detail})"),
            None => why.to_owned(),
        }
    }

    /// Marks a failure the provider already asked again for.
    pub fn after_retries(self) -> Self {
        Self {
            retried: true,
            ..self
        }
    }
}

impl From<Why> for Failure {
    fn from(why: Why) -> Self {
        Self {
            why,
            detail: None,
            retried: false,
        }
    }
}

/// Explains nothing: this build or machine has no model.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unavailable;

impl Explainer for Unavailable {
    fn explain(
        &self,
        _: &str,
        _: &str,
        _: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        Err(Why::NotHere.into())
    }
}

/// Answers from a script, for tests and shots; "fail" fails retryably.
#[derive(Clone, Copy, Debug, Default)]
pub struct Canned;

impl Explainer for Canned {
    fn by(&self) -> String {
        "Sample answers".to_owned()
    }

    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        if word.eq_ignore_ascii_case("fail") {
            return Err(Failure::detailed(Why::NotReady, "scripted failure"));
        }
        let lower = word.to_lowercase();
        let info = match lower.as_str() {
            "quick" => WordInfo {
                pos: "adjective".into(),
                meaning: "Moving fast, or done in a short time.".into(),
                synonyms: vec!["fast".into(), "swift".into(), "rapid".into()],
                usages: vec![
                    "She took a quick look at the map.".into(),
                    "The quick reply surprised him.".into(),
                ],
                book_only: false,
            },
            _ => WordInfo {
                pos: "word".into(),
                meaning: format!("What “{word}” means here, in plain words."),
                synonyms: vec![format!("{lower}-like"), "kindred".into()],
                usages: vec![format!("As in: {context}")],
                book_only: false,
            },
        };
        // As a streaming model would: the meaning first.
        partial(WordInfo {
            pos: info.pos.clone(),
            meaning: info.meaning.clone(),
            ..WordInfo::default()
        });
        Ok(info.clamped())
    }
}

/// [`Canned`]'s answers after a wait of this many milliseconds, for shots and
/// tests of an answer still coming.
#[derive(Clone, Copy, Debug, Default)]
pub struct Slow(pub u64);

impl Explainer for Slow {
    fn by(&self) -> String {
        Canned.by()
    }

    fn explain(
        &self,
        word: &str,
        context: &str,
        partial: &(dyn Fn(WordInfo) + Sync),
    ) -> Result<WordInfo, Failure> {
        // A browser's one thread can't wait.
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::sleep(std::time::Duration::from_millis(self.0));
        Canned.explain(word, context, partial)
    }

    fn waits(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[test]
    fn a_failure_says_why_then_its_small_print() {
        let offline = Failure::detailed(Why::Offline, "refused").said();
        assert_eq!(offline, format!("{} (refused)", Why::Offline.message()));
        let none = Failure::detailed(Why::NotHere, "No AI model is set for this.");
        assert_eq!(none.said(), "No AI model is set for this.");
        assert_eq!(Failure::from(Why::Busy).said(), Why::Busy.message());
    }

    #[test]
    fn the_stand_ins_answer_as_promised() {
        let none = Unavailable.explain("fox", "the fox", &|_| {});
        assert_eq!(none, Err(Why::NotHere.into()));
        assert!(!Why::NotHere.retry());
        let seen = Mutex::new(Vec::new());
        let quick = Canned.explain("Quick", "The quick fox", &|p| {
            if let Ok(mut s) = seen.lock() {
                s.push(p);
            }
        });
        assert_eq!(quick.map(|i| i.pos).as_deref(), Ok("adjective"));
        let seen = seen.into_inner().unwrap_or_default();
        assert_eq!(seen.len(), 1);
        assert!(
            seen.iter()
                .all(|p| p.synonyms.is_empty() && !p.meaning.is_empty())
        );
        let failed = Canned.explain("fail", "", &|_| {});
        assert_eq!(failed.map_err(|f| f.why), Err(Why::NotReady));
        assert!(Why::NotReady.retry());
    }

    #[test]
    fn small_print_is_one_trimmed_line_and_cut() {
        let f = Failure::detailed(Why::Setup, "  model\n  not   found ");
        assert_eq!(f.detail.as_deref(), Some("model not found"));
        let long = Failure::detailed(Why::Setup, "é".repeat(500));
        let shown = long.detail.unwrap_or_default();
        assert_eq!(shown.chars().count(), Failure::DETAIL_CHARS + 1);
        assert!(shown.ends_with('…'));
        assert_eq!(Failure::detailed(Why::Busy, "  ").detail, None);
    }
}
