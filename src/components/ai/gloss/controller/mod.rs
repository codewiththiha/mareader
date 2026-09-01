//! The gloss state-machine hub. The controller groups its twenty-odd signals
//! into cohesive slices — [`GlossContent`] (what the model is doing),
//! [`GlossGeometry`] (what the card's box is doing), [`GlossOpen`] (which
//! mark the card belongs to), [`GlossDrag`] (pointer state) and
//! [`GlossCache`] (session answers) — so a hook that needs one concern takes
//! one slice instead of the whole flat field list, and the shared behaviours
//! ([`GlossCommands`]: reset, collapse, mark persistence, retry) are clearly
//! commands rather than more state.
//!
//! One module per slice, plus [`commands`] (the behaviours over them) and
//! [`wiring`] (the open path: the request listener, the verdict that decides
//! what a request means, and the three transitions that act on it). This file
//! is the barrel: the cap, the aggregate, and how it is built.

pub mod cache;
pub mod commands;
pub mod content;
pub mod drag;
pub mod geometry;
pub mod open;
pub mod wiring;

pub use cache::GlossCache;
pub use commands::GlossCommands;
pub use content::GlossContent;
pub use drag::GlossDrag;
pub use geometry::GlossGeometry;
pub use open::GlossOpen;
pub use wiring::{use_open_effect, use_open_listener};

use crate::state::AppState;

/// Per-document cap on persisted marks (oldest evicted). A reading session's
/// worth of looked-up words, bounded so localStorage can't grow without end.
pub const MARK_CAP: usize = 200;

/// All gloss state + the shared behaviours, grouped into cohesive slices.
/// Every field is `Copy`, so hooks can take the controller — or just the
/// slice they need — by value without lifetime gymnastics.
#[derive(Clone, Copy)]
pub struct GlossController {
    pub content: GlossContent,
    pub geometry: GlossGeometry,
    pub open: GlossOpen,
    pub drag: GlossDrag,
    pub cache: GlossCache,
    pub commands: GlossCommands,
}

/// Build the controller: one set of slices, one set of commands over them.
pub fn use_gloss_controller(state: AppState) -> GlossController {
    let content = GlossContent::new();
    let geometry = GlossGeometry::new();
    let open = GlossOpen::new();
    let drag = GlossDrag::new();
    let cache = GlossCache::new();

    GlossController {
        content,
        geometry,
        open,
        drag,
        cache,
        commands: commands::build_commands(state, content, geometry, open, drag, cache),
    }
}
