//! The reader session's engine side of the shared appearance chrome
//! (`app_chrome::appearance_hooks`): the PDF raster pipeline answers the
//! menu's three raster follow-ups for exactly this session's lifetime. The
//! library runtime installs nothing — its appearance menu drives CSS alone.

use std::rc::Rc;

use app_chrome::appearance_hooks::AppearanceEngineHooks;

struct PdfAppearanceHooks;

/// Re-bake this pane's look into its engine, then tell the host the rail's
/// pictures of it are stale: a picture the rail holds is the HOST's copy of
/// a raster baked against the look before this one, and only the host can
/// render its cells again (`PaneToHost::ThumbsStale`). The tokens are
/// already painted — every caller paints before it re-bakes
/// (`app_ui::appearance::raster`), which is what lets the host's render
/// land on the new bake instead of the old one.
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

/// Install the PDF appearance hooks for this reader session. Called at the
/// top of the session scope; the guard is dropped in the disposal chain, so
/// a closed reader's hooks never answer a later runtime's menu drag.
pub fn install() -> app_chrome::appearance_hooks::AppearanceHooksGuard {
    app_chrome::appearance_hooks::install(Rc::new(PdfAppearanceHooks))
}
