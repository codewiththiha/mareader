//! Gloss word-card: a spring-driven morph from an in-page highlighter stroke
//! into an explanation card and back down onto it, ported from the Gloss
//! reference (with the Marginalia highlighter look for the mark itself).
//!
//! Layout:
//! * The geometry + spring math (pure) lives in `ai_core::gloss`, including
//!   the side-aware card placement ([`ai_core::gloss::place_card`]).
//! * Page-aware anchors live in [`crate::components::ai::anchor`] (shared
//!   with the selection Info pill).
//! * [`controller`]     — the state machine hub: grouped state slices
//!   (content / geometry / open / drag / cache), the shared commands,
//!   and the open path as a named verdict + transitions.
//! * [`placement`]      — the card's target memos (expanded box + drag offset).
//! * [`targeting`]      — the targeting bundle: anchor watch, viewport,
//!   measurement, spring, progress.
//! * [`drag`]           — pointer physics for dragging the expanded card
//!   (thin domain wrapper over the primitive drag mechanics).
//! * [`interactions`]   — window-level behaviour (Escape/outside, exit, flip).
//! * [`hooks`]          — chunk ingestion (measurement is the generic hook).
//! * [`selection_mode`] — multi-select mode: entry guards, exit paths, the
//!   context-menu listener, the undo pipeline.
//! * [`mark_layer`]     — the persistent highlighter stroke layer per page
//!   (incl. the long-press gesture + contextmenu).
//! * [`selection_bar`]  — the bottom-right selection action bar.
//! * [`context_menu`]   — the right-click "Remove highlight" menu.
//! * [`undo_toast`]     — the "Removed n highlights — Undo" toast.
//! * [`gloss_surface`]  — the morphing surface component (composing the
//!   primitive `FloatingCard` with the gloss phase styling).
//! * [`word_info`]      — the card's body: the AI answer sections at the
//!   chosen density (also rendered headless by the measure twin).
//! * [`gloss_ai_popover`] — wiring + view (the composition root).
//!
//! Generic mechanics (viewport, reduced motion, spring, drag, long press,
//! dismissal, measurement, shimmer, context-menu/toast shells) live in
//! `crate::components::primitives`; this module keeps only the policy.

pub mod context_menu;
pub mod controller;
pub mod drag;
pub mod gloss_ai_popover;
pub mod gloss_surface;
pub mod hooks;
pub mod interactions;
pub mod mark_layer;
pub mod placement;
pub mod selection_bar;
pub mod selection_mode;
pub mod targeting;
pub mod undo_toast;
pub mod word_info;
