//! Fallback provider: canned data so the UI stays testable on Windows,
//! Linux, Intel Macs and builds without the `ai` feature — and whenever the
//! Swift bridge fails to start.
//!
//! The `__fail` word is a deterministic error path: look it up to exercise
//! the error card and the retry affordance on a machine without Apple
//! Intelligence.

use async_stream::stream;
use futures::Stream;
use std::pin::Pin;
use std::time::Duration;

use super::{
    schema::WordInfo,
    traits::{AiChunk, AiError, AiErrorKind, AiProvider},
};

/// Selecting this word yields a retryable error instead of an answer.
pub const FAIL_WORD: &str = "__fail";

pub struct MockAiProvider;

impl MockAiProvider {
    pub fn new() -> Self {
        Self
    }
}

impl AiProvider for MockAiProvider {
    fn explain_word(
        &self,
        word: String,
        _context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>> {
        Box::pin(stream! {
            // Deterministic error path for UI work on fallback builds.
            if word == FAIL_WORD {
                tokio::time::sleep(Duration::from_millis(400)).await;
                yield AiChunk::Error(AiError {
                    kind: AiErrorKind::ModelNotReady,
                    message: "mock: the on-device model is still downloading".into(),
                    retryable: true,
                });
                return;
            }

            // A short "thinking" beat so the stroke's processing pulse is
            // still visible, then stream the answer out bit by bit the way
            // the real provider does: the card opens on the FIRST content
            // chunk (the meaning), and the rest patches in as it lands —
            // the exercise for growing / streaming, not a single late pop.
            tokio::time::sleep(Duration::from_millis(320)).await;

            let meaning = format!("(Mock) A simulated, simplified meaning for the word '{}'.", word);

            yield AiChunk::Snapshot(WordInfo {
                pos: "noun".to_string(),
                meaning: meaning.clone(),
                synonyms: Vec::new(),
                usages: Vec::new(),
            });

            tokio::time::sleep(Duration::from_millis(180)).await;

            yield AiChunk::Snapshot(WordInfo {
                pos: "noun".to_string(),
                meaning: meaning.clone(),
                synonyms: vec!["mock-synonym-1".to_string(), "mock-synonym-2".to_string()],
                usages: Vec::new(),
            });

            tokio::time::sleep(Duration::from_millis(180)).await;

            yield AiChunk::Snapshot(WordInfo {
                pos: "noun".to_string(),
                meaning: meaning.clone(),
                synonyms: vec!["mock-synonym-1".to_string(), "mock-synonym-2".to_string()],
                usages: vec![
                    format!("This is the first mock example using the word {}.", word),
                    format!("Here is another context where {} is used.", word),
                ],
            });

            // A short settle before calling it done.
            tokio::time::sleep(Duration::from_millis(120)).await;

            yield AiChunk::Done;
        })
    }
}
