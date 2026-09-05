//! The raster pipeline's appearance hooks: the token set it paints and the
//! engine bridge it needs. A reflowable page never touches these — its
//! variables live in `reflow.rs` and it repaints from CSS alone, with no
//! engine call.

use reader_core::appearance::Appearance;

/// The seven `--color-*` tokens the tint may override. Listed once so they
/// can be cleared as a set — a stale override left behind when the tint is
/// removed would keep tinting the UI with no way for the user to see why.
pub const UI_TOKENS: [&str; 7] = [
    "--color-paper",
    "--color-surface",
    "--color-line",
    "--color-ink",
    "--color-muted",
    "--color-accent",
    "--color-accent-soft",
];

/// The variables the PDF pipeline paints: the canvas filter/blend pair
/// (always) and the tinted UI-token overrides (empty when no tint is
/// active). The engine bakes its rasters against these.
pub fn token_vars(a: &Appearance) -> Vec<(&'static str, String)> {
    let mut vars = vec![
        ("--canvas-filter", a.canvas_filter()),
        ("--canvas-blend", a.canvas_blend().to_string()),
    ];
    vars.extend(a.ui_overrides());
    vars
}

/// Re-bake the theme into every raster the engine already holds (mounted
/// pages + cached thumbnails). A no-op while no PDF reader is mounted —
/// the guard lives in the engine api.
pub fn refresh_theme() {
    pdf_engine::api::refresh_theme();
}

/// Enter/leave appearance-scrub mode: while a slider drag repaints the
/// variables every frame, the engine shows the RAW rasters under the live
/// CSS filter/blend so the page re-colours per frame; leaving re-bakes
/// from the raws.
pub fn set_scrub_mode(on: bool) {
    pdf_engine::api::set_scrub_mode(on);
}

/// Choose how the appearance reaches the pixels: live (the compositor
/// filters and blends the raw rasters every frame) or baked (the filter
/// is burned into each raster once per appearance change).
pub fn set_live_pipeline(on: bool) {
    pdf_engine::api::set_live_pipeline(on);
}
