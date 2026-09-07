//! AI provider wiring for the in-reader dictionary/assistant features.
//!
//! The backend lives entirely in this Tauri crate: the WASM frontend cannot
//! spawn the Swift helper process, so it sends selected text over `invoke`
//! and receives [`AiChunk`] events back. Which provider runs is a build-time
//! choice: Apple Silicon + the `ai` feature get the real Apple Intelligence
//! provider (Foundation Models via `fm-bridge`); everything else — Windows,
//! Linux, Intel Macs, `ai` disabled — gets a mock provider streaming canned
//! data, so the UI stays testable.

// Only the Apple Silicon provider consumes the prompt text, so it is gated with
// it: on every other target the prompts module is not compiled at all.
#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub mod prompts;

pub mod schema;
pub mod traits;

// Compiled on every target: it is the build-time fallback for unsupported
// platforms AND the runtime fallback when the Swift bridge fails to start.
pub mod mock;

// Apple Silicon Implementation
#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub mod apple;

// The frontend-facing surface: the chunk stream and its error vocabulary.
// `WordInfo` reaches commands inside an `AiChunk`, so it is not re-exported.
pub use traits::{AiChunk, AiError, AiErrorKind, AiProvider, AiStreamEvent};

#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub fn create_provider() -> Box<dyn AiProvider> {
    match apple::AppleAiProvider::new() {
        Ok(provider) => Box::new(provider),
        Err(e) => {
            eprintln!("Failed to init Apple AI, falling back to Mock: {}", e);
            Box::new(mock::MockAiProvider::new())
        }
    }
}

// Windows, Linux, Intel Macs, or the `ai` feature disabled.
#[cfg(not(all(feature = "ai", target_os = "macos", target_arch = "aarch64")))]
pub fn create_provider() -> Box<dyn AiProvider> {
    Box::new(mock::MockAiProvider::new())
}
