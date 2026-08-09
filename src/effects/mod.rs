//! Reactive effects that translate state changes into DOM/engine side effects.
//!
//! Branch ownership (foundation creates stubs; branches fill them):
//!   - theme_applier        foundation (complete)
//!   - continuous_scroll    branch A (viewer/continuous)
//!   - shortcuts            branch B (viewer/chrome)
//!   - search_effects       branch C (panels/sidebar)
//!   - theme_ui             branch D (panels/settings)
//!
//! mod.rs is FROZEN after the foundation commit.

pub mod continuous_scroll;
pub mod fit;
pub mod page_tracking;
pub mod search_effects;
pub mod shortcuts;
pub mod theme_applier;
pub mod theme_ui;
