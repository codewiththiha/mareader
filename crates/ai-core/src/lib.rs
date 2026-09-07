//! The format-agnostic core of the AI reading features: the wire types of the
//! word-explanation backend ([`types`]), the gloss card's geometry and spring
//! ([`gloss`], stepping `ui_geom::spring`), and the Tauri `explain_word`
//! kickoff ([`bridge`]).
//!
//! Its one dependency is `reader-core`, and only for what the reader owns:
//! the word card's *settings* are flat `gloss_*` fields of the persisted
//! `Settings` blob, so `GlossColor` and `GlossDensity` live there. The card's
//! spring comes from `ui-geom` — the same leaf the floating panels step,
//! which keeps the two surfaces feeling identical without this crate and the
//! chrome crate depending on each other.
//!
//! The dependency rule is one-way: format crates (pdf-core, the app) depend
//! on this crate, never the reverse, so a new format reuses the wire
//! protocol, the card, the mark schema and the springs untouched.
//!
//! What a new format DOES decide is where its mark's identity lives. A PDF's
//! spot is durable pixels (page + rect), so it is the mark's flattened
//! `anchor` ([`gloss::mark::PageAnchor`]). A reflowable document is re-cut
//! whenever the typography moves, so its spot is a block index and a
//! character range travelling in `GlossMark::context` as a tagged envelope
//! the app owns (`components::ai::reflow_anchor`) — pixels re-derived at
//! watch time, never stored. Implementing [`gloss::mark::MarkAnchor`] is only
//! the right move when the identity is as durable as a rect.
//!
//! Pure modules; `cargo test -p ai-core` on the host.

pub mod bridge;
pub mod gloss;
pub mod types;
