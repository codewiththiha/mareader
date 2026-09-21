//! ui-common: shared primitives for all isolated WASM runtimes.
//! Mirrors the plan's ui-common crate: leptos, wasm-bindgen, minimal
//! web-sys surface, tauri-bridge, app-chrome and ui-geom.
//! Format-specific code (pdf-engine, reflow, txt/md parsers) must NOT enter here.

pub use app_chrome;
pub use tauri_bridge;
pub use ui_geom;

/// Thumbnail source abstraction — the plan's core boundary.
/// One UI, multiple providers (reader-direct vs workspace-remote).
pub trait ThumbnailSource {
    fn page_count(&self) -> leptos::prelude::Signal<u32>;
    fn current_page(&self) -> leptos::prelude::Signal<u32>;
    fn thumbnail(&self, page: u32) -> leptos::prelude::Signal<Option<String>>;
    fn request(&self, page: u32);
}

/// Geometry invariant: the virtualizer and the card must agree.
/// CELL_W is the thumbnail column width; row_height respects aspect.
pub mod geometry {
    pub const CELL_W: f64 = 120.0;
    pub const GAP_CROSS: f64 = 12.0;
    pub const PAD: f64 = 12.0;
    pub const ROW_BUFFER: usize = 4;
    pub const MIN_VIEWPORT_H: f64 = 400.0;
    pub fn row_height(aspect: f64) -> f64 {
        let a = if aspect.is_finite() && aspect > 0.1 && aspect < 5.0 { aspect } else { 1.414 };
        CELL_W * a
    }
}
