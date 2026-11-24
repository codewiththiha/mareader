//! A recall in Reactive mode: one shaded word, the reader's answers and the
//! judge's verdicts.
//!
//! One recall is open at a time. Opening it asks the AI for the word's back
//! (a meaning and synonyms, kept in [`super::Backs`]), which gives the hint
//! after a miss and the answer once revealed. Each answer goes to the
//! [`Judge`]; a judge that fails costs no try and keeps the answer to send
//! again. Closing the recall drops both jobs, which cancels them.

use std::ops::Range;
use std::sync::Arc;

use mareader_cefr::Ticket as TagTicket;
use mareader_dict::Ticket as ClassTicket;
use mareader_settings::{TaskId, no_model};
use mareader_study::mode::{
    Attempt, CardJob, Classes, Done, Judge, JudgeJob, Key, Outcome, Phase, Pos, Recall, Wanted,
    Written,
};
use mareader_workspace::PaneId;

use super::backs::Writer;

/// What the recall popover asks.
#[derive(Clone, Debug, PartialEq)]
pub enum Ask {
    /// Opens a recall on the word at `chars` of `page`.
    Open {
        page: usize,
        chars: Range<usize>,
    },
    /// Sends an answer to be judged.
    Answer(String),
    /// Back to the answer box after a miss, or after the judge failed.
    Again,
    /// "I don't know", or "Show answer" after a miss.
    Reveal,
    /// Ends it: memorized or learning once correct, missed once revealed.
    Finish(Outcome),
    Close,
}

/// The word a recall asks about, where the page shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub key: Key,
    /// As printed.
    pub form: String,
    pub sentence: String,
    pub band: u8,
    pub page: usize,
    pub chars: Range<usize>,
}

impl Word {
    fn wanted(&self) -> Wanted {
        Wanted {
            key: self.key.clone(),
            form: self.form.clone(),
            sentence: self.sentence.clone(),
        }
    }
}

/// One open recall.
pub struct Recalling {
    pub pane: PaneId,
    pub word: Word,
    pub recall: Recall,
    /// The last answer sent, kept when the judge fails.
    pub answer: String,
    /// Why the judge failed, while the popover says so.
    pub failure: Option<String>,
    /// The word's back once written; none while it is, or when it can't be.
    pub back: Option<Written>,
    /// Who judges, as the popover names them while it waits.
    pub by: String,
    /// The tagger's and the dictionary's classes, as they answer.
    pub heard: Classes,
    /// Their asks while out: the tagger's, then the dictionary's.
    pub(crate) asks: (Option<TagTicket>, Option<ClassTicket>),
    /// The dictionary was asked; it is asked once at most.
    pub(crate) looked_up: bool,
    /// The back being written, and who was asked, whose cache it goes in.
    back_job: Option<(String, Box<CardJob>)>,
    judging: Option<Box<JudgeJob>>,
}

impl Recalling {
    /// A recall on `word`, its back taken from `writer`'s cache or asked for.
    pub(crate) fn new(pane: PaneId, word: Word, writer: Option<Writer>, by: String) -> Self {
        let mut recalling = Self {
            pane,
            word,
            recall: Recall::default(),
            answer: String::new(),
            failure: None,
            back: None,
            by,
            heard: Classes::default(),
            asks: (None, None),
            looked_up: false,
            back_job: None,
            judging: None,
        };
        if let Some(writer) = writer {
            let wanted = recalling.word.wanted();
            let id = writer.cards.id();
            recalling.back = writer.backs.get(&id, &wanted);
            if recalling.back.is_none() {
                recalling.back_job = Some((id, writer.cards.write(vec![wanted])));
            }
        }
        recalling
    }

    /// Sends `answer` to `judge`; false when there is nothing to send.
    pub(crate) fn send(&mut self, answer: &str, judge: Option<Arc<dyn Judge>>) -> bool {
        if !self.recall.check(answer) {
            return false;
        }
        self.answer = answer.trim().to_owned();
        self.failure = None;
        let Some(judge) = judge else {
            self.recall.failed();
            self.failure = Some(no_model(TaskId::StudyJudge));
            return true;
        };
        let attempt = Attempt {
            form: self.word.form.clone(),
            lemma: self.word.key.lemma.clone(),
            sentence: self.word.sentence.clone(),
            answer: self.answer.clone(),
        };
        self.judging = Some(judge.judge(attempt));
        true
    }

    /// Takes in the back and the verdict: whether anything moved, and the
    /// finished recall when a correct verdict ends it at once.
    pub(crate) fn step(&mut self, ask: bool, writer: Option<&Writer>) -> (bool, Option<Done>) {
        let mut moved = false;
        if let Some(answer) = self.back_job.as_mut().and_then(|(_, job)| job.take()) {
            let id = self.back_job.take().map(|(id, _)| id).unwrap_or_default();
            moved = true;
            let back = answer
                .ok()
                .and_then(|backs| backs.into_iter().next().flatten());
            if let (Some(back), Some(writer)) = (&back, writer) {
                writer.backs.put(&id, &self.word.wanted(), back.clone());
            }
            self.back = back;
        }
        let Some(judged) = self.judging.as_mut().and_then(|job| job.take()) else {
            return (moved, None);
        };
        self.judging = None;
        match judged {
            Ok(j) => (true, self.recall.judged(j.verdict, j.feedback, ask)),
            Err(failure) => {
                self.recall.failed();
                self.failure = Some(failure.said());
                (true, None)
            }
        }
    }

