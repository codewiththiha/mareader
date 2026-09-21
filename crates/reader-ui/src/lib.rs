//! reader-ui: shared reader chrome, virtualization and reflow maths.
//! Used by pdf/txt/md readers. Must NOT depend on pdf-engine.

pub use ai_core;
pub use reader_core;
pub use reflow_core;
pub use ui_common;
pub use virtual_list;
pub use virtual_list_leptos;

/// Common reader surface mounting.
/// Format-specific readers delegate here then add their engine.
pub fn mount_reader_surface() {
    // Actual ReaderSurface lives in mareader crate; this stub documents the boundary.
}
