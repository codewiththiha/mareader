//! The format-agnostic core of the AI reading features.
//!
//! Everything here is independent of any document format: the wire types of
//! the word-explanation backend ([`types`]), the gloss card's geometry and
//! spring ([`gloss`], whose `geometry::step_spring` steps `ui_geom::spring`),
//! and the Tauri `explain_word` kickoff ([`bridge`]).
//!
//! What this crate DOES depend on is `reader-core`, and only for what the
//! reader owns rather than the AI: the word card's *settings* are flat `gloss_*`
//! fields of the persisted `Settings` blob (`reader_core::settings`, so
//! `GlossColor` and `GlossDensity` live there — a crate whose types the schema
//! names cannot also own part of the schema). The card's spring comes from
//! `ui-geom`, the dependency-free leaf the floating panels step too, which is
//! what keeps the two surfaces feeling identical without this crate and the
//! chrome crate depending on each other. Both directions point the same way:
//! features may lean on the general crates, never the reverse.
//!
//! The dependency rule is one-way: format crates (pdf-core, the app) depend
//! on this crate, never the reverse — so a new format reuses the wire protocol,
//! the card, the mark schema and the springs without touching anything here.
//!
//! What a new format DOES have to decide is where its mark's identity lives, and
//! the two formats so far answered differently. A PDF's spot is durable pixels
//! (a page and a rect), so it is the mark's flattened `anchor`
//! ([`gloss::mark::PageAnchor`]). A reflowable document is re-cut whenever the
//! typography moves, so its spot is a block index and a character range, and it
//! travels in `GlossMark::context` as a tagged envelope the app owns
//! (`components::ai::reflow_anchor`) — pixels are re-derived at watch time rather
//! than stored. Implementing [`gloss::mark::MarkAnchor`] for a new anchor type is
//! only the right move when the identity is as durable as a rect is.
//!
//! The pure modules are unit-testable on the host via
//! `cargo test -p ai-core`.

pub mod bridge;
pub mod gloss;
pub mod types;
