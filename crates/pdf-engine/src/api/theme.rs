//! The appearance broadcast: re-bake, scrub mode, menu retention.
use super::guard_pdf_reader;
use crate::bridge;

/// Re-bake the theme into every raster every live session holds.
pub fn refresh_theme() {
    if !guard_pdf_reader() {
        return;
    }
    bridge::refresh_theme();
}

/// Enter or leave the scrub window's real-time compositing.
pub fn set_scrub_mode(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_scrub_mode(on);
}

/// Whether the appearance popover is open; the engine retains raws.
pub fn set_appearance_menu_open(on: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_appearance_menu_open(on);
}
