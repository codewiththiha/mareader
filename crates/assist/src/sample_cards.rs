//! A card writer that answers as a model does, in JSON, for shots.
//!
//! The shot option `study_cards=ai` writes the answer a chatty model might
//! give to the study cards prompt, at the longest it allows, and reads it
//! with the real study cards task, so a shot shows the backs it makes.

use crate::{StudyCards, Task};
use mareader_study::mode::{CardJob, CardWriter, Wanted, Written};
use mareader_study::pending::{News, Pending};

/// One card of the answer; `{n}` and `{word}` name the asked word.
const CARD: &str = r#"{
  "n": {n},
  "pos": "Noun.",
  "meaning": "A short showing of how {word} works, given so people can see it in use before they decide to try it themselves.",
  "synonyms": ["showcase", "preview", "trial run", "walkthrough", "sample"],
  "usage": "Before lunch she gave the whole team a quick {word} of the new reading app."
}"#;

/// Writes every back from a model's answer, at once.
pub struct SampleCards;

/// The model's whole answer for `words`, as it would send it.
fn answer(words: &[Wanted]) -> String {
    let cards: Vec<String> = (words.iter().enumerate())
        .map(|(i, wanted)| {
            let word = serde_json::to_string(&wanted.form).unwrap_or_default();
            let word = word.trim_matches('"');
            let n = i.saturating_add(1).to_string();
            CARD.replace("{n}", &n).replace("{word}", word)
        })
        .collect();
    format!("{{\"cards\": [{}]}}", cards.join(",\n"))
}

impl CardWriter for SampleCards {
    fn by(&self) -> String {
        "Sample model".to_owned()
    }

    fn id(&self) -> String {
        "answered-json".to_owned()
    }

    fn write(&self, words: Vec<Wanted>) -> Box<CardJob> {
        let sent: serde_json::Value = serde_json::from_str(&answer(&words)).unwrap_or_default();
        let task = StudyCards {
            words: &words,
            language: None,
        };
        let backs = task.read(&sent, true);
        Box::new(Done(Some(backs.unwrap_or_else(|| vec![None; words.len()]))))
    }
}

/// Backs ready from the start.
struct Done(Option<Vec<Option<Written>>>);

impl Pending<Vec<Option<Written>>> for Done {
    fn news(&mut self) -> Vec<News<Vec<Option<Written>>>> {
        let done = self.0.take().map(|backs| News::Done(Ok(backs)));
        done.into_iter().collect()
    }
}
