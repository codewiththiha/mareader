//! Dragging empty space pans and flings; held still in a split, it lifts.

use egui::style::ScrollAnimation;
use egui::{CursorIcon, Id, LayerId, Order, Pos2, Response, Shape, Stroke, Ui, Vec2};

use crate::theme::Tokens;

/// A release this long (s) after the last movement rests, not flings.
const REST: f32 = 0.08;
/// Below this speed (points a second) a fling has stopped.
const STOP: f32 = 20.0;
/// The share of a fling's speed kept each 16 ms.
const DECAY: f32 = 0.94;
/// Held still this long (s) on empty space, the pane lifts.
pub const HOLD: f64 = 1.0;
/// Moving this far (points) turns a hold into a pan.
const SLOP: f32 = 4.0;
/// The ring waits this long (s), so plain clicks never flash it.
const RING_AFTER: f64 = 0.2;
const RING: f32 = 14.0;

#[derive(Clone, Copy, Default)]
struct Grab {
    panning: bool,
    /// Points a second the pages still glide.
    fling: Vec2,
    /// When a press that may lift began.
    held: Option<f64>,
    /// The pane is lifted; the press belongs to the workspace now.
    lifted: bool,
}

/// What a press on empty space is doing this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Grabbed {
    /// Panning or lifted: the press selects nothing.
    pub busy: bool,
    /// The hold ran out here: the pane lifts.
    pub lift: Option<Pos2>,
}

/// Pans while a drag begun on empty space lasts; `can_lift` allows holds.
pub fn pan(ui: &Ui, response: &Response, from_empty: bool, can_lift: bool) -> Grabbed {
    let id = ui.id().with("grab");
    let mut g: Grab = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    let (pressed, dt, speed, still, origin) = ui.input(|i| {
        let p = &i.pointer;
        let dt = i.stable_dt.min(0.1);
        let still = p.time_since_last_movement();
        (p.any_pressed(), dt, p.velocity(), still, p.press_origin())
    });
    let (now, latest, down) = ui.input(|i| (i.time, i.pointer.latest_pos(), i.pointer.any_down()));
    let mut out = Grabbed::default();
    if pressed {
        g.fling = Vec2::ZERO;
        let mine = response.is_pointer_button_down_on();
        g.held = (mine && from_empty && can_lift).then_some(now);
    }
    if g.lifted {
        g.lifted = down;
        g.panning = false;
        ui.data_mut(|d| d.insert_temp(id, g));
        out.busy = true;
        return out;
    }
    if let (Some(start), Some(o)) = (g.held, origin) {
        let moved = latest.is_some_and(|p| p.distance(o) > SLOP);
        if !down || moved {
            g.held = None;
        } else if now - start >= HOLD {
            g.held = None;
            g.lifted = true;
            g.panning = false;
            out = Grabbed {
                busy: true,
                lift: Some(o),
            };
            ui.data_mut(|d| d.insert_temp(id, g));
            return out;
        } else {
            if now - start > RING_AFTER {
                let share = ((now - start - RING_AFTER) / (HOLD - RING_AFTER)) as f32;
                ring(ui, o, share);
            }
            ui.ctx().request_repaint();
        }
    }
    if response.drag_started() && from_empty {
        g.panning = true;
    }
    let panning = g.panning;
    if panning {
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        // The first step makes up the distance before a drag counts.
        let at = response.interact_pointer_pos();
        let delta = match (response.drag_started(), origin, at) {
            (true, Some(o), Some(p)) => p - o,
            _ => response.drag_delta(),
        };
        ui.scroll_with_delta_animation(delta, ScrollAnimation::none());
        if !response.dragged() {
            g.panning = false;
            if still < REST {
                g.fling = speed;
            }
        }
    } else if g.fling != Vec2::ZERO {
        ui.scroll_with_delta_animation(g.fling * dt, ScrollAnimation::none());
        g.fling *= DECAY.powf(dt / 0.016);
        if g.fling.length() < STOP {
            g.fling = Vec2::ZERO;
        }
        ui.ctx().request_repaint();
    }
    ui.data_mut(|d| d.insert_temp(id, g));
    out.busy = panning;
    out
}

/// The hold's countdown: a ring filling clockwise round the press.
fn ring(ui: &Ui, at: Pos2, share: f32) {
    let t = Tokens::get(ui.ctx());
    let layer = LayerId::new(Order::Foreground, Id::new("lift-ring"));
    let painter = ui.ctx().layer_painter(layer);
    painter.circle_filled(at, RING + 4.0, t.surface.gamma_multiply(0.85));
    painter.circle_stroke(at, RING, Stroke::new(3.0, t.line));
    let steps = 40;
    let turn = std::f32::consts::TAU * share.clamp(0.0, 1.0);
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + turn * i as f32 / steps as f32;
            at + Vec2::angled(a) * RING
        })
        .collect();
    painter.add(Shape::line(points, Stroke::new(3.0, t.accent)));
}
