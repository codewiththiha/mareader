//! The floating system's DOM adapters: placement glue, shared dismissal
//! mechanics and the element/geometry helpers.
//!
//! The pure placement math stays in `ui_geom::floating` (host-testable);
//! these modules adapt it to living DOM nodes, viewport reads and the
//! app's Escape/outside-press contract. The anchored surfaces built on top
//! (popover, context menu, floating card) live in the app under
//! `primitives::floating`, because they are views; the hooks and adapters
//! are chrome and reusable, so they live here.

pub mod dismiss;
pub mod position;
pub mod types;
