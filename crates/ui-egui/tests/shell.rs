//! The whole app, headless: title bar, pin, panes and restyling.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_engine::command::Command;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::UiCommand;

fn harness(app: MareaderApp) -> Harness<'static, MareaderApp> {
    sized(app, egui::vec2(1200.0, 800.0))
}

fn sized(app: MareaderApp, size: egui::Vec2) -> Harness<'static, MareaderApp> {
    let mut h = Harness::builder()
        .with_size(size)
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(64)
        .build_eframe(|_| app);
    h.run_steps(3);
    h
}

#[test]
fn the_bar_appears_on_hover_and_hides_after_the_grace() {
    let mut h = harness(MareaderApp::default());
    assert!(!h.state().bar_shown());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(2);
    assert!(h.state().bar_shown());
    h.hover_at(egui::pos2(600.0, 400.0));
    h.run_steps(2);
    assert!(h.state().bar_shown(), "the 400 ms grace keeps it");
    for _ in 0..40 {
        h.step();
    }
    assert!(!h.state().bar_shown());
}

#[test]
fn a_split_adds_and_focuses_a_pane() {
    let mut h = harness(MareaderApp::default());
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(Command::Split(
            mareader_workspace::SplitAxis::Horizontal,
        )));
    h.run_steps(2);
    assert_eq!(h.state().panes().len(), 2);
    assert_eq!(h.state().active_pane(), h.state().panes()[1]);
}

#[test]
fn the_bar_has_no_split_buttons() {
    let mut h = with(&[("platform", "windows")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    assert!(h.query_by_label("Search").is_some(), "the bar is up");
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    // Panes split by dragging a book from the sidebar onto an edge.
    for gone in ["Split right", "Split down"] {
        assert!(h.query_by_label(gone).is_none(), "{gone}");
    }
}

#[test]
fn a_pinned_bar_stays_after_the_pointer_leaves() {
    let mut app = MareaderApp::default();
    app.set_option("pinned", "true")
        .map_err(|e| e.to_string())
        .ok();
    let mut h = harness(app);
    h.hover_at(egui::pos2(600.0, 500.0));
    for _ in 0..40 {
        h.step();
    }
    assert!(h.state().bar_shown());
}

#[test]
fn keep_title_bar_shown_holds_it_after_the_pointer_leaves() {
    let mut h = harness(MareaderApp::default());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    h.get_by_label("Keep title bar shown").click();
    h.run_steps(2);
    h.hover_at(egui::pos2(600.0, 500.0));
    for _ in 0..40 {
        h.step();
    }
    assert!(h.state().bar_shown());
}

#[test]
fn closing_a_pane_from_its_button_focuses_the_survivor() {
    let mut app = MareaderApp::default();
    app.set_option("panes", "2").map_err(|e| e.to_string()).ok();
    let mut h = harness(app);
    let survivor = h.state().panes()[0];
    h.hover_at(egui::pos2(900.0, 400.0));
    h.run_steps(2);
    h.get_by_label("Close pane").click();
    h.run_steps(2);
    assert_eq!(h.state().panes(), vec![survivor]);
    assert_eq!(h.state().active_pane(), survivor);
}

#[test]
fn an_appearance_change_restyles_on_the_frame_it_lands() {
    let mut h = harness(MareaderApp::default());
    let sepia = mareader_appearance::builtin_presets()
        .into_iter()
        .find(|p| p.id == "sepia")
        .map(|p| p.appearance)
        .unwrap_or_default();
    let want = mareader_ui_egui::theme::Tokens::new(&sepia);
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(
            Command::SetAppearance(sepia),
        ));
    h.step();
    assert_eq!(mareader_ui_egui::theme::Tokens::get(&h.ctx), want);
    assert_eq!(h.ctx.global_style().visuals.panel_fill, want.surface);
}

fn open_appearance(h: &mut Harness<'static, MareaderApp>) {
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(2);
    h.get_by_label("Appearance").click();
    h.run_steps(2);
}

#[test]
fn the_appearance_popover_switches_mode_and_keeps_the_bar() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    h.get_by_label("Dark").click();
    h.run_steps(2);
    let base = h.state().settings().appearance.base;
    assert_eq!(base, mareader_appearance::BaseMode::Dark);
    assert!(mareader_ui_egui::theme::Tokens::get(&h.ctx).dark);
    h.hover_at(egui::pos2(600.0, 500.0));
    h.run_steps(40);
    assert!(h.state().bar_shown(), "an open popover holds the bar");
}

/// The popover hangs from the bar rather than its button.
#[test]
fn the_appearance_popover_hangs_off_the_bar_near_the_edge() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    let id = egui::Id::new(mareader_ui_egui::titlebar::APPEARANCE_POPOVER);
    let area = h
        .ctx
        .memory(|m| m.area_rect(id))
        .expect("the popover is laid out");
    let screen = h.ctx.content_rect();
    // The flyout sits just inside the window edge.
    let margin = mareader_ui_egui::titlebar::FLYOUT_MARGIN;
    let off_right = screen.right() - area.right();
    assert!(
        (off_right - margin).abs() <= 1.0,
        "{off_right} points off the right edge, not {margin}"
    );
    // The flyout meets the bar without overlapping it.
    let bar_bottom = screen.top() + mareader_ui_egui::theme::TITLE_BAR_H;
    let off_bar = area.top() - bar_bottom;
    assert!(
        (-1.0..=1.0).contains(&off_bar),
        "{off_bar} points off the bar's underside at {bar_bottom}"
    );
}

