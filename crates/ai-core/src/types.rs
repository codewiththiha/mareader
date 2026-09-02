//! The wire types of the word-explanation feature.
//!
//! These mirror the shapes in `src-tauri/src/ai/traits.rs` (the backend's
//! `WordInfo` / `AiError` / `AiChunk` / `AiStreamEvent`) — keep the serde
//! shapes in sync. They are format-agnostic: the backend answers "what does
//! this word mean", never "where is it in the document" (that is a
//! [`crate::gloss::mark::MarkAnchor`], owned by the format layer).

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// The structured word information returned by the AI.
/// Mirrors the backend `WordInfo` struct.
///
/// `Serialize` is required so the app-lifetime AI chunk bridge can
/// re-broadcast snapshots as a window `CustomEvent` for the popover.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WordInfo {
    pub pos: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}

/// Bounds on a single answer, applied at ingestion. A well-behaved model
/// answers in a sentence or two; these are the sizes past which a response is
/// no longer an answer but a runaway generation, and the card holds it for the
/// rest of the session (every mark's answer stays cached until its mark is
/// removed). Clipping at the door keeps that ceiling flat instead of letting
/// one pathological run set it.
const MAX_MEANING_CHARS: usize = 1_200;
const MAX_SYNONYMS: usize = 16;
const MAX_SYNONYM_CHARS: usize = 80;
const MAX_USAGES: usize = 8;
const MAX_USAGE_CHARS: usize = 400;

/// The ellipsis that marks a clipped string, so a truncated answer reads as
/// truncated rather than as a model that stopped mid-word.
const ELLIPSIS: char = '\u{2026}';

/// Clip `s` to at most `max` characters (not bytes — this is human text and
/// the boundary must be a char boundary), marking the cut.
fn clamp_text(s: String, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s,
        Some((byte, _)) => {
            let mut clipped = s;
            clipped.truncate(byte);
            clipped.push(ELLIPSIS);
            clipped
        }
    }
}

impl WordInfo {
    /// The answer bounded to the sizes above. Applied once, where snapshots
    /// enter the app, so nothing downstream — the card, the measure twin, the
    /// session cache — has to reason about how big an answer can be.
    pub fn clamped(self) -> Self {
        Self {
            pos: clamp_text(self.pos, MAX_SYNONYM_CHARS),
            meaning: clamp_text(self.meaning, MAX_MEANING_CHARS),
            synonyms: self
                .synonyms
                .into_iter()
                .take(MAX_SYNONYMS)
                .map(|s| clamp_text(s, MAX_SYNONYM_CHARS))
                .collect(),
            usages: self
                .usages
                .into_iter()
                .take(MAX_USAGES)
                .map(|u| clamp_text(u, MAX_USAGE_CHARS))
                .collect(),
        }
    }
}

/// Mirror of the backend's `AiErrorKind` (`src-tauri/src/ai/traits.rs`) —
/// keep the serde shapes in sync. Branch on `kind`, never on `message`
/// wording: the prose may change between OS releases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiErrorKind {
    NotEnabled,
    ModelNotReady,
    DeviceNotEligible,
    OsTooOld,
    BlockedByGuardrail,
    ContextTooLong,
    Timeout,
    Busy,
    BadResponse,
    Other(String),
}

/// Mirror of the backend's `AiError`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiError {
    pub kind: AiErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl AiError {
    /// Short, human explanation mapped once per kind so the wording stays
    /// consistent everywhere it is shown. [`AiErrorKind::Other`] is the
    /// escape hatch whose `message` was written to be user-facing (and is
    /// bounded in length by whoever constructs it).
    pub fn friendly(&self) -> Cow<'_, str> {
        match &self.kind {
            AiErrorKind::NotEnabled => {
                "Apple Intelligence is turned off. Turn it on in System Settings → Apple Intelligence."
                    .into()
            }
            AiErrorKind::ModelNotReady => {
                "The on-device model is still downloading. Try again in a little while."
                    .into()
            }
            AiErrorKind::DeviceNotEligible => "This Mac doesn't support Apple Intelligence.".into(),
            AiErrorKind::OsTooOld => "macOS 26 or newer is required for on-device explanations.".into(),
            AiErrorKind::BlockedByGuardrail => "The model declined to answer this one.".into(),
            AiErrorKind::ContextTooLong => "The selected passage is too long to analyze.".into(),
            AiErrorKind::Timeout => "The model took too long to respond.".into(),
            AiErrorKind::Busy => "The model is busy with another request right now.".into(),
            AiErrorKind::BadResponse => "The response didn't match the expected format.".into(),
            AiErrorKind::Other(_) => self.message.as_str().into(),
        }
    }

    /// Fallback for render paths that must show *something* even if the
    /// error signal was (impossibly) cleared between phase and paint.
    pub fn unknown() -> Self {
        Self {
            kind: AiErrorKind::Other("unknown".into()),
            message: "Something went wrong.".into(),
            retryable: false,
        }
    }
}

/// One chunk of an explanation. Mirrors `AiChunk` in
/// `src-tauri/src/ai/traits.rs` (same `type`/`data` tagging) — keep in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunk {
    Snapshot(WordInfo),
    Done,
    /// A typed failure; see [`AiError`] for the cause/retry contract.
    Error(AiError),
}

