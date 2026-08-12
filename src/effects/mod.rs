//! Reactive effects that translate state changes into DOM/engine side effects.
//!
//! Branch ownership (foundation creates stubs; branches fill them):
//!   - theme_applier        foundation (complete)
//!   - continuous_scroll    branch A (viewer/continuous)
//!   - shortcuts            branch B (viewer/chrome)
//!   - search_effects       branch C (panels/sidebar)
//!
//! The FROZEN-mod.rs rule from the foundation commit was lifted for the
//! task-8 modularisation; new effects are added here as normal.

pub mod continuous_scroll;
pub mod fit;
pub mod link_nav;
pub mod page_tracking;
pub mod search_effects;
pub mod shortcuts;
pub mod theme_applier;
