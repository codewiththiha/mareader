//! The deck's clock and keys.

use super::*;

#[test]
fn the_clock_rounds_up_to_whole_seconds() {
    assert_eq!(clock(0.0), "0:00");
    assert_eq!(clock(0.2), "0:01");
    assert_eq!(clock(72.0), "1:12");
    assert_eq!(clock(-4.0), "0:00");
    assert_eq!(clock(f64::NAN), "0:00");
}

#[test]
fn deck_keys_are_taken_once() {
    let mut input = egui::InputState::default();
    input.events.push(egui::Event::Key {
        key: Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    assert_eq!(Press::read(&mut input), Some(Press::Known));
    assert_eq!(Press::read(&mut input), None);
}
