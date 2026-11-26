//! Toggling the fixed-width sidebar from its edge.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::sidebar::WIDTH;

use super::shell::with;

type H = Harness<'static, MareaderApp>;

fn button(h: &mut H, at: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

#[test]
fn clicking_the_edge_toggles_persistent_sidebar_state() {
    let mut h = with(&[]);
    let before = h.state().pane_rects()[0].1;
    h.hover_at(pos2(2.0, 400.0));
    h.run_steps(30);
    assert!(h.state().sidebar().is_open());
    assert!(!h.state().sidebar().is_docked());
    assert_eq!(h.state().pane_rects()[0].1, before);
    let edge = pos2(WIDTH - 3.0, 400.0);
    h.hover_at(edge);
    h.run_steps(45);
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::Default
    );
    assert!(h.query_by_label("Keep sidebar open at edge").is_some());
    assert!(h.query_by_label("Click to keep sidebar open").is_some());
    button(&mut h, edge, true);
    button(&mut h, edge, false);
    h.run_steps(45);
    assert!(h.state().sidebar().is_open());
    assert!(h.state().sidebar().is_docked());
    assert!(h.state().pane_rects()[0].1.left() > before.left());
    h.hover_at(pos2(800.0, 400.0));
    h.run_steps(45);
    assert!(h.state().sidebar().is_open(), "it stays open after leaving");
    h.hover_at(edge);
    h.run_steps(45);
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::Default
    );
    assert!(h.query_by_label("Close sidebar at edge").is_some());
    assert!(h.query_by_label("Click to close sidebar").is_some());
    button(&mut h, edge, true);
    button(&mut h, edge, false);
    h.run_steps(45);
    assert!(!h.state().sidebar().is_open());
    assert!(!h.state().sidebar().is_docked());
    assert_eq!(h.state().pane_rects()[0].1, before);
}

#[test]
fn dragging_the_edge_does_not_resize_or_close_the_sidebar() {
    let mut h = with(&[("sidebar", "outline")]);
    h.run_steps(30);
    let pane = h.state().pane_rects()[0].1;
    let start = pos2(WIDTH - 3.0, 400.0);
    let end = pos2(WIDTH - 60.0, 400.0);
    h.hover_at(start);
    h.run_steps(2);
    button(&mut h, start, true);
    h.hover_at(end);
    h.run_steps(4);
    button(&mut h, end, false);
    h.run_steps(8);
    assert!(h.state().sidebar().is_open());
    assert!(h.state().sidebar().is_docked());
    assert_eq!(h.state().pane_rects()[0].1, pane);
}

#[test]
fn hovering_changes_the_edge_pixels_and_leaving_restores_them() -> Result<(), String> {
    let mut h = with(&[("sidebar", "outline")]);
    h.hover_at(pos2(100.0, 400.0));
    h.run_steps(30);
    let plain = h.render()?.get_pixel(285, 400).0;
    h.hover_at(pos2(285.0, 400.0));
    h.run_steps(4);
    let lit = h.render()?.get_pixel(285, 400).0;
    assert_ne!(plain, lit, "hover changes the edge surface");
    h.hover_at(pos2(100.0, 400.0));
    h.run_steps(4);
    let restored = h.render()?.get_pixel(285, 400).0;
    assert_eq!(plain, restored, "the highlight leaves with the pointer");
    Ok(())
}

#[test]
fn edge_tooltip_waits_then_tracks_the_pointer_in_both_modes() {
    for docked in [false, true] {
        let options = if docked {
            vec![("sidebar", "outline")]
        } else {
            vec![]
        };
        let mut h = with(&options);
        if !docked {
            h.hover_at(pos2(2.0, 400.0));
        }
        h.run_steps(30);
        let hint = if docked {
            "Click to close sidebar"
        } else {
            "Click to keep sidebar open"
        };
        h.hover_at(pos2(WIDTH - 3.0, 300.0));
        h.run_steps(5);
        assert!(h.query_by_label(hint).is_none());
        h.run_steps(45);
        let before = h.get_by_label(hint).rect();
        h.hover_at(pos2(WIDTH - 3.0, 450.0));
        h.run_steps(3);
        let after = h.get_by_label(hint).rect();
        assert!(
            (after.top() - before.top() - 150.0).abs() < 5.0,
            "tooltip should follow pointer: {before:?} to {after:?}"
        );
    }
}