    /// The class the word is kept under: the tagger's, else its back's,
    /// else the dictionary's.
    pub fn class(&self) -> Option<Pos> {
        let ai = self.back.as_ref().and_then(|b| b.pos);
        Classes { ai, ..self.heard }.class()
    }

    /// Whether the dictionary is the one source left to ask: the tagger
    /// and the back are in and named no class, and it was not yet asked.
    pub(crate) fn wants_dictionary(&self) -> bool {
        let others_done = self.asks.0.is_none() && !self.writing_back();
        others_done && self.class().is_none() && !self.looked_up
    }

    /// Whether a back or a verdict is still coming.
    pub fn busy(&self) -> bool {
        self.back_job.is_some() || self.judging.is_some()
    }

    /// Whether the judge works on an answer: none when not judging, else
    /// whether it waits in line for its model.
    pub fn judging(&self) -> Option<bool> {
        self.judging.as_ref().map(|job| job.queued())
    }

    /// Whether the word's back is still being written.
    pub fn writing_back(&self) -> bool {
        self.back_job.is_some()
    }

    /// The hint after the first miss: how a synonym starts.
    pub fn hint(&self) -> Option<String> {
        if !self.recall.hint() {
            return None;
        }
        let synonym = self.back.as_ref()?.synonyms.first()?;
        let first = synonym.chars().next()?;
        Some(format!("Hint: a synonym starts with “{first}”."))
    }

    /// How the recall ends if `outcome` is asked for now; none when it can't.
    pub fn ending(&self, outcome: Outcome) -> Option<Done> {
        let fits = match outcome {
            Outcome::Memorized | Outcome::Learning => self.recall.phase == Phase::Ask,
            Outcome::Missed => self.recall.phase == Phase::Revealed,
        };
        fits.then(|| self.recall.finish(outcome))
    }
}

#[cfg(test)]
mod tests {
    use mareader_study::mode::{Practice, Verdict};

    use super::*;
    use crate::study_mode::Backs;

    fn word() -> Word {
        Word {
            key: Key::new("brisk", None),
            form: "brisk".to_owned(),
            sentence: "A brisk walk.".to_owned(),
            band: 4,
            page: 0,
            chars: 2..7,
        }
    }

    fn writer(practice: Practice) -> Writer {
        Writer {
            cards: Arc::new(practice),
            backs: Backs::default(),
        }
    }

    #[test]
    fn a_miss_hints_from_the_back_and_a_correct_answer_asks() {
        let ready = writer(Practice::Ready);
        let judge: Arc<dyn Judge> = Arc::new(Practice::Ready);
        let mut r = Recalling::new(PaneId(1), word(), Some(ready.clone()), String::new());
        assert!(r.step(true, Some(&ready)).0, "the back comes in");
        assert_eq!(ready.backs.count(), 1, "and is kept");
        assert!(r.send("slow", Some(Arc::clone(&judge))));
        assert_eq!(r.step(true, Some(&ready)), (true, None));
        assert_eq!(r.recall.verdict, Some(Verdict::Wrong));
        assert_eq!(
            r.hint().as_deref(),
            Some("Hint: a synonym starts with “s”.")
        );
        assert_eq!(r.ending(Outcome::Memorized), None, "only once correct");
        assert!(r.recall.again());
        assert!(r.send("right: lively", Some(judge)));
        r.step(true, Some(&ready));
        assert_eq!(r.recall.phase, Phase::Ask);
        let done = r
            .ending(Outcome::Memorized)
            .map(|d| (d.attempts, d.first_try));
        assert_eq!(done, Some((2, false)));
        let again = Recalling::new(PaneId(1), word(), Some(ready), String::new());
        assert!(
            again.back.is_some() && !again.busy(),
            "a kept back is not asked again"
        );
    }

    #[test]
    fn the_dictionary_is_asked_only_when_tagger_and_back_name_no_class() {
        let ready = writer(Practice::Ready);
        let mut r = Recalling::new(PaneId(1), word(), Some(ready.clone()), String::new());
        r.asks.0 = Some(TagTicket(1));
        assert!(!r.wants_dictionary(), "the tagger is still out");
        r.asks.0 = None;
        assert!(!r.wants_dictionary(), "the back is still being written");
        r.step(true, Some(&ready));
        assert!(r.wants_dictionary(), "neither named a class");
        r.looked_up = true;
        assert!(!r.wants_dictionary(), "asked once at most");
        let mut tagged = Recalling::new(PaneId(1), word(), None, String::new());
        tagged.heard.tagger = Some(Pos::Adjective);
        assert!(!tagged.wants_dictionary(), "the tagger named it");
        assert_eq!(tagged.class(), Some(Pos::Adjective));
    }

    #[test]
    fn a_failed_judge_keeps_the_answer_and_costs_no_try() {
        let judge: Arc<dyn Judge> = Arc::new(Practice::Failing);
        let mut r = Recalling::new(PaneId(1), word(), None, String::new());
        assert!(!r.send("  ", Some(Arc::clone(&judge))), "nothing to send");
        assert!(r.send(" right ", Some(judge)));
        r.step(false, None);
        assert_eq!((r.recall.phase, r.recall.attempts), (Phase::Failed, 0));
        assert_eq!(r.answer, "right");
        assert!(
            r.failure
                .as_deref()
                .is_some_and(|f| f.contains("couldn't be reached"))
        );
        assert!(r.recall.again());
        assert!(r.send("right", None));
        assert_eq!(r.failure, Some(no_model(TaskId::StudyJudge)));
        r.recall.again();
        r.recall.reveal();
        let done = r.ending(Outcome::Missed).map(|d| d.attempts);
        assert_eq!(done, Some(1), "a reveal counts one attempt");
    }
}
