//! `mareader-reader-app`: the reading surface as its own crate.
//!
//! The reader side of the split — its state slice ([`state`]), the viewer
//! and its page hosts ([`components`]), the reactive systems that keep them
//! in sync ([`effects`]), the zoom pipeline ([`zoom`]), the
//! DOM contract the host depends on ([`dom_contract`]), and the epoch gate
//! the shell defers to ([`epoch`]). The page assembly ([`features`]) is
//! here too, so Phase 2's build can mount the same surface the shell's
//! route mounts today.
//!
//! The boundary rule: this crate never imports the shell's `AppState` or
//! its services. What the reader needs FROM the shell arrives as narrowed
//! props or contexts — the settings snapshot, the library's covers, the
//! [`GlossSave`] persistence door — declared at the call site, checked by
//! the compiler, and invisible to everything below.

pub mod components;
pub mod dom_contract;
pub mod effects;
pub mod epoch;
pub mod features;
pub mod state;
pub mod zoom;

// The persistence door is named at the crate root because that is where both
// sides meet: the shell provides a [`state::GlossSave`] at the route, and the
// gloss controller below reads it back by this short path.
pub use state::GlossSave;
