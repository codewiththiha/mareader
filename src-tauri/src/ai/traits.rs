//! The wire protocol between the backend and the frontend: chunks and errors.

use futures::Stream;
use std::pin::Pin;

use super::schema::WordInfo;

/// Machine-readable cause; branch on this, never on the message wording.
    not(all(feature = "ai", target_os = "macos", target_arch = "aarch64")),
    allow(dead_code)
)]
pub enum AiErrorKind {
    /// Apple Intelligence is off in System Settings; the user can fix it.
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

/// A typed error serialized across the wire, so the frontend retries rightly.
pub struct AiError {
    /// Machine-readable cause — the frontend's branch point.
    pub kind: AiErrorKind,
    /// Human-readable detail; for `Other` this is shown directly.
    pub message: String,
    /// Mirrors `fm_bridge::Error::is_retryable` (plus schema-shape faults).
    pub retryable: bool,
}

/// The chunks of data we will stream to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunk {
    Snapshot(WordInfo),
    Done,
    /// The run failed; carries a typed, retryable-aware error.
    Error(AiError),
}

/// One `ai-stream-chunk`: a chunk plus the id of the run that produced it.
pub struct AiStreamEvent {
    /// The run id passed to `explain_word`.
    pub run: String,
    pub chunk: AiChunk,
}

/// The trait every AI provider implements, over a pinned boxed stream.
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

    /// The envelope keeps the chunk under `chunk`, with the run id beside it.
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
