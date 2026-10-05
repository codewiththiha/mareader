//! Which mark the card belongs to, which one is queued, and which backend
//! run is still allowed to answer into it.

use ai_core::gloss::GlossMark;
use leptos::prelude::*;

/// The open plumbing: which persisted mark the card belongs to, the mark
/// queued by the latest request, and the request nonce that re-runs the
/// open effect even when the popover is already open.
#[derive(Clone, Copy)]
pub struct GlossOpen {
    /// The mark the open card belongs to (None while closed).
    pub mark: RwSignal<Option<GlossMark>>,
    /// The mark queued by the most recent open request, consumed by the
    /// open effect.
    pub pending: RwSignal<Option<GlossMark>>,
    /// Monotonic request counter — tracking it is what makes a second open
    /// of an already-open popover re-run the open effect.
    pub request: RwSignal<u64>,
    /// The backend run whose chunks this card is still willing to accept, or
    /// `None` when nothing is in flight. See [`GlossOpen::begin_run`].
    active_run: RwSignal<Option<String>>,
    /// Monotonic run counter. The mark id alone cannot identify a run: a
    /// retry after a failure is a second run on the SAME mark, and the first
    /// one's late error would otherwise tear down the retry.
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

    /// Adopt a fresh backend run for `mark_id` and return its wire id.
    ///
    /// The backend echoes this id on every chunk. Runs are never cancelled —
    /// the model is already working — so the id is how a superseded run's
    /// answer is told apart from the live one's, which is what stops a slow
    /// answer for one word from landing on (and being cached under) the word
    /// the reader glossed next.
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

    /// Stop accepting chunks: the run finished, failed, or was abandoned
    /// (a different mark opened, the card was dismissed).
    pub fn end_run(&self) {
        if self.active_run.get_untracked().is_some() {
            self.active_run.set(None);
        }
    }
}
