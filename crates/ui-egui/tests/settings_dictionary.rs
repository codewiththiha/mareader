//! Dictionary settings folds.

use egui_kittest::kittest::Queryable as _;
use mareader_engine::{command::Command, settings::Tab};
use mareader_ui_egui::{settings_window::DOWNLOADED_LANGUAGES, ui_command::Sent};

#[test]
fn downloaded_languages_fold_without_removing_the_packs() {
    let dir = crate::dictionary::shelf();
    let mut h = crate::dictionary::reader(dir.path());
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Dictionary)));
    h.run();
    h.get_by_label("Remove English → French");
    h.get_by_label(DOWNLOADED_LANGUAGES).click();
    h.run();
    assert!(h.query_by_label("Remove English → French").is_none());
    h.get_by_label(DOWNLOADED_LANGUAGES).click();
    h.run();
    h.get_by_label("Remove English → French");
}
