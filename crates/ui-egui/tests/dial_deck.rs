//! Tests for the Dial deck Appearance popover (`appearance_menu = dial_deck`).

use egui::Key;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_appearance::{BaseMode, NoiseMode, TextureMode};
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_settings::AppearanceMenuStyle;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

fn reader(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    for (k, v) in options {
        app.set_option(k, v).expect("valid test option");
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 760.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .build_eframe(|_| app);
    h.run_steps(6);
    h
}

#[test]
fn classic_remains_the_default_and_settings_switches_to_dial_deck() {
    let mut h = reader(&[("epub", "sample:2"), ("pinned", "true")]);
    assert_eq!(
        h.state().settings().chrome.appearance_menu,
        AppearanceMenuStyle::Classic
    );
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(3);
    assert!(h.query_by_label("Mode & colour").is_some());
    assert!(h.query_by_label("Tint colour and strength").is_none());
    h.key_press(Key::Escape);
    h.run_steps(2);

    // Switch to Dial deck in Settings -> Appearance while reading a book.
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Appearance)));
    h.run_steps(3);
    let pick = h.get_by_label("Dial deck");
    pick.scroll_to_me();
    h.run();
    h.get_by_label("Dial deck").click();
    h.run_steps(2);
    assert_eq!(
        h.state().settings().chrome.appearance_menu,
        AppearanceMenuStyle::DialDeck
    );

    h.get_by_label("Open Appearance").click();
    h.run_steps(4);
    assert!(h.query_by_label("Mode & colour").is_none());
    assert!(h.query_by_label("Tint colour and strength").is_some());
}

#[test]
fn look_tab_controls_mode_wheel_landmarks_reset_and_book_ink() {
    let mut h = reader(&[
        ("epub", "sample:2"),
        ("pinned", "true"),
        ("appearance_menu", "dial_deck"),
    ]);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(3);

    h.get_by_label("Dim").click();
    h.run_steps(2);
    assert_eq!(h.state().settings().appearance.base, BaseMode::Dim);

    h.get_by_label("Sepia hue").click();
    h.run_steps(2);
    let a = h.state().settings().appearance;
    assert_eq!(a.tint_hue, 34);
    assert_eq!(
        a.tint_strength,
        mareader_engine::appearance_menu::STRENGTH_ON_HUE
    );

    h.get_by_label("Reset tint").click();
    h.run_steps(2);
    assert_eq!(h.state().settings().appearance.tint_strength, 0);

    let ink = h.get_by_role_and_label(Role::Slider, "Text ink");
    ink.focus();
    h.run();
    h.key_press(Key::ArrowDown);
    h.run_steps(2);
    assert!((h.state().settings().text.ink_contrast - 95.0).abs() < 0.1);
}

#[test]
fn text_tab_dials_appear_for_books_and_hide_for_pdfs() {
    let mut h = reader(&[
        ("epub", "sample:2"),
        ("pinned", "true"),
        ("appearance_menu", "dial_deck"),
    ]);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(3);
    h.get_by_label("Text").click();
    h.run_steps(2);

    let size = h.get_by_role_and_label(Role::Slider, "Size");
    size.focus();
    h.run();
    h.key_press(Key::ArrowUp);
    h.run_steps(2);
    assert!((h.state().settings().text.size_pct() - 105.0).abs() < 0.5);

    h.get_by_role_and_label(Role::CheckBox, "Override book layout")
        .click();
    h.run_steps(2);
    assert!(h.state().settings().text.override_layout);

    let mut pdf = reader(&[
        ("pdf", "sample:3"),
        ("pinned", "true"),
        ("appearance_menu", "dial_deck"),
    ]);
    pdf.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    pdf.run_steps(3);
    assert!(pdf.query_by_label("Text").is_none());
    assert!(
        pdf.query_by_role_and_label(Role::Slider, "Text ink")
            .is_none()
    );
}

#[test]
fn page_and_grain_tabs_adapt_to_route_and_keep_fixed_width() {
    let mut h = reader(&[
        ("pdf", "sample:3"),
        ("pinned", "true"),
        ("appearance_menu", "dial_deck"),
    ]);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(3);
    let id = egui::Id::new(mareader_ui_egui::titlebar::APPEARANCE_POPOVER);
    let width = |h: &Harness<'static, MareaderApp>| {
        h.ctx.memory(|m| m.area_rect(id)).map_or(0.0, |r| r.width())
    };
    let w_look = width(&h);

    h.get_by_label("Page").click();
    h.run_steps(2);
    assert!(h.query_by_label("Page texture").is_some());
    h.get_by_label("Grid").click();
    h.run_steps(2);
    assert_eq!(h.state().settings().appearance.texture, TextureMode::Grid);

    let grain = h.get_by_role_and_label(Role::Slider, "Grain");
    grain.focus();
    h.run();
    h.key_press(Key::ArrowUp);
    h.run_steps(2);
    assert_eq!(h.state().settings().appearance.noise, NoiseMode::Static);
    assert!(h.state().settings().appearance.noise_intensity > 0);
    assert_eq!(width(&h), w_look, "tab and dial values never shift width");

    // In the library there is no page to texture: Grain replaces Page.
    h.key_press(Key::Escape);
    h.run_steps(2);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run_steps(2);
    h.get_by_label("Appearance").click();
    h.run_steps(3);
    assert!(h.query_by_label("Page").is_none());
    assert!(h.query_by_label("Page texture").is_none());
    h.get_by_label("Grain").click();
    h.run_steps(2);
    assert!(
        h.query_by_role_and_label(Role::Slider, "Intensity")
            .is_some()
    );
}

#[test]
fn presets_tab_applies_saves_and_deletes_looks_and_pins_split_switches() {
    let mut h = reader(&[
        ("pdf", "sample:3"),
        ("panes", "2"),
        ("pinned", "true"),
        ("appearance_menu", "dial_deck"),
    ]);
    h.state_mut().queue(Sent::Ui(UiCommand::ShowAppearance));
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Theme per pane")
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Texture per pane")
            .is_some()
    );

    h.get_by_label("Presets").click();
    h.run_steps(2);
    h.get_by_label("Parchment").click();
    h.run_steps(2);
    assert_eq!(h.state().settings().appearance.texture, TextureMode::Paper);

    h.get_by_label("Save current look as preset").click();
    h.run_steps(2);
    h.get_by_label("Preset name").type_text("Warm Study");
    h.run_steps(2);
    h.get_by_label("Save").click();
    h.run_steps(3);
    assert!(
        h.state()
            .settings()
            .user_presets
            .iter()
            .any(|p| p.name == "Warm Study")
    );

    h.get_by_label("Delete Warm Study").click();
    h.run_steps(3);
    assert!(
        h.state()
            .settings()
            .user_presets
            .iter()
            .all(|p| p.name != "Warm Study")
    );
}
