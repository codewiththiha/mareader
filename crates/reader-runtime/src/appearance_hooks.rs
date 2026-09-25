//! The reader session's engine side of the shared appearance chrome
//! (`app_chrome::appearance_hooks`): the PDF raster pipeline answers the
//! menu's three raster follow-ups for exactly this session's lifetime. The
//! library runtime installs nothing — its appearance menu drives CSS alone.

use std::rc::Rc;

use app_chrome::appearance_hooks::AppearanceEngineHooks;

struct PdfAppearanceHooks;

impl AppearanceEngineHooks for PdfAppearanceHooks {
    fn refresh_theme(&self) {
        pdf_engine::api::refresh_theme();
    }
    fn set_scrub_mode(&self, on: bool) {
        pdf_engine::api::set_scrub_mode(on);
    }
    fn set_appearance_menu_open(&self, on: bool) {
        pdf_engine::api::set_appearance_menu_open(on);
    }
}

/// Install the PDF appearance hooks for this reader session. Called at the
/// top of the session scope; the guard is dropped in the disposal chain, so
/// a closed reader's hooks never answer a later runtime's menu drag.
pub fn install() -> app_chrome::appearance_hooks::AppearanceHooksGuard {
    app_chrome::appearance_hooks::install(Rc::new(PdfAppearanceHooks))
}
