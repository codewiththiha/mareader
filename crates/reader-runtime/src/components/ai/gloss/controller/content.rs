//! What the model is doing, and what it has said so far.

use std::sync::Arc;

use ai_core::types::{AiError, WordInfo};
use leptos::prelude::*;

use crate::components::ai::gloss::phase::AiPhase;

/// The data phase and payload of the open card.
#[derive(Clone, Copy)]
pub struct GlossContent {
    pub phase: RwSignal<AiPhase>,
    pub word: RwSignal<String>,
    /// The answer, shared with the card, twin and cache.
    pub word_info: RwSignal<Option<Arc<WordInfo>>>,
    /// The typed failure behind `AiPhase::Error`, if any.
    pub error: RwSignal<Option<AiError>>,
}

impl GlossContent {
    pub(super) fn new() -> Self {
        Self {
            phase: RwSignal::new(AiPhase::Idle),
            word: RwSignal::new(String::new()),
            word_info: RwSignal::new(None::<Arc<WordInfo>>),
            error: RwSignal::new(None::<AiError>),
        }
    }

    /// Back to nothing: no word, no answer, no failure.
    pub(super) fn clear(&self) {
        self.phase.set(AiPhase::Idle);
        self.word.set(String::new());
        self.word_info.set(None);
        self.error.set(None);
    }
}
