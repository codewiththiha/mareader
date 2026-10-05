//! The reader's feature surfaces that are neither host nor pane plumbing:
//! the rail (the sidebar's content tree, which the active pane fills into
//! the host's rail slot) and the virtualizer setup a pane's mount runs.
//! The workspace composition lives in `crate::host`; the per-pane effect
//! wiring in `crate::pane`.

pub mod rail;
pub mod virtualizers;
