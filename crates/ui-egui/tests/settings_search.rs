//! The settings index finds real controls and navigates without changing them.

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_ui_egui::ui_command::Sent;

use crate::common::reader;

#[test]
fn search_jumps_to_a_setting_and_can_edit_it() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(4);
    let before = h.state().settings().chrome.glass;
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings").type_text("frosted glass");
    h.run_steps(3);
    h.get_by_label("Frosted glass · Layout").click();
    h.run_steps(5);
    assert_eq!(h.state().settings().chrome.glass, before);
    let switch = h.get_by_role_and_label(Role::CheckBox, "Frosted glass");
    assert!(switch.rect().center().y >= 0.0 && switch.rect().center().y <= 700.0);
    switch.click();
    h.run_steps(4);
    assert_ne!(h.state().settings().chrome.glass, before);
}

#[test]
fn no_results_can_be_cleared_with_escape() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Layout)));
    h.run_steps(4);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings")
        .type_text("something impossibly obscure xyz");
    h.run_steps(3);
    h.get_by_label("No settings found. Try another search.");
    h.key_press(Key::Escape);
    h.run_steps(3);
    h.get_by_label("Search settings");
    h.get_by_role_and_label(Role::CheckBox, "Page indicator");
}

#[test]
fn conditional_result_explains_context_without_changing_the_switch() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    let mut settings = h.state().settings().clone();
    settings.layout.floating_label = false;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(settings))));
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(4);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings")
        .type_text("label width limit");
    h.run_steps(3);
    h.get_by_label("Label width limit · Layout").click();
    h.run_steps(4);
    assert!(!h.state().settings().layout.floating_label);
    h.get_by_label(
        "Some options appear only when their feature, download or reading context is available.",
    );
}

#[test]
fn enter_opens_the_first_result_with_keyboard() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(4);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings").type_text("page shadow");
    h.run_steps(3);
    h.key_press(Key::Enter);
    h.run_steps(50);
    let shadow = h
        .get_by_role_and_label(Role::CheckBox, "Page shadow")
        .rect();
    assert!(
        shadow.center().y > 100.0 && shadow.center().y < 620.0,
        "{shadow:?}"
    );
}

#[test]
fn search_and_results_fit_a_smaller_window() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    h.set_size(egui::vec2(550.0, 700.0));
    h.run_steps(3);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(4);
    let field = h.get_by_label("Search settings");
    assert!(field.rect().left() >= 0.0 && field.rect().right() <= 550.0);
    field.click();
    h.run_steps(2);
    h.get_by_label("Search settings")
        .type_text("hover dictionary");
    h.run_steps(4);
    let result = h.get_by_label("Rest on a shaded word to look it up · Vocabulary");
    assert!(result.rect().left() >= 0.0 && result.rect().right() <= 550.0);
}

#[test]
fn jump_scrolls_to_a_lower_setting() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(4);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings").type_text("skip asides");
    h.run_steps(3);
    h.get_by_label("Skip asides · Read aloud").click();
    h.run_steps(12);
    let row = h
        .get_by_role_and_label(Role::CheckBox, "Skip asides")
        .rect();
    assert!(row.center().y > 100.0 && row.center().y < 620.0, "{row:?}");
}

#[test]
fn limited_settings_do_not_search_hidden_categories() {
    let mut h = reader(&[("animations", "false"), ("library", "show")]);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::ReadAloud)));
    h.run_steps(4);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings").type_text("frosted glass");
    h.run_steps(3);
    assert!(h.query_by_label("Frosted glass · Layout").is_none());
    h.get_by_label("No settings found. Try another search.");
}

#[test]
fn page_shadow_jump_reveals_the_row() {
    let mut h = reader(&[("pdf", "sample:5"), ("animations", "false")]);
    h.set_size(egui::vec2(1000.0, 800.0));
    h.run_steps(3);
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(4);
    h.get_by_label("Search settings").click();
    h.run_steps(2);
    h.get_by_label("Search settings").type_text("page shadow");
    h.run_steps(3);
    h.get_by_label("Page shadow · Layout").click();
    h.run_steps(50);
    let row = h
        .get_by_role_and_label(Role::CheckBox, "Page shadow")
        .rect();
    assert!(row.center().y > 150.0 && row.center().y < 700.0, "{row:?}");
}
