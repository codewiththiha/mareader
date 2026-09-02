//! The format-agnostic core of the AI reading features.
//!
//! Everything here is independent of any document format: the wire types of
//! the word-explanation backend ([`types`]), the gloss card's geometry and
//! spring ([`gloss`], [`spring`]), the AI settings types ([`settings`]), and
//! the Tauri `explain_word` kickoff ([`bridge`]).
//!
//! The dependency rule is one-way: format crates (pdf-core, the app) depend
//! on this crate, never the reverse — so a future format (epub, plain text,
//! ...) reuses the wire protocol, the card, the spring and the settings
//! without touching anything here, and only implements
//! [`gloss::mark::MarkAnchor`] for its own notion of "where in the document".
//!
//! The pure modules are unit-testable on the host via
//! `cargo test -p ai-core`.

pub mod bridge;
pub mod gloss;
pub mod settings;
pub mod spring;
pub mod types;
