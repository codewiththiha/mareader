//! Generic DOM/timer hooks: each owns one listener/effect family so
//! components read as wiring + view, and the raw pattern (closure parking,
//! cleanup ordering, JS-reference lifetimes) is written once.
//!
//! Contract: nothing here may know what a PDF reader is. These are the
//! layer-1 primitives the floating/interaction systems compose. The one
//! hook that is NOT format-agnostic — the typed CustomEvent hook, which
//! dispatches the app's own event protocol — stays in the app
//! (`primitives::hooks::use_custom_event`).
//!
//! Two composites live here as well. [`hover_reveal`] sits on top of
//! `use_timeout`'s hover primitive and owns the whole auto-hide surface —
//! the shared `hovered` truth, the hold recheck, the pin — so the title
//! bar, the bottom bar and the overlay rail wire one call each instead of
//! twenty copied lines. [`verified_switch`] owns the other half of "the
//! hide always lands": a switch driven by an async command re-checks the
//! live truth once the command resolves, so a decision that moved
//! mid-flight cannot leave the stale one settled last.

pub mod dom;
pub mod hover_reveal;
pub mod use_raf;
pub mod use_resize_observer;
pub mod use_timeout;
pub mod use_viewport;
pub mod use_window_event;
pub mod verified_switch;

// The auto-hide composite is re-exported flat: it is the entry point most
// callers want, and `hooks::use_hover_reveal` is the name they reach for.
pub use hover_reveal::{
    use_drag_hold, use_hover_reveal, use_hover_reveal_with, HoverConfig, HoverReveal,
    HoverRevealSurface, DEFAULT_HOVER_DELAY,
};
pub use verified_switch::{use_verified_switch, VerifiedSwitch};
