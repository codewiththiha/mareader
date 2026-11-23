//! Study mode's card writer and judge, off the UI thread.
//!
//! A deck's backs come in one request, and a recall's verdict in another.
//! Each goes through the dispatcher: decks written ahead wait their turn as
//! long as it takes, and the judge goes before them. Dropping a job frees
//! its place.

use std::sync::Arc;

use mareader_settings::TaskId;
use mareader_study::mode::{
    Attempt, CardJob, CardWriter, Judge, JudgeJob, Judged, Wanted, Written,
};

use super::Assist;
use crate::backend::Backend;
use crate::dispatch::Dispatcher;
use crate::retry;
use crate::running::{Running, Say};
use crate::study_judge::StudyJudge;
use crate::study_mode::StudyCards;

impl Assist {
    /// The writer of study mode's cards, on the cards' model.
    pub fn cards(&self) -> Arc<dyn CardWriter> {
        Arc::new(AiCards(self.jobs_on.clone()))
    }

    /// The judge of Reactive mode's recalls, on the judge's model.
    pub fn judge(&self) -> Arc<dyn Judge> {
        Arc::new(AiCards(self.jobs_on.clone()))
    }
}

/// Study's models, as both the card writer and the judge; each job reads
/// the settings followed when it starts.
struct AiCards(Dispatcher);

impl CardWriter for AiCards {
    fn by(&self) -> String {
        self.0.followed().by(TaskId::StudyCards)
    }

    fn id(&self) -> String {
        let now = self.0.followed();
        let language = now.ai.study_language().unwrap_or("English");
        format!("{}|{language}", now.by(TaskId::StudyCards))
    }

    fn write(&self, words: Vec<Wanted>) -> Box<CardJob> {
        let now = self.0.followed();
        let (words, language) = (Arc::new(words), now.ai.study_language());
        // Each card comes as it is written, so a waiting deck fills.
        let step = move |backend: Arc<dyn Backend>, retry, say: Say<_, _>| {
            let words = Arc::clone(&words);
            async move {
                let task = StudyCards {
                    words: &words,
                    language,
                };
                retry::attempt(backend.as_ref(), &task, &|b| say.partial(b), retry).await
            }
        };
        let id = TaskId::StudyCards;
        let job: Running<Vec<Option<Written>>> = self.0.dispatch().ask(id, now.route(id), step);
        Box::new(job)
    }
}

impl Judge for AiCards {
    fn by(&self) -> String {
        self.0.followed().by(TaskId::StudyJudge)
    }

    fn judge(&self, attempt: Attempt) -> Box<JudgeJob> {
        let now = self.0.followed();
        let (attempt, language) = (Arc::new(attempt), now.ai.study_language());
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
        let id = TaskId::StudyJudge;
        let job: Running<Judged> = self.0.dispatch().ask(id, now.route(id), step);
        Box::new(job)
    }
}
