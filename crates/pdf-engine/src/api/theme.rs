//! The appearance broadcast: re-bake / scrub mode / menu retention.
//!
//! Appearance is a GLOBAL setting; the rasters it is baked into are
//! session-owned. Each call fans out to every live engine session, which
//! re-derives its OWN raster theme on its own queue — no call here names or
//! mutates a document. (The advisory sweeps are session calls:
//! [`crate::session::PdfSession::sweep`].)

use super::guard_pdf_reader;
use crate::bridge;

/// Re-bake the theme into every raster every live session holds (mounted
/// pages + cached thumbnails). Called by the theme applier right after it
/// writes the new CSS variables; pages render with the new look without a
/// pdf.js re-render.
pub fn refresh_theme() {
    if !guard_pdf_reader() {
        return;
    }
    bridge::refresh_theme();
}

/// Enter/leave the scrub window's real-time compositing. While a slider drag
/// repaints the theme variables every frame, the engine shows the RAW rasters
/// under the live CSS filter/blend so the page re-colours per frame; leaving
/// re-bakes the pre-themed rasters from the raws. The engine swaps canvas
/// contents and the CSS class in the same task, so no frame is ever
/// double-filtered or unfiltered.
pub fn set_scrub_mode(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_scrub_mode(on);
}

/// Whether the appearance popover is open. While it is, the engine retains
/// the unbaked raw of every page that finishes rendering, so the first tint
/// drag of a session blits retained pixels under the live CSS instead of
/// re-rendering every page; closing arms the short idle tail that frees
/// them. A plain flag, not a theme-queue mutation: nothing raster moves.
pub fn set_appearance_menu_open(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_appearance_menu_open(on);
}
