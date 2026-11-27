//! Study mode's card writer and judge, off the UI thread.
//!
//! A deck's backs come in requests of the cards' model's Cards per request
//! (the engine cuts them), and a recall's verdict in another. Each goes
//! through the dispatcher, which runs as many at once as the model takes:
//! decks written ahead wait their turn as long as it takes, and the judge
//! goes before them. Dropping a job frees its place.

use std::sync::Arc;

use mareader_settings::TaskId;
use mareader_study::mode::{
    Attempt, CardJob, CardWriter, Judge, JudgeJob, Judged, PER_REQUEST, PER_REQUEST_MAX, Wanted,
    Written,
};

use super::Assist;
use crate::backend::Backend;
use crate::dispatch::{Dispatcher, Followed};
use crate::retry;
use crate::running::{Running, Say};
use crate::study_judge::StudyJudge;
use crate::study_mode::StudyCards;

impl Assist {
    /// The writer of study mode's cards, on the cards' model.
    pub fn cards(&self) -> Arc<dyn CardWriter> {
        Arc::new(AiCards(self.jobs_on.clone(), None))
    }

    /// The judge of Reactive mode's recalls, on the judge's model.
    pub fn judge(&self) -> Arc<dyn Judge> {
        Arc::new(AiCards(self.jobs_on.clone(), None))
    }
}

/// Study's models, as both the card writer and the judge; each job reads
/// the settings followed when it starts.
struct AiCards(Dispatcher, Option<Arc<Followed>>);

impl AiCards {
    fn now(&self) -> Arc<Followed> {
        self.1.clone().unwrap_or_else(|| self.0.followed())
    }
}

impl CardWriter for AiCards {
    fn snapshot(&self) -> Option<Arc<dyn CardWriter>> {
        Some(Arc::new(Self(self.0.clone(), Some(self.now()))))
    }

    fn by(&self) -> String {
        self.now().by(TaskId::StudyCards)
    }

    fn id(&self) -> String {
        let now = self.now();
        let language = now.ai.study_language().unwrap_or("English");
        format!("{}|{language}", now.by(TaskId::StudyCards))
    }

    fn per_request(&self) -> usize {
        let route = self.now().route(TaskId::StudyCards);
        let set = route
            .first()
            .map_or(PER_REQUEST, |s| s.preset.cards_per_request);
        usize::from(set.clamp(1, PER_REQUEST_MAX))
    }

    fn write(&self, words: Vec<Wanted>) -> Box<CardJob> {
        let now = self.now();
        let (words, language) = (Arc::new(words), now.ai.study_language());
        let tag = now.ai.tasks.study.language.clone();
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
        let (who, route) = (now.asker(id), now.route(id));
        let job: Running<Vec<Option<Written>>> = self.0.dispatch().ask_in(who, route, tag, step);
        Box::new(job)
    }

    fn language(&self) -> Option<&'static str> {
        let now = self.now();
        Some(now.ai.study_language().unwrap_or("English"))
    }
}

impl Judge for AiCards {
    fn by(&self) -> String {
        self.0.followed().by(TaskId::StudyJudge)
    }

    fn judge(&self, attempt: Attempt) -> Box<JudgeJob> {
        let now = self.now();
        let (attempt, language) = (Arc::new(attempt), now.ai.study_language());
        let tag = now.ai.tasks.study.language.clone();
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
        let (who, route) = (now.asker(id), now.route(id));
        let job: Running<Judged> = self.0.dispatch().ask_in(who, route, tag, step);
        Box::new(job)
    }
}
