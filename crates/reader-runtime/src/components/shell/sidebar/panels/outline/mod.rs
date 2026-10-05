//! The outline panel: the document's headings, and the rail's wrapper around
//! them.
//!
//!   * `panel` — the reusable outline: row rendering, the reveal effect and
//!     the center-on-tab gesture.
//!   * `view`  — the rail's host: the absolutely-stacked wrapper with its
//!     paint/outro toggles.

pub mod panel;
pub mod view;

pub(crate) use panel::OutlinePanel;
