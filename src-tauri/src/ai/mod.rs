//! AI provider wiring: Apple Intelligence when available, a mock otherwise.

// Only the Apple provider reads prompts, so the module is gated with it.
pub mod prompts;

pub mod schema;
pub mod traits;

// Compiled on every target: the fallback for unsupported platforms.
pub mod mock;

#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub mod apple;

// The frontend-facing surface: the chunk stream and its error vocabulary.
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
