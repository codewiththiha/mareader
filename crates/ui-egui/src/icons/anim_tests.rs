//! Tint preservation and rapid re-trigger tests for animated icons.

use egui::epaint::ClippedShape;
use egui::{
    Color32, Context, Event, Modifiers, PointerButton, Pos2, Rect, Shape, TextureId, Vec2, pos2,
};
use mareader_appearance::{Appearance, BaseMode};

use super::Icon;
use super::anim::{
    Act, FRAME_RATE, MAX_SPEED, Order, announce, current, entering, frame_index, ink_of, order,
    run_state, seed_cached_atlas, trigger_speed,
};
use crate::theme::{self, Tokens};
use crate::widgets;

fn dark_context() -> (Context, Tokens) {
    let ctx = Context::default();
    let appearance = Appearance {
        base: BaseMode::Dark,
        ..Appearance::default()
    };
    let tokens = Tokens::new(&appearance);
    theme::apply(&ctx, &tokens);
    (ctx, tokens)
}

fn click_events(at: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(at),
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
        },
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
        },
    ]
}

fn mesh_colors(shapes: &[ClippedShape], tex: TextureId) -> Vec<Color32> {
    fn walk(shape: &Shape, tex: TextureId, out: &mut Vec<Color32>) {
        match shape {
            Shape::Mesh(mesh) if mesh.texture_id == tex => {
                out.extend(mesh.vertices.iter().map(|v| v.color));
            }
            Shape::Vec(children) => {
                for child in children {
                    walk(child, tex, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in shapes {
        walk(&clipped.shape, tex, &mut out);
    }
    out
}

#[test]
fn frame_times_are_quantised_and_bounded_by_the_run() {
    assert_eq!(frame_index(0.0, 0.56), 0);
    assert_eq!(frame_index(1.0 / FRAME_RATE - 1.0e-4, 0.56), 0);
    assert_eq!(frame_index(1.0 / FRAME_RATE + 1.0e-4, 0.56), 1);
    assert_eq!(frame_index(0.559, 0.56), 33);
    assert_eq!(frame_index(8.0, 0.56), 33);
    assert!(
        mareader_animated_icons::all()
            .all(|pack| { frame_index(pack.duration(), pack.duration()) <= 50 })
    );
}

#[test]
fn each_act_waits_for_its_own_event() {
    assert!(!Act::Still.fires(true, true));
    assert!(Act::Hover.fires(true, false));
    assert!(!Act::Hover.fires(false, true));
    assert!(Act::Click.fires(false, true));
    assert!(!Act::Click.fires(true, false));
}

#[test]
fn a_live_run_restarts_instead_of_queuing() {
    assert_eq!(order(Act::Click, false, true, false), Order::Start);
    assert_eq!(order(Act::Click, false, true, true), Order::Restart);
    assert_eq!(order(Act::Hover, true, false, true), Order::Restart);
    assert_eq!(order(Act::Hover, false, true, false), Order::None);
}

#[test]
fn every_generated_pack_is_reachable() {
    assert_eq!(mareader_animated_icons::all().len(), 104);
    for pack in mareader_animated_icons::all() {
        assert!(!pack.id().is_empty());
        assert!(pack.frame_count() > 0);
    }
}

#[test]
fn the_nearest_announcement_overrides_its_menu() {
    let ctx = Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(100.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        assert_eq!(current(ui.ctx()), Act::Still);
        let menu = announce(ui, Act::Hover);
        assert_eq!(current(ui.ctx()), Act::Hover);
        {
            let control = announce(ui, Act::Click);
            assert_eq!(current(ui.ctx()), Act::Click);
            drop(control);
        }
        assert_eq!(current(ui.ctx()), Act::Hover);
        drop(menu);
        assert_eq!(current(ui.ctx()), Act::Still);
    });
    out.textures_delta.clear();
}

#[test]
fn a_move_starts_on_a_reaching_not_a_resting() {
    assert!(entering(true, false, false), "reached");
    assert!(!entering(true, true, false), "already resting on it");
    assert!(entering(true, true, true), "back after its menu closed");
    assert!(!entering(false, false, false));
    assert_eq!(order(Act::Hover, true, false, false), Order::Start);
    assert_eq!(order(Act::Click, false, true, true), Order::Restart);
    assert_eq!(order(Act::Hover, false, false, true), Order::None);
}

#[test]
fn trigger_speed_scales_with_rapid_cadence_and_bounds_extremes() {
    assert_eq!(trigger_speed(0.5, None), 1.0);
    assert_eq!(trigger_speed(0.5, Some(0.5)), 1.0);
    assert_eq!(trigger_speed(0.5, Some(1.2)), 1.0);
    assert_eq!(trigger_speed(0.5, Some(-0.1)), 1.0);
    assert_eq!(trigger_speed(0.5, Some(f64::NAN)), 1.0);
    assert!((trigger_speed(0.5, Some(0.25)) - 2.0).abs() < 1.0e-5);
    assert_eq!(trigger_speed(0.5, Some(0.05)), MAX_SPEED);
    assert_eq!(trigger_speed(0.5, Some(0.0)), MAX_SPEED);
}

#[test]
fn ink_of_matches_button_tint_for_selected_override_and_fallback() {
    let (ctx, tokens) = dark_context();
    assert_ne!(tokens.accent, tokens.ink);
    assert_ne!(tokens.danger, tokens.ink);
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
        ..Default::default()
    };
    let mut out = ctx.run_ui(raw, |ui| {
        let cell = Rect::from_min_size(pos2(8.0, 8.0), Vec2::splat(24.0));
        let resp = ui.interact(cell, ui.id().with("btn"), egui::Sense::click());
        assert_eq!(ink_of(ui, &resp, true), tokens.accent);
        assert_eq!(ink_of(ui, &resp, false), tokens.ink);
        ui.scope(|ui| {
            ui.visuals_mut().override_text_color = Some(tokens.danger);
            assert_eq!(ink_of(ui, &resp, false), tokens.danger);
            assert_eq!(ink_of(ui, &resp, true), tokens.accent);
        });
        ui.scope(|ui| {
            let fallback = Color32::from_rgb(0x12, 0x34, 0x56);
            ui.visuals_mut().override_text_color = None;
            ui.visuals_mut().widgets.inactive.fg_stroke.color = fallback;
            assert_eq!(ink_of(ui, &resp, false), fallback);
        });
    });
    out.textures_delta.clear();
}

#[test]
fn selected_icon_button_animates_in_accent_tint() {
    let (ctx, tokens) = dark_context();
    let tex_id = seed_cached_atlas(&ctx, Icon::Pin, tokens.icon);
    assert!(tex_id.is_some(), "Pin should have an animated pack");
    let Some(tex_id) = tex_id else {
        return;
    };
    let mut at = pos2(20.0, 20.0);
    let away = pos2(190.0, 190.0);
    let mut mid_colors = Vec::new();
    for (frame, time) in [0.0f64, 0.02, 0.05].into_iter().enumerate() {
        let pointer = if frame == 0 { away } else { at };
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
            events: vec![Event::PointerMoved(pointer)],
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let _act = announce(ui, Act::Hover);
            let resp = widgets::icon_button(ui, Icon::Pin, "Dock sidebar", true);
            at = resp.rect.center();
        });
        if frame == 2 {
            mid_colors = mesh_colors(&out.shapes, tex_id);
        }
        out.textures_delta.clear();
    }
    assert!(
        !mid_colors.is_empty(),
        "selected Pin should paint animated mesh frames while live"
    );
    assert!(
        mid_colors.iter().all(|&color| color == tokens.accent),
        "selected Pin must keep accent tint while animating, got {mid_colors:?}"
    );
}

#[test]
fn custom_override_icon_button_animates_in_custom_tint() {
    let (ctx, tokens) = dark_context();
    let tex_id = seed_cached_atlas(&ctx, Icon::Remove, tokens.icon);
    assert!(tex_id.is_some(), "Remove should have an animated pack");
    let Some(tex_id) = tex_id else {
        return;
    };
    let mut at = pos2(20.0, 20.0);
    let mut mid_colors = Vec::new();
    for (frame, time) in [0.0f64, 0.02, 0.05].into_iter().enumerate() {
        let events = if frame == 1 {
            click_events(at)
        } else {
            vec![Event::PointerMoved(pos2(190.0, 190.0))]
        };
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.scope(|ui| {
                ui.visuals_mut().override_text_color = Some(tokens.danger);
                let resp = widgets::icon_button(ui, Icon::Remove, "Remove", false);
                at = resp.rect.center();
            });
        });
        if frame == 2 {
            mid_colors = mesh_colors(&out.shapes, tex_id);
        }
        out.textures_delta.clear();
    }
    assert!(
        !mid_colors.is_empty(),
        "custom-tinted Remove should paint animated mesh frames while live"
    );
    assert!(
        mid_colors.iter().all(|&color| color == tokens.danger),
        "custom-tinted Remove must keep danger tint while animating, got {mid_colors:?}"
    );
}

