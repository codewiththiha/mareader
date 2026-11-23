//! Study mode's card writer and judge, off the UI thread.
//!
//! A deck's backs come in one request, and a recall's verdict in another.
//! Each takes a short slot on the study task's model, so decks written ahead
//! wait their turn behind Explain; dropping a job frees its slot.

use std::future::Future;
use std::sync::Arc;

use mareader_settings::ModelPreset;
use mareader_study::mode::{
    Answer, Attempt, CardJob, CardWriter, Judge, JudgeJob, Judged, Judgement, Wanted, Written,
};
use mareader_study::{Failure, Why};

use super::{Assist, Engine, Waker, short_slot};
use crate::backend::{Backend, Nobody, run};
use crate::running::{Running, start};
use crate::slots::{Slots, Ticket};
use crate::study_judge::StudyJudge;
use crate::study_mode::StudyCards;

impl Assist {
    /// The writer of study mode's cards, on the study task's model.
    pub fn cards(&self) -> Arc<dyn CardWriter> {
        Arc::new(self.study_ai())
    }

    /// The judge of Reactive mode's recalls, on the study task's model.
    pub fn judge(&self) -> Arc<dyn Judge> {
        Arc::new(self.study_ai())
    }

    fn study_ai(&self) -> AiCards {
        let preset = self.ai.study_preset().cloned();
        let backend = match &preset {
            Some(p) => self.backend(p),
            None => Arc::new(Nobody(Why::NotHere.into())),
        };
        AiCards {
            engine: Arc::clone(&self.engine),
            slots: self.slots.clone(),
            waker: Arc::clone(&self.waker),
            by: preset.as_ref().map(ModelPreset::title).unwrap_or_default(),
            preset,
            backend,
            language: self.ai.study_language(),
        }
    }
}

/// Study's model, as both the card writer and the judge.
struct AiCards {
    engine: Arc<Engine>,
    slots: Slots,
    waker: Waker,
    preset: Option<ModelPreset>,
    backend: Arc<dyn Backend>,
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
        let (slots, preset, backend) = self.parts();
        Box::new(self.spawn(write_deck(slots, preset, backend, words, self.language)))
    }
}

impl Judge for AiCards {
    fn by(&self) -> String {
        self.by.clone()
    }

    fn judge(&self, attempt: Attempt) -> Box<dyn JudgeJob> {
        let (slots, preset, backend) = self.parts();
        Box::new(self.spawn(judge_one(slots, preset, backend, attempt, self.language)))
    }
}

type Parts = (Slots, Option<ModelPreset>, Arc<dyn Backend>);

impl AiCards {
    fn parts(&self) -> Parts {
        let backend = Arc::clone(&self.backend);
        (self.slots.clone(), self.preset.clone(), backend)
    }

    /// Runs `asked` on the runtime; its result comes back through the job.
    fn spawn<T: Send + 'static>(
        &self,
        asked: impl Future<Output = Result<T, Failure>> + Send + 'static,
    ) -> Pending<T> {
        start(self.engine.rt(), &self.waker, |_| asked)
    }
}

/// One deck's backs, once the model has a slot free.
async fn write_deck(
    slots: Slots,
    preset: Option<ModelPreset>,
    backend: Arc<dyn Backend>,
    words: Vec<Wanted>,
    language: Option<&'static str>,
) -> Answer {
    let _ticket = slot(&slots, preset.as_ref(), backend.as_ref()).await?;
    let task = StudyCards {
        words: &words,
        language,
    };
    run(backend.as_ref(), &task, &|_| {}).await
}

/// One recall's verdict, once the model has a slot free.
async fn judge_one(
    slots: Slots,
    preset: Option<ModelPreset>,
    backend: Arc<dyn Backend>,
    attempt: Attempt,
    language: Option<&'static str>,
) -> Judgement {
    let _ticket = slot(&slots, preset.as_ref(), backend.as_ref()).await?;
    let task = StudyJudge {
        attempt: &attempt,
        language,
    };
    run(backend.as_ref(), &task, &|_| {}).await
}

/// A short slot on study's model, or why there is none.
async fn slot(
    slots: &Slots,
    preset: Option<&ModelPreset>,
    backend: &dyn Backend,
) -> Result<Ticket, Failure> {
    let Some(preset) = preset else {
        let none = "no model is chosen for study; pick one in Settings › AI";
        return Err(Failure::detailed(Why::NotHere, none));
    };
    short_slot(slots, preset, backend).await
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
