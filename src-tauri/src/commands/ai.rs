use std::sync::OnceLock;

use futures::StreamExt;
use tauri::{AppHandle, Emitter};

use ai_core::gloss::is_glossable;

use crate::ai::{AiChunk, AiError, AiErrorKind, AiProvider, AiStreamEvent, create_provider};

/// One provider for the process's lifetime; the env read happens once.
static PROVIDER: OnceLock<Box<dyn AiProvider>> = OnceLock::new();

fn provider() -> &'static dyn AiProvider {
    PROVIDER.get_or_init(create_provider).as_ref()
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

/// Start a streaming explanation for `word`; `run` is echoed on every chunk.
#[tauri::command]
pub async fn explain_word(
    app: AppHandle,
    word: String,
    context: String,
    run: String,
) -> Result<(), String> {
    // The UI mutes over-long selections, so this fires only on a direct invoke.
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

    // Coalesce Snapshot chunks: flush after 4 or 64 ms; Done flushes now.
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
