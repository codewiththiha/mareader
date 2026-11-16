//! UI integration tests for Settings › Updates and the startup update modal.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::updates::Status;

use crate::common::reader;

#[test]
fn macos_updates_tab_shows_notify_switch_without_auto_update() {
    let mut h = reader(&[("platform", "mac"), ("settings", "updates")]);
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Notify about updates")
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Auto-update")
            .is_none()
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Check for updates")
            .is_some()
    );
    assert!(h.state().settings().updates.check_on_startup);
    h.get_by_role_and_label(Role::CheckBox, "Notify about updates")
        .click();
    h.run_steps(2);
    assert!(!h.state().settings().updates.check_on_startup);
}

#[test]
fn windows_updates_tab_shows_auto_update_and_automatic_check_switches() {
    let mut h = reader(&[("platform", "windows"), ("settings", "updates")]);
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Auto-update")
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Check for updates automatically")
            .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::Button, "Check for updates")
            .is_some()
    );
    h.get_by_role_and_label(Role::CheckBox, "Auto-update")
        .click();
    h.run_steps(2);
    assert!(!h.state().settings().updates.auto_update);
    h.get_by_role_and_label(Role::CheckBox, "Check for updates automatically")
        .click();
    h.run_steps(2);
    assert!(!h.state().settings().updates.check_on_startup);
}

#[test]
fn macos_startup_modal_shows_summary_and_dismisses_on_download() {
    let mut h = reader(&[("platform", "mac"), ("mock_update", "mac:notify")]);
    assert!(h.query_by_label("Update available").is_some());
    assert!(h.query_by_label("New").is_some());
    assert!(h.query_by_label("Fixes").is_some());
    assert!(
        h.query_by_role_and_label(Role::Button, "Download update")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Download update")
        .click();
    h.run_steps(3);
    assert!(h.query_by_label("Update available").is_none());
}

#[test]
fn windows_startup_modal_runs_update_and_restart_flow() {
    let mut h = reader(&[("platform", "windows"), ("mock_update", "windows:install")]);
    assert!(h.query_by_label("Update available").is_some());
    h.get_by_role_and_label(Role::Button, "Update & Restart")
        .click();
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::Button, "Restart now")
            .is_some()
    );
    h.get_by_role_and_label(Role::Button, "Restart now").click();
    h.run_steps(3);
    assert!(h.query_by_label("Update available").is_none());
    assert_eq!(h.state().session().updates().status(), &Status::UpToDate);
}

#[test]
fn silent_auto_update_stages_quietly_without_opening_modal() {
    let h = reader(&[("platform", "gnome"), ("mock_update", "linux:staged")]);
    assert!(h.query_by_label("Update available").is_none());
    assert!(matches!(
        h.state().session().updates().status(),
        Status::ReadyToRestart { silent: true, .. }
    ));

    let settings = reader(&[
        ("platform", "gnome"),
        ("mock_update", "linux:staged"),
        ("settings", "updates"),
    ]);
    assert!(
        settings
            .query_by_role_and_label(Role::Button, "Restart to update")
            .is_some()
    );
}