#[test]
fn rapid_clicks_restart_immediately_and_accelerate_to_cadence() {
    let (ctx, tokens) = dark_context();
    assert!(seed_cached_atlas(&ctx, Icon::ZoomIn, tokens.icon).is_some());
    let mut at = pos2(20.0, 20.0);
    let mut states = Vec::new();
    let steps = [
        (0.00f64, false),
        (0.02, true),
        (0.22, true),
        (0.43, false),
        (0.44, true),
        (1.50, true),
    ];
    for (time, click) in steps {
        let events = if click {
            click_events(at)
        } else {
            vec![Event::PointerMoved(pos2(190.0, 190.0))]
        };
        let raw = egui::RawInput {
            time: Some(time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(200.0))),
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            let resp = widgets::icon_button(ui, Icon::ZoomIn, "Zoom in", false);
            at = resp.rect.center();
            let state = ui
                .push_id(("icon-button", "Zoom in"), |ui| {
                    run_state(ui, Icon::ZoomIn, "Zoom in")
                })
                .inner;
            states.push(state);
        });
        out.textures_delta.clear();
    }
    // First click starts at normal 1.0x speed.
    assert_eq!(states[1], Some((0.02, 1.0)));
    // Second click 0.20s later restarts immediately (not queued) and speeds up.
    assert_eq!(states[2], Some((0.22, MAX_SPEED)));
    // Accelerated run (0.50 / 2.5 = 0.20s) has already landed by 0.43s.
    assert!(states[3].is_some_and(|(start, _)| start < 0.0));
    // Third click right after landing stays cadence-accelerated (> 2.0x).
    let third_ok = states[4].is_some_and(|(start, speed)| {
        (start - 0.44).abs() < 1.0e-6 && speed > 2.0 && speed <= MAX_SPEED
    });
    assert!(third_ok, "expected accelerated burst, got {:?}", states[4]);
    // After an idle pause past pack duration, playback returns to normal 1.0x.
    assert_eq!(states[5], Some((1.50, 1.0)));
}
