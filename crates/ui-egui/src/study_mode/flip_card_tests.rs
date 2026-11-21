//! The flip card's turn, swipe and spoken faces.

use egui::{pos2, vec2};

use super::*;

fn pose(angle: f32) -> Pose {
    Pose {
        center: pos2(100.0, 100.0),
        angle,
        tilt: 0.0,
        scale: 1.0,
        depth: 1200.0,
    }
}

#[test]
fn a_card_facing_the_reader_keeps_its_size() {
    assert_eq!(pose(0.0).place(vec2(50.0, -30.0)), pos2(150.0, 70.0));
}

#[test]
fn a_card_on_its_edge_has_no_width() {
    let edge = pose(PI / 2.0);
    let left = edge.place(vec2(-200.0, 0.0));
    let right = edge.place(vec2(200.0, 0.0));
    assert!((right.x - left.x).abs() < 0.01, "{left:?} {right:?}");
}

#[test]
fn a_turning_card_shrinks_its_far_edge() {
    let turned = pose(0.4);
    let far = turned.place(vec2(200.0, 100.0)).y - 100.0;
    let near = turned.place(vec2(-200.0, 100.0)).y - 100.0;
    assert!(far < near, "far {far} near {near}");
}

#[test]
fn a_swipe_steps_past_the_threshold_or_on_a_flick() {
    assert_eq!(settle(-150.0, 0.0, 400.0, 0, 3), 1);
    assert_eq!(settle(150.0, 0.0, 400.0, 1, 3), -1);
    assert_eq!(settle(-40.0, 0.0, 400.0, 0, 3), 0);
    assert_eq!(settle(-40.0, -900.0, 400.0, 0, 3), 1);
    assert_eq!(settle(-40.0, 900.0, 400.0, 0, 3), 0, "a flick back");
}

#[test]
fn a_swipe_never_wraps_past_either_end() {
    assert_eq!(settle(150.0, 0.0, 400.0, 0, 3), 0);
    assert_eq!(settle(-150.0, 0.0, 400.0, 2, 3), 0);
    assert_eq!(band(100.0, 0, 3), 25.0);
    assert_eq!(band(-100.0, 0, 3), -100.0);
    assert_eq!(band(-100.0, 2, 3), -25.0);
}

#[test]
fn the_back_is_heard_once_turned() {
    let faces = Faces {
        word: "gaunt".into(),
        meaning: "very thin".into(),
        class: "adjective".into(),
        ..Faces::default()
    };
    assert_eq!(faces.label(false), format!("gaunt, {TURN_HINT}"));
    assert_eq!(faces.label(true), "very thin · adjective");
}
