//! The geometry every floating surface shares, and nothing else.
//!
//! Two modules, both pure — no DOM, no leptos, no wasm, no knowledge of what a
//! document is:
//!
//!   * [`floating`] — placement and viewport clamping for an anchored panel:
//!     given an anchor rect and a panel size, which side does it go on, where
//!     exactly, and is the result inside the viewport.
//!   * [`spring`] — the damped integrator an animated box rides.
//!
//! This crate exists because both of those are needed by code that must not
//! depend on each other. The window chrome (`app-chrome`) places popovers and
//! menus; the AI feature (`ai-core`) steps the gloss card's box; the reader's
//! domain crate (`reader-core`) owns neither. Parking the shared maths in the
//! reader's core made the chrome crate depend on the reader's domain to position
//! a popover, and moving it into the chrome crate would have made a pure feature
//! crate depend on a DOM one. A leaf with no dependencies is the only place both
//! can reach.
//!
//! Everything here is unit-testable on the host: `cargo test -p ui-geom`.

pub mod floating;
pub mod spring;
