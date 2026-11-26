//! Sidebar toggle and edge-preview behavior.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use mareader_ui_egui::ui_command::UiCommand;

use super::shell::with;

#[test]
fn toggling_a_hovered_sidebar_docks_it_and_keeps_it_open() {
    let mut h = with(&[]);
    let before = h.state().pane_rects()[0].1;
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(30);
    assert!(h.state().sidebar().is_open());
    assert!(!h.state().sidebar().is_docked());
    assert_eq!(
        h.state().pane_rects()[0].1,
        before,
        "hover does not resize the page"
    );
    assert!(h.query_by_label("Dock sidebar").is_none());
    h.get_by_label("Keep sidebar open").click();
    h.run_steps(30);
    assert!(h.state().sidebar().is_docked());
    assert!(h.state().pane_rects()[0].1.left() > before.left());
    h.hover_at(egui::pos2(800.0, 400.0));
    h.run_steps(45);
    assert!(
        h.state().sidebar().is_open(),
        "toggle stays open after leaving"
    );
    h.get_by_label("Close sidebar").click();
    h.run_steps(45);
    assert!(!h.state().sidebar().is_open());
    assert!(!h.state().sidebar().is_docked());
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(15);
    assert!(
        h.state().sidebar().is_open(),
        "hover still works after close"
    );
    assert!(!h.state().sidebar().is_docked());
}

#[test]
fn closing_at_the_edge_does_not_immediately_reopen() {
    let mut h = with(&[]);
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(15);
    assert!(h.state().sidebar().is_open());
    h.state_mut().queue(mareader_ui_egui::ui_command::Sent::Ui(
        UiCommand::CloseSidebar,
    ));
    h.run_steps(40);
    assert!(!h.state().sidebar().is_open());
    h.hover_at(egui::pos2(600.0, 400.0));
    h.run_steps(4);
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(4);
    assert!(h.state().sidebar().is_open());
}

#[test]
fn choosing_a_panel_in_the_preview_does_not_dock_it() {
    let mut h = with(&[]);
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(30);
    h.get_by_role_and_label(Role::Button, "Outline").click();
    h.run_steps(4);
    assert_eq!(
        h.state().sidebar().panel(),
        mareader_ui_egui::sidebar::Panel::Outline
    );
    assert!(!h.state().sidebar().is_docked());
    h.hover_at(egui::pos2(800.0, 400.0));
    h.run_steps(30);
    assert!(!h.state().sidebar().is_open());
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(15);
    assert_eq!(
        h.state().sidebar().panel(),
        mareader_ui_egui::sidebar::Panel::Outline
    );
}

#[test]
fn legacy_overlay_setting_no_longer_changes_the_behavior() {
    for old in ["true", "false"] {
        let mut h = with(&[("float", old)]);
        let before = h.state().pane_rects()[0].1;
        h.hover_at(egui::pos2(600.0, 20.0));
        h.run_steps(8);
        h.get_by_label("Toggle sidebar").click();
        h.run_steps(30);
        assert!(h.state().sidebar().is_docked());
        assert!(h.state().pane_rects()[0].1.left() > before.left());
        h.get_by_label("Close sidebar").click();
        h.run_steps(30);
        h.hover_at(egui::pos2(2.0, 400.0));
        h.run_steps(30);
        assert!(h.state().sidebar().is_open());
        assert!(!h.state().sidebar().is_docked());
        assert_eq!(h.state().pane_rects()[0].1, before);
    }
}

#[test]
fn programmatic_open_promotes_a_preview_and_escape_closes_it() {
    use mareader_ui_egui::sidebar::Panel;
    use mareader_ui_egui::ui_command::Sent;
    let mut h = with(&[]);
    h.hover_at(egui::pos2(2.0, 400.0));
    h.run_steps(15);
    h.state_mut()
        .queue(Sent::Ui(UiCommand::OpenSidebar(Some(Panel::Outline))));
    h.run_steps(30);
    assert!(h.state().sidebar().is_docked());
    assert_eq!(h.state().sidebar().panel(), Panel::Outline);
    h.key_press(egui::Key::Escape);
    h.run_steps(30);
    assert!(!h.state().sidebar().is_open());
    assert!(!h.state().sidebar().is_docked());
}