#[test]
fn a_preset_swatch_applies_its_look() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    h.get_by_label("Sepia").click();
    h.run_steps(2);
    let sepia = mareader_appearance::builtin_presets()
        .into_iter()
        .find(|p| p.id == "sepia")
        .map(|p| p.appearance);
    assert_eq!(Some(h.state().settings().appearance), sepia);
}

#[test]
fn preset_cards_are_wide_enough_for_their_names() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    // "Parchment" was cut to "Parchme…"; one row stays, the name fits.
    let card = h.get_by_label("Parchment").rect();
    let first = h.get_by_label("Sepia").rect();
    assert_eq!(card.top(), first.top(), "the five share a row");
    let font = egui::FontId::proportional(10.0);
    let name = h.ctx.fonts_mut(|f| {
        f.layout_no_wrap("Parchment".into(), font, egui::Color32::WHITE)
            .size()
            .x
    });
    // A centred name may use the 6 pt gaps beside its card.
    assert!(
        name <= card.width() + 4.0,
        "{name:.1} in {:.1}",
        card.width()
    );
}

/// [`with`], with the engine's own two files kept in `dir`.
fn with_kept(dir: &std::path::Path, options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::kept(dir);
    for (k, v) in options {
        app.set_option(k, v).map_err(|e| e.to_string()).ok();
    }
    harness(app)
}

fn with(options: &[(&str, &str)]) -> Harness<'static, MareaderApp> {
    let mut app = MareaderApp::default();
    for (k, v) in options {
        app.set_option(k, v).map_err(|e| e.to_string()).ok();
    }
    harness(app)
}

#[test]
fn the_bar_toggle_docks_the_sidebar_and_hands_closing_to_it() {
    let mut h = harness(MareaderApp::default());
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    h.get_by_label("Toggle sidebar").click();
    h.run_steps(30);
    assert!(h.state().sidebar().is_open());
    assert!(h.query_by_label("Toggle sidebar").is_none());
    h.get_by_label("Close sidebar").click();
    h.run_steps(30);
    assert!(!h.state().sidebar().is_open());
}

#[test]
fn the_bar_band_starts_after_a_docked_sidebar() {
    let mut h = with(&[("sidebar", "outline")]);
    h.hover_at(egui::pos2(150.0, 20.0));
    h.run_steps(4);
    assert!(!h.state().bar_shown(), "the rail's header is not the band");
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(4);
    assert!(h.state().bar_shown());
}

#[test]
fn the_switcher_changes_panel_and_a_reclick_keeps_it() {
    use mareader_ui_egui::sidebar::Panel;
    let mut h = with(&[("sidebar", "thumbnails")]);
    h.get_by_role_and_label(Role::Button, "Outline").click();
    h.run_steps(2);
    assert_eq!(h.state().sidebar().panel(), Panel::Outline);
    h.get_by_role_and_label(Role::Button, "Outline").click();
    h.run_steps(2);
    assert!(h.state().sidebar().is_open());
    assert_eq!(h.state().sidebar().panel(), Panel::Outline);
}

