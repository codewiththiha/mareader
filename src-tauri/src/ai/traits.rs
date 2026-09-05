//! The wire protocol between the Tauri backend and the frontend: the chunk
//! stream ([`AiChunk`]) and the typed error it can carry ([`AiError`]).
//!
//! The frontend's half of the contract is `crates/ai-core/src/types.rs` for
//! the error and the word payload — the app imports that crate, so there is
//! nothing to keep in step — and `src/services/ai.rs` for the chunk envelope it
//! deserializes off the Tauri event. Keep THAT one's serde shape in sync; the
//! test below pins this crate's half.

use futures::Stream;
use std::pin::Pin;

use super::schema::WordInfo;

/// Machine-readable cause of an [`AiError`]. Branch on this, never on the
/// human-facing `message`, whose wording may change between OS releases.
///
/// Serializes flat — unit variants as `"snake_case"` strings — so the wire
/// shape stays `{"kind":"model_not_ready"}` rather than nesting tags.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    not(all(feature = "ai", target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
pub enum AiErrorKind {
    /// Apple Intelligence is switched off in System Settings; the user can fix it.
    NotEnabled,
    /// Model assets are still downloading/preparing; transient.
    ModelNotReady,
    /// The hardware can never run Apple Intelligence.
    DeviceNotEligible,
    /// macOS too old / Foundation Models absent.
    OsTooOld,
    /// Safety guardrails blocked the request or the response.
    BlockedByGuardrail,
    /// Prompt + response exceeded the context window.
    ContextTooLong,
    /// The request exceeded the bridge timeout (queue wait or generation).
    Timeout,
    /// The model is already responding to another request.
    Busy,
    /// The response did not match the WordInfo schema.
    BadResponse,
    /// Anything else; carries a short user-facing summary.
    Other(String),
}

/// A typed error from the AI pipeline, serialized across the wire so the
/// frontend can show the right message and a retry affordance exactly when
/// retrying might help.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct AiError {
    /// Machine-readable cause — the frontend's branch point.
    pub kind: AiErrorKind,
    /// Human-readable detail. For [`AiErrorKind::Other`] this is written to
    /// be shown directly (bounded in length); for the named kinds it is the
    /// full provider/bridge text, kept for logs and devtools.
    pub message: String,
    /// Mirrors `fm_bridge::Error::is_retryable` (plus schema-shape faults).
    pub retryable: bool,
}

/// The chunks of data we will stream to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunk {
    /// A partial or final snapshot of the WordInfo
    Snapshot(WordInfo),
    /// The stream is complete
    Done,
    /// The run failed; carries a typed, retryable-aware error.
    Error(AiError),
}

/// What actually goes over the `ai-stream-chunk` event: a chunk plus the id
/// of the run that produced it.
///
/// The frontend can have more than one run in flight — a reader who glosses a
/// second word before the first answer lands — and every run emits on the same
/// event name. Without the id, a late chunk from the abandoned run is
/// indistinguishable from the live one's, so it gets rendered (and cached)
/// against the wrong word. The id is chosen by the caller and echoed verbatim;
/// the backend never interprets it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AiStreamEvent {
    /// The run id passed to `explain_word`.
    pub run: String,
    pub chunk: AiChunk,
}

/// The trait that all AI providers must implement.
/// We use a pinned boxed stream to allow async streaming without lifetime hell.
pub trait AiProvider: Send + Sync {
    fn explain_word(
        &self,
        word: String,
        context: String,
    ) -> Pin<Box<dyn Stream<Item = AiChunk> + Send>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The frontend's `AiChunkEvent` mirror must parse exactly these shapes.
    #[test]
    fn error_chunk_serializes_with_a_flat_kind() {
        let chunk = AiChunk::Error(AiError {
            kind: AiErrorKind::ModelNotReady,
            message: "the on-device model is still downloading".into(),
            retryable: true,
        });
        let json = serde_json::to_value(&chunk).unwrap();
        assert_eq!(json["type"], "Error");
        assert_eq!(json["data"]["kind"], "model_not_ready");
        assert_eq!(json["data"]["retryable"], true);

        // The escape-hatch variant carries its summary inline.
        let other = AiChunk::Error(AiError {
            kind: AiErrorKind::Other("helper crashed".into()),
            message: "helper crashed".into(),
            retryable: false,
        });
        let json = serde_json::to_value(&other).unwrap();
        assert_eq!(json["data"]["kind"]["other"], "helper crashed");
    }

    /// The envelope must keep the chunk's own shape intact under `chunk` and
    /// carry the run id beside it — the frontend gate reads `run` and then
    /// parses `chunk` with the mirror asserted above.
    #[test]
    fn the_envelope_carries_the_run_id_beside_the_chunk() {
        let event = AiStreamEvent {
            run: "g3-1712#4".into(),
            chunk: AiChunk::Done,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["run"], "g3-1712#4");
        assert_eq!(json["chunk"]["type"], "Done");
    }
}
