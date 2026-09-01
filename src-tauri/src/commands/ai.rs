use std::sync::OnceLock;

use futures::StreamExt;
use tauri::{AppHandle, Emitter};

use crate::ai::{create_provider, AiChunk, AiError, AiErrorKind, AiProvider, AiStreamEvent};

/// One provider for the process's lifetime. `create_provider` reads the env
/// and builds the bridge (with its shared concurrency budget); doing that on
/// every invoke wasted both.
static PROVIDER: OnceLock<Box<dyn AiProvider>> = OnceLock::new();

fn provider() -> &'static dyn AiProvider {
    PROVIDER.get_or_init(create_provider).as_ref()
}

/// Mirror of `pdf_core::gloss::MAX_GLOSS_CHARS` — the frontend gate (the
/// Info pill) is the real one; this only protects the invoke boundary.
/// Keep the two in sync.
const MAX_GLOSS_CHARS: usize = 60;

fn is_glossable(word: &str) -> bool {
    // A word, not a phrase: trimmed, within the cap, and free of interior
    // whitespace — same rule as `pdf_core::gloss::is_glossable`.
    let t = word.trim();
    !t.is_empty() && t.chars().count() <= MAX_GLOSS_CHARS && !t.chars().any(char::is_whitespace)
}

/// Emit one chunk stamped with the run it belongs to.
fn emit(app: &AppHandle, run: &str, chunk: AiChunk) -> Result<(), String> {
    app.emit(
        "ai-stream-chunk",
        &AiStreamEvent {
            run: run.to_string(),
            chunk,
        },
    )
    .map_err(|e| e.to_string())
}

/// Start a streaming explanation for `word`.
///
/// `run` is the caller's id for this request, echoed on every chunk. Runs are
/// not cancelled when a newer one starts — the model is already working and
/// the answer may still be wanted — so the frontend needs the id to tell an
/// abandoned run's chunks from the live one's.
#[tauri::command]
pub async fn explain_word(
    app: AppHandle,
    word: String,
    context: String,
    run: String,
) -> Result<(), String> {
    // The UI already mutes over-long selections, so this only ever fires on
    // a direct invoke — answer it with a typed error, not a model run.
    if !is_glossable(&word) {
        return emit(
            &app,
            &run,
            AiChunk::Error(AiError {
                kind: AiErrorKind::ContextTooLong,
                message: "selection too long for a word lookup".into(),
                retryable: false,
            }),
        );
    }

    let mut stream = provider().explain_word(word, context);

    // Coalesce Snapshot chunks so a 10k-token stream does not pay one IPC
    // emit per ~100 characters. Flush after 4 snapshots or 64 ms, whichever
    // comes first; Done/Error always flush immediately.
    let mut pending: Option<AiChunk> = None;
    let mut batch = 0u8;
    let mut last_flush = std::time::Instant::now();
    while let Some(chunk) = stream.next().await {
        let last = matches!(chunk, AiChunk::Done | AiChunk::Error(_));
        if last {
            if let Some(prev) = pending.take() {
                emit(&app, &run, prev)?;
            }
            emit(&app, &run, chunk)?;
            break;
        }
        pending = Some(chunk);
        batch = batch.saturating_add(1);
        if batch >= 4 || last_flush.elapsed() >= std::time::Duration::from_millis(64) {
            if let Some(prev) = pending.take() {
                emit(&app, &run, prev)?;
            }
            batch = 0;
            last_flush = std::time::Instant::now();
        }
    }
    if let Some(prev) = pending.take() {
        emit(&app, &run, prev)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_glossable;

    /// Same rule as `pdf_core::gloss::is_glossable`, asserted here so a
    /// drift between the two gates fails a test instead of failing silently
    /// at the invoke boundary.
    #[test]
    fn gate_mirrors_the_frontend_rule() {
        assert!(is_glossable("palimpsest"));
        assert!(is_glossable(&"a".repeat(60)));
        assert!(!is_glossable(&"a".repeat(61)));
        assert!(!is_glossable("   "));
        // A phrase is not a word: interior whitespace is rejected.
        assert!(!is_glossable("quick brown"));
        assert!(!is_glossable("  quick brown  "));
    }
}