#[test]
fn a_floating_sidebar_opens_from_the_edge_and_closes_after_the_grace() {
    let mut h = with(&[("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    h.run_steps(8);
    assert!(h.query_by_label("Toggle sidebar").is_none());
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(4);
    assert!(h.state().sidebar().is_open());
    h.hover_at(egui::pos2(150.0, 400.0));
    h.run_steps(30);
    assert!(h.state().sidebar().is_open(), "on the rail it stays");
    h.hover_at(egui::pos2(800.0, 400.0));
    h.run_steps(4);
    assert!(h.state().sidebar().is_open(), "the grace keeps it a moment");
    h.run_steps(30);
    assert!(!h.state().sidebar().is_open());
}

#[test]
fn the_edge_waits_while_the_pointer_is_in_the_title_bar() {
    let mut h = with(&[("platform", "mac"), ("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.state().bar_shown());
    // Overshooting the bar's buttons onto the edge doesn't move them.
    h.hover_at(egui::pos2(2.0, 20.0));
    wait(&mut h, 0.2);
    assert!(!h.state().sidebar().is_open(), "a brush past the edge");
    wait(&mut h, 0.45);
    assert!(h.state().sidebar().is_open(), "resting there opens it");
    // Below the bar the edge opens at once.
    let mut h = with(&[("platform", "mac"), ("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(2);
    assert!(h.state().sidebar().is_open());
}

#[test]
fn docking_a_floating_sidebar_keeps_it_open() {
    let mut h = with(&[("float", "true"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(150.0, 400.0));
    h.run_steps(4);
    h.get_by_label("Dock sidebar").click();
    h.run_steps(4);
    assert!(!h.state().settings().chrome.sidebar_overlay);
    assert!(h.state().sidebar().is_open());
}

#[test]
fn escape_closes_the_sidebar() {
    let mut h = with(&[("sidebar", "outline")]);
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().sidebar().is_open());
}

#[test]
fn a_command_schedules_the_frame_that_shows_it() {
    let mut h = with(&[("float", "true"), ("sidebar", "outline")]);
    h.run_steps(2);
    h.state_mut().queue(mareader_ui_egui::ui_command::Sent::Ui(
        UiCommand::CloseSidebar,
    ));
    // `run` steps only while the app asks for frames, like a real window.
    h.run();
    assert!(h.query_by_label("Close sidebar").is_none(), "faded out");
}

fn open_settings(h: &mut Harness<'static, MareaderApp>) {
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    // A docked sidebar's header has a gear; else it's in the menu.
    if h.query_by_label("Settings").is_none() {
        h.get_by_label("View & tools").click();
        h.run_steps(3);
    }
    if let Some(gear) = h.query_all_by_label("Settings").last() {
        gear.click();
    }
    h.run_steps(4);
}

#[test]
fn the_gear_opens_settings_and_escape_closes_only_them() {
    let mut h = with(&[("sidebar", "outline")]);
    open_settings(&mut h);
    assert!(h.state().settings_window().is_open());
    h.key_press(egui::Key::Escape);
    h.run_steps(2);
    assert!(!h.state().settings_window().is_open());
    assert!(h.state().sidebar().is_open(), "Escape was the window's");
}

#[test]
fn a_settings_switch_applies_on_the_frame_it_flips() {
    let mut h = with(&[("settings", "layout")]);
    h.get_by_role_and_label(Role::CheckBox, "Overlay sidebar")
        .click();
    h.step();
    h.step();
    assert!(h.state().settings().chrome.sidebar_overlay);
}

#[test]
fn the_system_asking_for_less_motion_stills_it_like_the_master() {
    use mareader_ui_egui::motion;
    let mut h = with(&[]);
    assert!(motion::enabled(&h.ctx) && !motion::reduced(&h.ctx));
    h.state_mut().set_option("reduced", "true").ok();
    h.run_steps(2);
    assert!(!motion::enabled(&h.ctx) && motion::reduced(&h.ctx));
    assert_eq!(motion::icon_secs(&h.ctx, 0.5), 0.0);
    assert_eq!(h.ctx.global_style().animation_time, 0.0);
    assert!(
        h.state().settings().motion.enabled,
        "the setting is the user's"
    );
}

#[test]
fn the_glass_switch_starts_on_and_turns_it_off() {
    let mut h = with(&[("settings", "layout")]);
    assert!(h.state().settings().chrome.glass);
    h.get_by_role_and_label(Role::CheckBox, "Frosted glass")
        .click();
    h.step();
    h.step();
    assert!(!h.state().settings().chrome.glass);
}

#[test]
fn the_motion_tab_hides_each_switch_with_the_master() {
    use mareader_engine::settings::Tab;
    let mut h = with(&[("settings", "animations")]);
    let each = "Icon animations";
    assert!(h.query_by_role_and_label(Role::CheckBox, each).is_some());
    h.get_by_role_and_label(Role::CheckBox, "Animations")
        .click();
    h.run_steps(2);
    assert!(!h.state().settings().motion.enabled);
    assert!(h.query_by_role_and_label(Role::CheckBox, each).is_none());
    // The tab stays put, so the list never shifts.
    let tab = h.state().settings_window().tab();
    assert_eq!(tab, Tab::Motion);
}

#[test]
fn the_icon_animation_switch_stills_only_icons() {
    use mareader_ui_egui::motion;
    let mut h = with(&[("settings", "animations")]);
    assert_eq!(motion::icon_secs(&h.ctx, 0.5), 0.5);
    h.get_by_role_and_label(Role::CheckBox, "Icon animations")
        .click();
    h.run_steps(2);
    assert!(h.state().settings().motion.enabled);
    assert!(!h.state().settings().motion.icon_animations);
    assert_eq!(motion::icon_secs(&h.ctx, 0.5), 0.0);
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Sidebar animation")
            .is_some()
    );
}

#[test]
fn every_settings_tab_is_named_in_its_sidebar() {
    use mareader_engine::settings::Tab;
    let mut h = with(&[("settings", "layout")]);
    for tab in Tab::ALL {
        let label = tab.label();
        let named = h.query_all_by_role_and_label(Role::Button, label);
        assert!(named.count() >= 1, "{label}");
    }
    h.get_by_role_and_label(Role::Button, "Panes").click();
    h.run_steps(2);
    assert_eq!(h.state().settings_window().tab(), Tab::Panes);
    // The old names land on their new homes.
    assert_eq!(Tab::parse("fonts"), Some(Tab::Appearance));
    assert_eq!(Tab::parse("Theme"), Some(Tab::Appearance));
    assert_eq!(Tab::parse("animations"), Some(Tab::Motion));
    assert_eq!(Tab::parse("workspace"), Some(Tab::Panes));
}

#[test]
fn a_dependent_setting_shows_once_its_parent_is_on() {
    let mut h = with(&[("settings", "layout")]);
    let on = h.state().settings().layout.floating_label;
    assert_eq!(h.query_by_label("Label shows").is_some(), on);
    h.get_by_role_and_label(Role::CheckBox, "Floating label")
        .click();
    h.run_steps(2);
    assert_eq!(h.query_by_label("Label shows").is_some(), !on);
    let mut h = with(&[("panes", "2"), ("settings", "panes")]);
    let shared = "Light, Dark and Dim change every pane";
    assert!(h.query_by_role_and_label(Role::CheckBox, shared).is_none());
    independent_themes(&mut h);
    h.run_steps(2);
    assert!(h.query_by_role_and_label(Role::CheckBox, shared).is_some());
}

#[test]
fn split_pane_outline_colour_comes_from_the_workspace_tab() {
    let mut h = with(&[("panes", "2"), ("settings", "panes")]);
    h.get_by_label("Red").scroll_to_me();
    h.run();
    h.get_by_label("Red").click();
    h.run_steps(2);
    let look = h.state().settings().workspace.look;
    assert_eq!(look.outline_color, mareader_settings::Swatch::Red);
}

#[test]
fn settings_survive_a_save_and_relaunch() -> Result<(), String> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let options = [("float", "true"), ("theme", "dim")];
    let mut h = with_kept(dir.path(), &options);
    h.state_mut().flush();
    // The next launch reads the two files the engine wrote.
    let back = MareaderApp::kept(dir.path());
    assert_eq!(back.settings(), h.state().settings());
    assert!(back.settings().chrome.sidebar_overlay);
    Ok(())
}

#[test]
fn open_appearance_from_settings_swaps_the_window_for_the_popover() {
    let mut h = with(&[("settings", "theme")]);
    h.get_by_label("Open Appearance").click();
    h.run_steps(3);
    assert!(!h.state().settings_window().is_open());
    assert!(egui::Popup::is_id_open(
        &h.ctx,
        egui::Id::new(mareader_ui_egui::titlebar::APPEARANCE_POPOVER)
    ));
}

/// Steps `secs` of app time at 60 frames a second.
fn wait(h: &mut Harness<'static, MareaderApp>, secs: f32) {
    h.run_steps((secs * 60.0).ceil() as usize);
}

#[test]
fn a_toolbar_tooltip_names_the_control_and_its_shortcut() {
    let mut h = with(&[("platform", "gnome"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    // The sidebar header's gear; the bar keeps Settings in its menu.
    h.get_by_label("Settings").hover();
    wait(&mut h, 0.6);
    assert!(
        h.query_by_label("Ctrl+,").is_some(),
        "the shortcut rides in the tooltip"
    );
}

#[test]
fn a_narrow_bar_folds_tools_into_the_menu() {
    let mut app = MareaderApp::default();
    app.set_option("platform", "windows").ok();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", "sample:3").ok();
    let mut h = sized(app, egui::vec2(600.0, 600.0));
    h.hover_at(egui::pos2(300.0, 20.0));
    h.run_steps(8);
    assert!(h.query_by_label("Search").is_some(), "room for Search");
    // Narrower, Search folds, and works from the menu.
    h.set_size(egui::vec2(480.0, 600.0));
    h.hover_at(egui::pos2(240.0, 20.0));
    h.run_steps(8);
    assert!(h.query_by_label("Search").is_none(), "it folds");
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    h.get_by_label("Search").click();
    h.run_steps(3);
    assert!(h.state().find().is_open(), "a folded tool still works");
}

#[test]
fn settings_and_the_pin_live_in_the_menu() {
    let mut h = with(&[("platform", "windows")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    for gone in ["Settings", "Pin title bar"] {
        assert!(h.query_by_label(gone).is_none(), "{gone} on the bar");
    }
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    h.get_by_label("Keep title bar shown");
    h.get_by_label("Settings").click();
    h.run_steps(3);
    assert!(h.state().settings_window().is_open());
}

#[test]
fn a_docked_sidebar_takes_the_bar_s_tools() {
    let mut h = with(&[("platform", "windows"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    // One Search, the header's: the bar shows no second copy.
    assert_eq!(h.query_all_by_label("Search").count(), 1);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("Search").count(), 1, "nor its menu");
}

#[test]
fn a_wide_bar_keeps_every_tool_out_of_the_menu() {
    let mut h = with(&[("platform", "windows")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    assert_eq!(h.query_all_by_label("Search").count(), 1, "bar only");
    assert!(h.query_by_label("Full screen").is_some());
}

#[test]
fn the_menu_opens_the_shortcut_sheet_in_the_middle() {
    let mut h = with(&[("platform", "gnome")]);
    h.hover_at(egui::pos2(700.0, 20.0));
    h.run_steps(8);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    assert!(h.query_by_label("Esc").is_none(), "no list in the menu");
    h.get_by_label("Keyboard shortcuts").click();
    h.run_steps(3);
    assert!(mareader_ui_egui::shortcuts::is_open(&h.ctx));
    let menu = egui::Id::new(mareader_ui_egui::view_menu::VIEW_MENU);
    assert!(!egui::Popup::is_id_open(&h.ctx, menu), "the menu made way");
    assert!(h.query_by_label("Esc").is_some());
    let f11 = h.get_by_label("F11").rect();
    let middle = h.ctx.content_rect().center();
    let sheet = h.get_by_label("Filter shortcuts").rect();
    assert!(
        (sheet.center().x - middle.x).abs() < 4.0,
        "centred: {sheet:?}"
    );
    assert!(f11.top() > sheet.bottom(), "keys sit under the filter");
}

#[test]
fn command_slash_toggles_the_sheet_and_escape_shuts_it() {
    let mut h = with(&[("platform", "gnome")]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Slash);
    h.run_steps(3);
    assert!(mareader_ui_egui::shortcuts::is_open(&h.ctx));
    // Typing filters; the filter takes focus as the sheet opens.
    h.event(egui::Event::Text("pane".into()));
    h.run_steps(3);
    assert!(h.query_by_label("Next pane").is_some());
    assert!(
        h.query_by_label("F11").is_none(),
        "full screen filtered out"
    );
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Slash);
    h.run_steps(3);
    assert!(
        !mareader_ui_egui::shortcuts::is_open(&h.ctx),
        "its key shuts it"
    );
    h.state_mut().queue(mareader_ui_egui::ui_command::Sent::Ui(
        UiCommand::ShowShortcuts,
    ));
    h.run_steps(3);
    h.key_press(egui::Key::Escape);
    h.run_steps(3);
    assert!(
        !mareader_ui_egui::shortcuts::is_open(&h.ctx),
        "Esc shuts it"
    );
}

#[test]
fn command_comma_opens_settings() {
    let mut h = harness(MareaderApp::default());
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Comma);
    h.run_steps(3);
    assert!(h.state().settings_window().is_open());
}

#[test]
fn a_toast_leaves_by_itself() {
    let mut h = with(&[("toast", "Copied")]);
    assert!(h.query_by_label("Copied").is_some());
    wait(&mut h, 3.0);
    assert!(h.state().toast().is_some(), "still up before 3.5 s");
    wait(&mut h, 1.0);
    assert!(h.state().toast().is_none());
    assert!(h.query_by_label("Copied").is_none());
}

#[test]
fn reading_a_toast_keeps_it_up() {
    let mut h = with(&[("toast", "Copied")]);
    h.get_by_label("Copied").hover();
    wait(&mut h, 6.0);
    assert!(h.state().toast().is_some(), "hover holds it");
    h.hover_at(egui::pos2(600.0, 300.0));
    wait(&mut h, 2.0);
    assert!(
        h.state().toast().is_some(),
        "its clock picks up where it stopped"
    );
    wait(&mut h, 2.0);
    assert!(h.state().toast().is_none());
}

#[test]
fn a_toast_closes_from_its_button() {
    let mut h = with(&[("toast", "error:Could not open the file")]);
    h.get_by_label("Dismiss").click();
    wait(&mut h, 0.5);
    assert!(h.state().toast().is_none());
}

#[test]
fn a_toast_sits_in_the_bottom_right_corner() {
    let h = with(&[("toast", "Copied")]);
    let card = h.get_by_label("Copied").rect();
    // Over the reader's page counter, which gives way while it is up.
    assert!(card.right() > 1150.0 && card.bottom() > 770.0, "{card:?}");
}

#[test]
fn undo_on_a_toast_is_a_button_not_a_handle() {
    let mut h = with(&[("toast", "Saved notes"), ("toast", "undo:Moved_a_book")]);
    let undo = h.get_by_label("Undo").rect().center();
    h.hover_at(undo);
    h.run_steps(2);
    let cursor = h.output().platform_output.cursor_icon;
    assert_eq!(cursor, egui::CursorIcon::PointingHand);
    let rest = h.get_by_label("Moved a book").rect();
    h.drag_at(undo);
    h.run_steps(1);
    for step in 1..=4 {
        h.hover_at(undo + egui::vec2(10.0 * step as f32, 0.0));
        h.run_steps(1);
    }
    let held = h.get_by_label("Moved a book").rect();
    assert!((held.left() - rest.left()).abs() < 1.0, "it drags nothing");
    h.drop_at(undo + egui::vec2(40.0, 0.0));
    h.run_steps(3);
    assert!(!h.state().toasts().1, "nor opens the list");
    assert_eq!(stacked(&h).len(), 2);
}

#[test]
fn the_newest_undo_toast_has_a_working_shortcut() {
    let mut h = with(&[("toast", "undo:Moved_a_book")]);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(4);
    assert!(stacked(&h).is_empty());
}

#[test]
fn a_stack_never_takes_more_than_its_corner() {
    let h = with(&[("toast", "Saved notes"), ("toast", "Copied link")]);
    let layer = h.ctx.memory(|m| m.area_rect(egui::Id::new("toasts")));
    let corner = egui::Rect::from_min_max(egui::pos2(800.0, 600.0), egui::pos2(1200.0, 800.0));
    assert!(layer.is_some_and(|r| corner.contains_rect(r)), "{layer:?}");
}

/// Shown toasts, newest first.
fn stacked(h: &Harness<'static, MareaderApp>) -> Vec<String> {
    let (shown, _) = h.state().toasts();
    shown.iter().map(|t| t.message.clone()).collect()
}

#[test]
fn toasts_stack_newest_in_front_and_clear_all_empties_it() {
    let mut h = with(&[
        ("toast", "Saved notes"),
        ("toast", "Copied link"),
        ("toast", "Exported list"),
    ]);
    assert_eq!(stacked(&h), ["Exported list", "Copied link", "Saved notes"]);
    assert!(h.query_by_label("Exported list").is_some());
    assert!(
        h.query_by_label("Saved notes").is_none(),
        "the ones behind peek"
    );
    h.get_by_label("Exported list").hover();
    h.run_steps(3);
    h.get_by_label("Clear all").click();
    wait(&mut h, 0.5);
    assert!(stacked(&h).is_empty());
}

#[test]
fn the_same_words_again_count_instead_of_stacking() {
    let h = with(&[("toast", "Saved"), ("toast", "Saved")]);
    assert_eq!(stacked(&h), ["Saved"]);
}

/// Drags the toast saying `words` sideways by `by`, then lets go.
fn swipe(h: &mut Harness<'static, MareaderApp>, words: &str, by: f32) {
    let from = h.get_by_label(words).rect().center();
    h.hover_at(from);
    h.run_steps(1);
    h.drag_at(from);
    h.run_steps(1);
    for step in 1..=4 {
        h.hover_at(from + egui::vec2(by * step as f32 / 4.0, 0.0));
        h.run_steps(1);
    }
}

#[test]
fn a_toast_swiped_away_goes_and_a_short_pull_springs_back() {
    let mut h = with(&[("toast", "Copied")]);
    let rest = h.get_by_label("Copied").rect();
    // Slow enough not to be a flick.
    swipe(&mut h, "Copied", 32.0);
    let pulled = h.get_by_label("Copied").rect();
    assert!(pulled.left() > rest.left() + 12.0, "it follows the pointer");
    h.drop_at(pulled.center());
    wait(&mut h, 0.5);
    assert_eq!(stacked(&h), ["Copied"], "a short pull springs back");
    let back = h.get_by_label("Copied").rect();
    assert!((back.left() - rest.left()).abs() < 1.0, "{back:?}");
    swipe(&mut h, "Copied", 160.0);
    h.drop_at(rest.center() + egui::vec2(160.0, 0.0));
    wait(&mut h, 0.5);
    assert!(stacked(&h).is_empty());
}

#[test]
fn hovering_a_stack_fans_out_the_other_notifications() {
    let mut h = with(&[("toast", "Saved notes"), ("toast", "Copied link")]);
    assert!(h.query_by_label("Saved notes").is_none());
    h.get_by_label("Copied link").hover();
    h.run_steps(20);
    let back = h.get_by_label("Saved notes").rect();
    let front = h.get_by_label("Copied link").rect();
    assert!(
        back.bottom() <= front.top(),
        "the cards fan apart: {back:?}, {front:?}"
    );
    assert!(h.query_by_label("Clear all").is_some());
}

#[test]
fn clicking_a_stack_opens_a_list_of_every_toast() {
    let mut h = with(&[("toast", "Saved notes"), ("toast", "Copied link")]);
    h.get_by_label("Copied link").click();
    h.run_steps(3);
    assert!(h.state().toasts().1, "the click opens it");
    assert!(h.query_by_label("Notifications").is_some());
    assert!(h.query_by_label("Saved notes").is_some());
    assert_eq!(h.query_all_by_label("Dismiss").count(), 2);
    h.get_by_label("Show less").click();
    h.run_steps(3);
    assert!(!h.state().toasts().1);
    h.hover_at(egui::pos2(300.0, 300.0));
    h.run_steps(3);
    h.get_by_label("Show all notifications").click();
    h.run_steps(3);
    assert!(h.state().toasts().1, "the strip opens it too");
    let away = egui::pos2(300.0, 300.0);
    h.drag_at(away);
    h.run_steps(1);
    h.drop_at(away);
    h.run_steps(3);
    assert!(!h.state().toasts().1, "a click elsewhere closes it");
    assert_eq!(stacked(&h).len(), 2, "closing the list keeps the toasts");
}

#[test]
fn an_open_stack_holds_its_toasts() {
    let mut h = with(&[("toast", "Saved notes"), ("toast", "Copied link")]);
    h.run_steps(2);
    h.state_mut().set_option("toasts", "open").ok();
    h.hover_at(egui::pos2(300.0, 300.0));
    wait(&mut h, 8.0);
    assert_eq!(stacked(&h).len(), 2);
    h.get_by_label("Clear all").click();
    wait(&mut h, 0.5);
    assert!(stacked(&h).is_empty());
    assert!(!h.state().toasts().1);
}

#[test]
fn a_refused_split_says_why() {
    use mareader_workspace::SplitAxis;
    let mut h = with(&[("panes", "4")]);
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(Command::Split(
            SplitAxis::Horizontal,
        )));
    h.run_steps(3);
    let toast = h.state().toast().map(|t| t.message.clone());
    assert!(toast.is_some_and(|m| m.contains("4 panes")));
}

fn lights(h: &Harness<'static, MareaderApp>) -> mareader_ui_egui::platform::NativeLights {
    h.state().native_lights()
}

#[test]
fn an_open_sidebar_keeps_the_lights_while_the_bar_hides() {
    let mut h = with(&[("platform", "mac"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(700.0, 500.0));
    wait(&mut h, 1.0);
    assert!(!h.state().bar_shown());
    assert_eq!((lights(&h).shift, lights(&h).opacity), (0.0, 1.0));
}

#[test]
fn closing_the_sidebar_slides_the_lights_away() {
    let mut h = with(&[("platform", "mac"), ("sidebar", "outline")]);
    h.hover_at(egui::pos2(700.0, 500.0));
    wait(&mut h, 1.0);
    h.state_mut().queue(mareader_ui_egui::ui_command::Sent::Ui(
        UiCommand::CloseSidebar,
    ));
    wait(&mut h, 0.15);
    let mid = lights(&h);
    assert!(
        mid.shift < 0.0 && mid.shift > -mareader_ui_egui::sidebar::WIDTH,
        "{mid:?}"
    );
    assert_eq!(mid.opacity, 1.0, "they ride the rail, no fade");
    wait(&mut h, 0.5);
    assert_eq!(lights(&h).opacity, 0.0, "gone with it; the bar is hidden");
}

#[test]
fn without_a_sidebar_the_lights_come_with_the_bar() {
    let mut h = with(&[("platform", "mac")]);
    h.hover_at(egui::pos2(700.0, 500.0));
    wait(&mut h, 1.0);
    assert_eq!(lights(&h).opacity, 0.0);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert_eq!((lights(&h).shift, lights(&h).opacity), (0.0, 1.0));
}

#[test]
fn in_overlay_mode_the_lights_slide_in_with_the_rail_only() {
    let mut h = with(&[("platform", "mac"), ("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.state().bar_shown());
    assert_eq!(lights(&h).opacity, 0.0, "the bar never hosts them here");
    h.hover_at(egui::pos2(2.0, 400.0));
    wait(&mut h, 0.12);
    h.hover_at(egui::pos2(100.0, 400.0));
    let mid = lights(&h);
    assert!(mid.shift < 0.0 && mid.opacity == 1.0, "{mid:?}");
    wait(&mut h, 0.5);
    assert_eq!((lights(&h).shift, lights(&h).opacity), (0.0, 1.0));
}

#[test]
fn the_overlay_bar_leaves_no_gutter_for_lights() {
    let mut h = with(&[("platform", "mac"), ("float", "true")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.get_by_label("Library").rect().left() < 20.0);
    let mut h = with(&[("platform", "mac")]);
    h.hover_at(egui::pos2(600.0, 20.0));
    wait(&mut h, 0.5);
    assert!(h.get_by_label("Toggle sidebar").rect().left() >= 80.0);
}

#[test]
fn the_floating_rail_slides_in_solid_not_fading() {
    let mut h = with(&[("float", "true"), ("platform", "gnome")]);
    h.hover_at(egui::pos2(2.0, 400.0));
    wait(&mut h, 0.15);
    let shift = h.state().sidebar().offset().unwrap_or(0.0);
    assert!(shift < -20.0, "still on its way in: {shift}");
    let image = h.render().map_err(|e| e.to_string());
    let Ok(image) = image else {
        return;
    };
    // Inside the visible rail: its own surface, not blended with the page.
    let x = (mareader_ui_egui::sidebar::WIDTH + shift - 10.0) as u32;
    let [r, g, b, _] = image.get_pixel(x, 400).0;
    let [pr, pg, pb, _] = image.get_pixel(900, 400).0;
    assert_ne!((r, g, b), (pr, pg, pb), "the rail shows");
    let settled = {
        h.hover_at(egui::pos2(100.0, 400.0));
        wait(&mut h, 0.6);
        h.render()
            .map(|i| i.get_pixel(278, 400).0)
            .unwrap_or([r, g, b, 255])
    };
    assert_eq!([r, g, b], settled[..3], "same colour mid-slide as at rest");
}

#[test]
fn saving_the_look_adds_a_preset_that_is_picked() {
    let mut h = with(&[("preset", "sepia")]);
    open_appearance(&mut h);
    h.get_by_label("Save current look").click();
    h.run_steps(2);
    h.event(egui::Event::Text("Night owl".into()));
    h.run_steps(1);
    h.key_press(egui::Key::Enter);
    h.run_steps(2);
    let s = h.state().settings();
    let saved = s
        .user_presets
        .first()
        .map(|p| (p.name.as_str(), p.appearance));
    assert_eq!(saved, Some(("Night owl", s.appearance)));
    // It lands under Custom, and as the look in use it is the picked one.
    assert!(h.query_all_by_label("Custom").count() > 0, "named section");
    let picked = h.get_by_label("Night owl").accesskit_node().toggled();
    assert_eq!(picked, Some(egui::accesskit::Toggled::True));
    assert!(
        h.query_by_label("Save current look").is_some(),
        "the form closed"
    );
}

#[test]
fn a_user_preset_shows_in_its_section_and_deletes_from_its_card() {
    let mut h = with(&[("save_preset", "Owl/Evening")]);
    open_appearance(&mut h);
    h.get_by_label("Evening");
    assert!(h.query_by_label("Delete Sepia").is_none(), "built-ins stay");
    h.get_by_label("Owl").hover();
    h.run_steps(2);
    h.get_by_label("Delete Owl").click();
    h.run_steps(2);
    assert!(h.state().settings().user_presets.is_empty());
    assert!(h.query_by_label("Owl").is_none());
}

#[test]
fn a_hue_landmark_tints_at_a_gentle_strength() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    h.get_by_label("Rose hue").click();
    h.run_steps(2);
    let a = h.state().settings().appearance;
    assert_eq!((a.tint_hue, a.tint_strength), (14, 18));
}

#[test]
fn the_appearance_popover_grows_to_its_cap_then_scrolls() {
    // Compact by default, so enough sections of looks to overflow it.
    let looks: Vec<_> = (0..12)
        .map(|i| ("save_preset", format!("L{i}/S{}", i % 4)))
        .collect();
    let looks: Vec<_> = looks.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut h = with(&looks);
    open_appearance(&mut h);
    h.run_steps(4);
    let id = egui::Id::new(mareader_ui_egui::titlebar::APPEARANCE_POPOVER);
    let rect = h
        .ctx
        .memory(|m| m.area_rect(id))
        .unwrap_or(egui::Rect::NOTHING);
    // 70 % of 800 points is past the 512 cap; the frame adds a margin.
    let cap = mareader_ui_egui::appearance_menu::MAX_HEIGHT;
    assert!(rect.height() >= cap, "{} tall", rect.height());
    assert!(rect.height() < cap + 40.0, "{} tall", rect.height());
}

#[test]
fn slider_values_never_widen_the_appearance_popover() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    let id = egui::Id::new(mareader_ui_egui::titlebar::APPEARANCE_POPOVER);
    let width = |h: &Harness<'static, MareaderApp>| {
        h.ctx.memory(|m| m.area_rect(id)).map_or(0.0, |r| r.width())
    };
    let mut a = h.state().settings().appearance;
    (a.tint_strength, a.texture_scale, a.noise_intensity) = (0, 25, 0);
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(
            Command::SetAppearance(a),
        ));
    h.run_steps(3);
    let narrow = width(&h);
    (a.tint_strength, a.texture_scale, a.noise_intensity) = (100, 400, 100);
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(
            Command::SetAppearance(a),
        ));
    h.run_steps(3);
    assert!(narrow > 0.0, "the popover is open");
    assert_eq!(
        width(&h),
        narrow,
        "\"400 %\" takes no more room than \"25 %\""
    );
}

#[test]
fn the_popover_offers_pane_modes_only_while_split() {
    let modes = ["Theme per pane", "Texture per pane"];
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    for label in modes {
        assert!(h.query_by_role_and_label(Role::CheckBox, label).is_none());
    }
    let mut h = with(&[("panes", "2")]);
    open_appearance(&mut h);
    // Together at the foot, below the last group's controls.
    let last = h.get_by_label("Animated").rect().bottom();
    for label in modes {
        let top = h.get_by_role_and_label(Role::CheckBox, label).rect().top();
        assert!(top > last, "{label} sits at {top}, above {last}");
    }
}

#[test]
fn clicking_a_group_label_folds_and_unfolds_it() {
    let mut h = harness(MareaderApp::default());
    open_appearance(&mut h);
    for (group, inside) in [("Film grain", "Static"), ("Presets", "Sepia")] {
        h.get_by_label(group).click();
        h.run_steps(2);
        assert!(h.query_by_label(inside).is_none(), "{group} folded");
        h.get_by_label(group).click();
        h.run_steps(2);
        assert!(h.query_by_label(inside).is_some(), "{group} unfolded");
    }
}

#[test]
fn a_preset_section_folds_on_its_own() {
    let mut h = with(&[("save_preset", "Owl/Evening")]);
    open_appearance(&mut h);
    h.get_by_label("Evening").click();
    h.run_steps(2);
    assert!(h.query_by_label("Owl").is_none(), "its cards fold away");
    assert!(h.query_by_label("Sepia").is_some(), "other sections stay");
}

#[test]
fn independent_themes_from_settings_colour_the_other_pane() {
    let mut h = with(&[("panes", "2"), ("settings", "panes")]);
    independent_themes(&mut h);
    h.run_steps(3);
    let app = h.state();
    assert!(app.settings().workspace.independent_themes);
    let (one, two) = match app.panes()[..] {
        [one, two] => (one, two),
        _ => panic!("two panes"),
    };
    assert_eq!(
        app.look(one),
        app.settings().appearance,
        "the front keeps it"
    );
    let own = app.look(two);
    assert_eq!(own.tint_strength, mareader_appearance::PANE_TINT_STRENGTH);
    assert_ne!(own.tint_hue, app.look(one).tint_hue);
}

#[test]
fn a_mode_switch_changes_only_the_active_pane_unless_shared() {
    for shared in [false, true] {
        let flag = if shared { "true" } else { "false" };
        let options = [
            ("panes", "2"),
            ("independent", "true"),
            ("shared_base", flag),
        ];
        let mut h = with(&options);
        // The second pane in front; the first wears the window's look.
        h.state_mut().set_option("focus", "2").ok();
        h.run();
        open_appearance(&mut h);
        h.get_by_label("Dark").click();
        h.run_steps(2);
        let app = h.state();
        let dark = |p| app.look(p).base == mareader_appearance::BaseMode::Dark;
        let (one, two) = (app.panes()[0], app.panes()[1]);
        assert!(dark(two), "the active pane is dark");
        assert_eq!(dark(one), shared, "the other follows only when shared");
        assert_eq!(app.settings().appearance.base.is_dark(), shared);
    }
}

#[test]
fn the_interface_wears_the_look_of_the_pane_split_from() {
    let options = [("panes", "3"), ("independent", "true")];
    let mut h = with(&options);
    let panes = h.state().panes();
    let (one, two) = (panes[0], panes[1]);
    // Its own look turned dark: the window, and so the interface, follow.
    open_appearance(&mut h);
    h.get_by_label("Dark").click();
    h.run_steps(2);
    assert!(h.state().settings().appearance.base.is_dark());
    assert_eq!(h.state().look(one), h.state().settings().appearance);
    // Closed, the eldest left is crowned and the window wears its look.
    let heir = h.state().look(two);
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(
            Command::ClosePane(one),
        ));
    h.run_steps(3);
    assert_eq!(h.state().panes().len(), 2);
    assert_eq!(h.state().settings().appearance, heir, "the heir's look");
}

#[test]
fn closing_to_one_pane_keeps_the_survivor_s_own_look() {
    let mut h = with(&[("panes", "2"), ("independent", "true")]);
    let two = h.state().panes()[1];
    let own = h.state().look(two);
    let one = h.state().panes()[0];
    h.state_mut()
        .queue(mareader_ui_egui::ui_command::Sent::Engine(
            Command::ClosePane(one),
        ));
    h.run_steps(3);
    assert_eq!(h.state().panes(), vec![two]);
    assert_eq!(
        h.state().settings().appearance,
        own,
        "the window took it over"
    );
}

/// Split, the Panes tab opens on the pane looks; the switch is below.
fn independent_themes(h: &mut Harness<'static, MareaderApp>) {
    let switch = "Independent theme for each pane";
    h.get_by_role_and_label(Role::CheckBox, switch)
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, switch).click();
}

#[test]
fn view_menu_exposes_switch_state_and_updates_it_without_closing() {
    use egui::accesskit::Toggled;

    let mut h = with(&[("pinned", "true")]);
    h.get_by_label("View & tools").click();
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Keep title bar shown")
            .accesskit_node()
            .toggled(),
        Some(Toggled::True)
    );
    assert_eq!(
        h.get_by_label("Distraction-free")
            .accesskit_node()
            .toggled(),
        Some(Toggled::False)
    );
    h.get_by_label("Keep title bar shown").click();
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Keep title bar shown")
            .accesskit_node()
            .toggled(),
        Some(Toggled::False)
    );
    h.get_by_label("Keep title bar shown").click();
    h.run_steps(3);
    assert_eq!(
        h.get_by_label("Keep title bar shown")
            .accesskit_node()
            .toggled(),
        Some(Toggled::True)
    );
    assert!(h.query_by_label("Settings").is_some());
    assert!(h.query_by_label("Keyboard shortcuts").is_some());
}
