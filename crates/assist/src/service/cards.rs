//! Study mode's card writer and judge, off the UI thread.
//!
//! A deck's backs come in one request, and a recall's verdict in another.
//! Each goes through the dispatcher: decks written ahead wait their turn as
//! long as it takes, and the judge goes before them. Dropping a job frees
//! its place.

use std::sync::Arc;

use mareader_settings::TaskId;
use mareader_study::mode::{
    Answer, Attempt, CardJob, CardWriter, Judge, JudgeJob, Judged, Judgement, Wanted, Written,
};

use super::{Assist, Line};
use crate::backend::Backend;
use crate::dispatch::Seat;
use crate::retry;
use crate::running::{Running, Say};
use crate::study_judge::StudyJudge;
use crate::study_mode::StudyCards;

impl Assist {
    /// The writer of study mode's cards, on the cards' model.
    pub fn cards(&self) -> Arc<dyn CardWriter> {
        Arc::new(self.study_ai(TaskId::StudyCards))
    }

    /// The judge of Reactive mode's recalls, on the judge's model.
    pub fn judge(&self) -> Arc<dyn Judge> {
        Arc::new(self.study_ai(TaskId::StudyJudge))
    }

    fn study_ai(&self, task: TaskId) -> AiCards {
        let route = self.route(task);
        AiCards {
            line: self.line(),
            by: route.first().map(|s| s.preset.title()).unwrap_or_default(),
            route,
            language: self.ai.study_language(),
        }
    }
}

/// Study's model, as both the card writer and the judge.
struct AiCards {
    line: Line,
    route: Vec<Seat>,
    language: Option<&'static str>,
    by: String,
}

impl CardWriter for AiCards {
    fn by(&self) -> String {
        self.by.clone()
    }

    fn id(&self) -> String {
        format!("{}|{}", self.by, self.language.unwrap_or("English"))
    }

    fn write(&self, words: Vec<Wanted>) -> Box<dyn CardJob> {
        let (words, language) = (Arc::new(words), self.language);
        let step = move |backend: Arc<dyn Backend>, retry, _: Say<_, _>| {
            let words = Arc::clone(&words);
            async move {
                let task = StudyCards {
                    words: &words,
                    language,
                };
                retry::attempt(backend.as_ref(), &task, &|_| {}, retry).await
            }
        };
        let (id, route) = (TaskId::StudyCards, self.route.clone());
        let job: Pending<Vec<Option<Written>>> = self.line.dispatch().ask(id, route, step);
        Box::new(job)
    }
}

impl Judge for AiCards {
    fn by(&self) -> String {
        self.by.clone()
    }

    fn judge(&self, attempt: Attempt) -> Box<dyn JudgeJob> {
        let (attempt, language) = (Arc::new(attempt), self.language);
        let step = move |backend: Arc<dyn Backend>, retry, _: Say<_, _>| {
            let attempt = Arc::clone(&attempt);
            async move {
                let task = StudyJudge {
                    attempt: &attempt,
                    language,
                };
                retry::attempt(backend.as_ref(), &task, &|_| {}, retry).await
            }
        };
        let (id, route) = (TaskId::StudyJudge, self.route.clone());
        let job: Pending<Judged> = self.line.dispatch().ask(id, route, step);
        Box::new(job)
    }
}

/// A request on its way; dropping it cancels it and frees its model.
type Pending<T> = Running<T>;

impl CardJob for Pending<Vec<Option<Written>>> {
    fn take(&mut self) -> Option<Answer> {
        self.answer()
    }
}

impl JudgeJob for Pending<Judged> {
    fn take(&mut self) -> Option<Judgement> {
        self.answer()
    }
}
