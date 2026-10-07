//! The gloss state-machine hub: the aggregate and its slices.

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

/// Per-document cap on persisted marks; the oldest evicted.
pub const MARK_CAP: usize = 200;

/// All gloss state and shared behaviours, grouped in slices.
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
pub fn use_gloss_controller(state: crate::context::ReaderContext) -> GlossController {
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
