//! Study mode's card writer: a deck's backs in one request, off the UI thread.
//!
//! Each deck takes a short slot on the study task's model, so decks written
//! ahead wait their turn behind Explain; dropping a job frees its slot.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use mareader_settings::ModelPreset;
use mareader_study::mode::{Answer, CardJob, CardWriter, Wanted};
use mareader_study::{Failure, Why};
use tokio::task::JoinHandle;

use super::{Assist, Engine, Waker, short_slot};
use crate::backend::{Backend, Nobody, run};
use crate::slots::Slots;
use crate::study_mode::StudyCards;

impl Assist {
    /// The writer of study mode's cards, on the study task's model.
    pub fn cards(&self) -> Arc<dyn CardWriter> {
        let preset = self.ai.study_preset().cloned();
        let backend = match &preset {
            Some(p) => self.backend(p),
            None => Arc::new(Nobody(Why::NotHere.into())),
        };
        Arc::new(AiCards {
            engine: Arc::clone(&self.engine),
            slots: self.slots.clone(),
            waker: Arc::clone(&self.waker),
            by: preset.as_ref().map(ModelPreset::title).unwrap_or_default(),
            preset,
            backend,
            language: self.ai.study_language(),
        })
    }
}

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
        let (tx, rx) = channel();
        let Some(rt) = self.engine.rt() else {
            let _ = tx.send(Err(Why::NotHere.into()));
            return Box::new(Writing::new(rx, None));
        };
        let (slots, waker) = (self.slots.clone(), Arc::clone(&self.waker));
        let (preset, backend) = (self.preset.clone(), Arc::clone(&self.backend));
        let language = self.language;
        let task = rt.spawn(async move {
            let model = preset.as_ref();
            let answer = write_deck(&slots, model, backend.as_ref(), &words, language).await;
            let _ = tx.send(answer);
            waker();
        });
        Box::new(Writing::new(rx, Some(task)))
    }
}

/// One deck's backs, once the model has a slot free.
async fn write_deck(
    slots: &Slots,
    preset: Option<&ModelPreset>,
    backend: &dyn Backend,
    words: &[Wanted],
    language: Option<&str>,
) -> Answer {
    let Some(preset) = preset else {
        let none = "no model is chosen for study; pick one in Settings › AI";
        return Err(Failure::detailed(Why::NotHere, none));
    };
    let _ticket = short_slot(slots, preset, backend).await?;
    let task = StudyCards { words, language };
    run(backend, &task, &|_| {}).await
}

/// A deck being written; dropping it cancels it and frees its model.
struct Writing {
    rx: Receiver<Answer>,
    task: Option<JoinHandle<()>>,
    answered: bool,
}

impl Writing {
    fn new(rx: Receiver<Answer>, task: Option<JoinHandle<()>>) -> Self {
        Self {
            rx,
            task,
            answered: false,
        }
    }
}

impl CardJob for Writing {
    fn take(&mut self) -> Option<Answer> {
        if self.answered {
            return None;
        }
        let answer = match self.rx.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(Why::NotHere.into()),
        };
        self.answered = true;
        Some(answer)
    }
}

impl Drop for Writing {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
