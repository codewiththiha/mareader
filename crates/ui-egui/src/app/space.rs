//! Space in the frame: the page below the bar, its CSV saved through the
//! window's file dialog, and the keys a shot shows it by.

use super::*;
use crate::space::Ask;
use mareader_engine::space::CSV_NAME;

impl MareaderApp {
    /// Space fills the window; no pane is drawn behind it.
    pub(super) fn space_page(&mut self, ui: &mut egui::Ui, ctx: &Context, out: &mut Commands) {
        self.ui.pane_rects.clear();
        self.ui.library_rect = None;
        let bar = Route::Space.bar();
        let pinned = self.settings().chrome.pinned(bar);
        let top = self.ui.titlebar.page_top(ctx, bar, pinned);
        let words = self.session.library_model();
        let asked = self.ui.space.show(ui, words, top, out);
        if let Some(Ask::Export(csv)) = asked {
            let name = (crate::space::EXPORT, CSV_NAME.to_owned());
            let bytes = csv.into_bytes();
            self.ui.files.save_as(ctx, name, ("CSV", "csv"), bytes);
        }
    }
}

/// Space's keys a shot or a test sets by name.
///
/// `route=space` (or `library`, `dictionary`, `reader`) shows that page;
/// `space_view=cards` draws the rows as cards. Made-up words to fill it come
/// from the engine's `words=demo`.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    let route = match (key, value) {
        ("route", "space") => Route::Space,
        ("route", "library") => Route::Library,
        ("route", "dictionary") => Route::Dictionary,
        ("route", "reader") => Route::Reader,
        ("space_view", "cards" | "list") => {
            app.ui.space.cards = value == "cards";
            return Some(Ok(()));
        }
        ("route", _) => return Some(Err("route: reader, library, dictionary or space".into())),
        ("space_view", _) => return Some(Err("space_view: list or cards".into())),
        _ => return None,
    };
    app.go(route);
    Some(Ok(()))
}