/// The wire format of one `ai-stream-chunk` payload: a chunk plus the id of
/// the run that produced it. Mirrors `AiStreamEvent` in
/// `src-tauri/src/ai/traits.rs` — keep in sync.
///
/// The run id is what makes concurrent glosses safe. Runs are never cancelled
/// backend-side, so a reader who glosses a second word while the first is
/// still thinking has two runs emitting on one event name; without the id the
/// abandoned run's answer would be rendered against — and cached under — the
/// word that is on screen now.
///
/// `Serialize` lets the app's chunk bridge park the same shape on a window
/// CustomEvent detail so per-mount UI can subscribe without touching Tauri
/// again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChunkEvent {
    /// The id passed to the `explain_word` invoke, echoed by the backend.
    pub run: String,
    pub chunk: AiChunk,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_answer_passes_through_untouched() {
        let info = WordInfo {
            pos: "adjective".into(),
            meaning: "lasting for a very short time".into(),
            synonyms: vec!["fleeting".into(), "transient".into()],
            usages: vec!["ephemeral pleasures".into()],
        };
        assert_eq!(info.clone().clamped(), info);
    }

    #[test]
    fn a_runaway_answer_is_clipped_at_every_axis() {
        let clamped = WordInfo {
            pos: "noun".into(),
            meaning: "x".repeat(MAX_MEANING_CHARS + 500),
            synonyms: (0..MAX_SYNONYMS + 10).map(|_| "y".repeat(200)).collect(),
            usages: (0..MAX_USAGES + 5).map(|_| "z".repeat(1_000)).collect(),
        }
        .clamped();

        assert_eq!(clamped.meaning.chars().count(), MAX_MEANING_CHARS + 1);
        assert!(clamped.meaning.ends_with(ELLIPSIS));
        assert_eq!(clamped.synonyms.len(), MAX_SYNONYMS);
        assert_eq!(clamped.synonyms[0].chars().count(), MAX_SYNONYM_CHARS + 1);
        assert_eq!(clamped.usages.len(), MAX_USAGES);
        assert_eq!(clamped.usages[0].chars().count(), MAX_USAGE_CHARS + 1);
    }

    #[test]
    fn clipping_lands_on_a_char_boundary() {
        // Multi-byte input: truncating by bytes here would panic.
        let text: String = "é".repeat(MAX_MEANING_CHARS + 20);
        let clipped = clamp_text(text, MAX_MEANING_CHARS);
        assert_eq!(clipped.chars().count(), MAX_MEANING_CHARS + 1);
    }

    #[test]
    fn a_string_exactly_at_the_bound_is_not_marked() {
        let text = "a".repeat(MAX_MEANING_CHARS);
        assert_eq!(clamp_text(text.clone(), MAX_MEANING_CHARS), text);
    }

    // The exact JSON shapes the backend's `app.emit("ai-stream-chunk",
    // &chunk)` produces from `AiChunk`'s serde tagging. If these parse, the
    // enum mirror above is wire-compatible.
    #[test]
    fn chunk_wire_shapes_parse() {
        let snapshot: AiChunk = serde_json::from_str(
            r#"{"type":"Snapshot","data":{"pos":"noun","meaning":"lasting briefly","synonyms":["fleeting"],"usages":["ephemeral beauty"]}}"#,
        )
        .unwrap();
        match snapshot {
            AiChunk::Snapshot(info) => {
                assert_eq!(info.pos, "noun");
                assert_eq!(info.synonyms, vec!["fleeting"]);
            }
            other => panic!("expected Snapshot, got {other:?}"),
        }

        let done: AiChunk = serde_json::from_str(r#"{"type":"Done"}"#).unwrap();
        assert!(matches!(done, AiChunk::Done));

        let error: AiChunk = serde_json::from_str(
            r#"{"type":"Error","data":{"kind":"model_not_ready","message":"the on-device model is still downloading","retryable":true}}"#,
        )
        .unwrap();
        match error {
            AiChunk::Error(err) => {
                assert_eq!(err.kind, AiErrorKind::ModelNotReady);
                assert!(err.retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }

        // The escape-hatch variant carries its summary inline.
        let other: AiChunk = serde_json::from_str(
            r#"{"type":"Error","data":{"kind":{"other":"helper crashed"},"message":"helper crashed","retryable":false}}"#,
        )
        .unwrap();
        match other {
            AiChunk::Error(err) => {
                assert_eq!(err.kind, AiErrorKind::Other("helper crashed".into()));
                assert!(!err.retryable);
            }
            other => panic!("expected Error(Other), got {other:?}"),
        }
    }

    /// The envelope the backend actually emits: the chunk nested under
    /// `chunk`, the run id beside it. If this drifts, every chunk is dropped
    /// by the listener's run gate and the card never opens.
    #[test]
    fn the_envelope_carries_the_run_id() {
        let event: AiChunkEvent = serde_json::from_str(
            r#"{"run":"g3-1712#4","chunk":{"type":"Snapshot","data":{"pos":"adj","meaning":"short-lived","synonyms":[],"usages":[]}}}"#,
        )
        .unwrap();
        assert_eq!(event.run, "g3-1712#4");
        match event.chunk {
            AiChunk::Snapshot(info) => assert_eq!(info.meaning, "short-lived"),
            other => panic!("expected Snapshot, got {other:?}"),
        }
    }
}
