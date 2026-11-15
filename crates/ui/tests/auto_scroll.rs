//! Auto-scroll: Shift+A's drift, its pace and what stops it, headless.

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::layout::ViewMode;
use mareader_ui::pdf_view::AUTO_SPEED;

use crate::common::reader;

fn offset(h: &Harness<'static, MareaderApp>) -> egui::Vec2 {
    let pane = h.state().active_pane();
    h.state().documents().offset(pane).unwrap_or_default()
}

fn drifting(h: &Harness<'static, MareaderApp>) -> bool {
    let pane = h.state().active_pane();
    h.state().documents().drifting(pane).unwrap_or(false)
}

fn toggle(h: &mut Harness<'static, MareaderApp>) {
    h.key_press_modifiers(Modifiers::SHIFT, Key::A);
    h.run_steps(2);
}

#[test]
fn shift_a_drifts_at_its_pace_whatever_the_steps() {
    let mut h = reader(&[("pdf", "sample:40")]);
    toggle(&mut h);
    assert!(drifting(&h), "Shift+A turns auto-scroll on");
    let start = offset(&h).y;
    h.run_steps(60);
    let moved = offset(&h).y - start;
    let pace = AUTO_SPEED;
    assert!(
        (moved - pace).abs() < 4.0,
        "{moved} in a second, not {pace}"
    );
    toggle(&mut h);
    assert!(!drifting(&h), "Shift+A again turns it off");
    let still = offset(&h).y;
    h.run_steps(30);
    assert!((offset(&h).y - still).abs() < 0.5, "it rests once off");
}

#[test]
fn paged_modes_refuse_it_and_say_why() {
    let mut h = reader(&[("pdf", "sample:40")]);
    h.state_mut().queue(Command::View(ViewMode::Single));
    h.run_steps(5);
    toggle(&mut h);
    assert!(!drifting(&h), "no drift in a paged mode");
    let toast = h.state().toast().map(|t| t.message.clone());
    let toast = toast.unwrap_or_default();
    assert!(toast.contains("Auto-scroll"), "toast {toast:?}");
}

#[test]
fn escape_and_a_paged_mode_stop_it() {
    let mut h = reader(&[("pdf", "sample:40")]);
    toggle(&mut h);
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!drifting(&h), "Escape stops it");
    toggle(&mut h);
    assert!(drifting(&h));
    h.state_mut().queue(Command::View(ViewMode::Single));
    h.run_steps(5);
    assert!(!drifting(&h), "a paged mode stops it");
}

#[test]
fn it_stops_at_the_strips_end() {
    let mut h = reader(&[("pdf", "sample:3")]);
    h.state_mut().queue(Command::LastPage);
    h.run_steps(120);
    toggle(&mut h);
    assert!(drifting(&h), "on above the end");
    for _ in 0..1500 {
        if !drifting(&h) {
            break;
        }
        h.run_steps(1);
    }
    assert!(!drifting(&h), "off at the end, at {}", offset(&h).y);
    let end = offset(&h).y;
    h.run_steps(30);
    assert!((offset(&h).y - end).abs() < 0.5, "it stays at the end");
}

#[test]
fn shift_a_still_types_in_a_field() {
    let mut h = reader(&[("pdf", "sample:40")]);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    h.run_steps(3);
    h.key_press_modifiers(Modifiers::SHIFT, Key::A);
    h.run_steps(2);
    assert!(!drifting(&h), "a focused field keeps Shift+A");
}
