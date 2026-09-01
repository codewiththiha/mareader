//! Appearance re-bake / scrub mode and the advisory memory sweep.

use super::guard_pdf_reader;
use crate::bridge;

/// Re-bake the theme into every raster the engine already holds (mounted
/// pages + cached thumbnails). Called by the theme applier right after it
/// writes the new CSS variables; pages render with the new look without a
/// pdf.js re-render.
pub fn refresh_theme() {
    if !guard_pdf_reader() {
        return;
    }
    bridge::refresh_theme();
}

/// Enter/leave appearance-scrub mode. While a slider drag repaints the theme
/// variables every frame, the engine shows the RAW rasters under the live
/// CSS filter/blend (the pre-baking pipeline) so the page re-colours per
/// frame; leaving re-bakes from the raws. The engine swaps canvas contents
/// and the CSS class in the same task, so no frame is ever double-filtered
/// or unfiltered.
pub fn set_scrub_mode(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_scrub_mode(on);
}

/// Choose how the appearance reaches the pixels: live (the compositor filters
/// and blends the raw rasters every frame, so pages and the backdrop share one
/// pass) or baked (the filter is burned into each raster once per appearance
/// change, leaving plain opaque textures on screen). The engine swaps the
/// rasters for the new mode; the caller only states the preference.
pub fn set_live_pipeline(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_live_pipeline(on);
}

/// Release rasters/caches the engine no longer needs (advisory
/// `pdf.cleanup`). Fired when reading work ends: zoom commit, mode flip,
/// scroll idle — so memory drops immediately instead of waiting for the
/// engine's own 30s idle sweep.
pub fn sweep() {
    if !guard_pdf_reader() {
        return;
    }
    bridge::sweep();
}
