//! Which mark the card belongs to, and which run may answer.

use ai_core::gloss::GlossMark;
use leptos::prelude::*;

/// The open plumbing: the mark, the queue, the request nonce.
#[derive(Clone, Copy)]
pub struct GlossOpen {
    /// The mark the open card belongs to (None while closed).
    pub mark: RwSignal<Option<GlossMark>>,
    /// The mark queued by the most recent open request, consumed by the
    /// open effect.
    pub pending: RwSignal<Option<GlossMark>>,
    /// Request counter; a repeat open re-runs the effect.
    pub request: RwSignal<u64>,
    /// The run whose chunks this card still accepts, or `None`.
    active_run: RwSignal<Option<String>>,
    /// Run counter: ids tell a superseded run's late error from the retry.
    run_seq: StoredValue<u64, LocalStorage>,
}

impl GlossOpen {
    pub(super) fn new() -> Self {
        Self {
            mark: RwSignal::new(None::<GlossMark>),
            pending: RwSignal::new(None::<GlossMark>),
            request: RwSignal::new(0u64),
            active_run: RwSignal::new(None::<String>),
            run_seq: StoredValue::new_local(0u64),
        }
    }

    /// Adopt a fresh run for `mark_id`, returning its wire id.
    pub fn begin_run(&self, mark_id: &str) -> String {
        let seq = self.run_seq.get_value().wrapping_add(1);
        self.run_seq.set_value(seq);
        let run = format!("{mark_id}#{seq}");
        self.active_run.set(Some(run.clone()));
        run
    }

    /// Whether `run` is the run this card is still waiting on.
    pub fn accepts(&self, run: &str) -> bool {
        self.active_run
            .with_untracked(|active| active.as_deref() == Some(run))
    }

    /// Stop accepting chunks: the run is done or abandoned.
    pub fn end_run(&self) {
        if self.active_run.get_untracked().is_some() {
            self.active_run.set(None);
        }
    }
}
