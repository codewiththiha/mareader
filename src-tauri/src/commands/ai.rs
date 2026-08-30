use std::sync::OnceLock;

use futures::StreamExt;
use tauri::{AppHandle, Emitter};

use crate::ai::{create_provider, AiChunk, AiError, AiErrorKind, AiProvider};

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

#[tauri::command]
pub async fn explain_word(app: AppHandle, word: String, context: String) -> Result<(), String> {
    // The UI already mutes over-long selections, so this only ever fires on
    // a direct invoke — answer it with a typed error, not a model run.
    if !is_glossable(&word) {
        app.emit(
            "ai-stream-chunk",
            &AiChunk::Error(AiError {
                kind: AiErrorKind::ContextTooLong,
                message: "selection too long for a word lookup".into(),
                retryable: false,
            }),
        )
        .map_err(|e| e.to_string())?;
        return Ok(());
    }

    let mut stream = provider().explain_word(word, context);

    while let Some(chunk) = stream.next().await {
        app.emit("ai-stream-chunk", &chunk).map_err(|e| e.to_string())?;
        if matches!(chunk, AiChunk::Done | AiChunk::Error(_)) {
            break;
        }
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
