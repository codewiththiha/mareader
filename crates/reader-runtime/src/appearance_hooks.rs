//! The reader session's engine side of the appearance chrome.

use std::rc::Rc;

use app_chrome::appearance_hooks::AppearanceEngineHooks;

struct PdfAppearanceHooks;

/// Re-bake the pane's look and tell the host its cells are stale.
pub fn refresh() {
    pdf_engine::api::refresh_theme();
    crate::pane_frame::pictures_stale();
}

impl AppearanceEngineHooks for PdfAppearanceHooks {
    fn refresh_theme(&self) {
        refresh();
    }
    fn set_scrub_mode(&self, on: bool) {
        pdf_engine::api::set_scrub_mode(on);
    }
    fn set_appearance_menu_open(&self, on: bool) {
        pdf_engine::api::set_appearance_menu_open(on);
    }
}

/// Install the PDF appearance hooks for this reader session.
pub fn install() -> app_chrome::appearance_hooks::AppearanceHooksGuard {
    app_chrome::appearance_hooks::install(Rc::new(PdfAppearanceHooks))
}
