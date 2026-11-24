//! The ring a job shows in the stack: it fills as far as the work is done,
//! easing to each step, with a soft pulse at its head while the work goes
//! on; a dot breathes at its top while it waits for its model, it turns red
//! once the work failed, and holds a tick once done.

use egui::{Context, Id, Painter, Pos2, Rect, Shape, Stroke, Vec2, vec2};

use crate::motion;
use crate::theme::Tokens;

/// The side the ring's measures are drawn for; others scale to it.
pub const SIDE: f32 = 30.0;
/// Seconds the fill eases to a new step; one breath at its head.
const EASE: f32 = 0.45;
const BREATH: f64 = 1.6;
/// Seconds a ring of unknown share takes to go round.
const TURN: f64 = 1.2;

/// What a ring shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Look {
    Queued,
    /// At work, with the share done when it is known.
    Busy(Option<f32>),
    Failed,
    Done,
}

/// `made`, eased from where `id` last stood, so each step glides.
pub fn eased(ctx: &Context, id: Id, made: f32) -> f32 {
    ctx.animate_value_with_time(id, made.clamp(0.0, 1.0), motion::secs(ctx, EASE))
}

/// A slow breath, 0 to 1, saying work goes on; still where motion is reduced.
pub fn pulse(ctx: &Context) -> f32 {
    if motion::reduced(ctx) || !motion::enabled(ctx) {
        return 0.6;
    }
    ctx.request_repaint();
    let now = ctx.input(|i| i.time);
    (0.5 - 0.5 * (now * std::f64::consts::TAU / BREATH).cos()) as f32
}

/// The rounded paper under a stack's line, with its shadow.
pub fn paper(painter: &Painter, rect: Rect, t: &Tokens, alpha: f32) {
    let fade = |c: egui::Color32| c.gamma_multiply(alpha);
    let round = rect.height() / 2.0;
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: fade(t.shadow),
    };
    painter.add(shadow.as_shape(rect, round));
    painter.rect_filled(rect, round, fade(t.surface));
    let edge = Stroke::new(1.0, fade(t.line));
    painter.rect_stroke(rect, round, edge, egui::StrokeKind::Inside);
}

/// The ring centred in `cell`, a square; `id` keeps its fill easing.
pub fn paint(ctx: &Context, painter: &Painter, (cell, id): (Rect, Id), look: Look, alpha: f32) {
    let t = Tokens::get(ctx);
    let fade = |c: egui::Color32| c.gamma_multiply(alpha);
    let k = cell.height() / SIDE;
    let (c, r) = (cell.center(), 8.0 * k);
    let line = Stroke::new(2.5 * k, fade(t.accent));
    if look == Look::Failed {
        painter.circle_filled(c, 5.0 * k, fade(t.danger));
        return;
    }
    painter.circle_stroke(c, r, Stroke::new(line.width, fade(t.track)));
    let glow = |at: Pos2| {
        let soft = fade(t.accent).gamma_multiply(0.35 + 0.65 * pulse(ctx));
        painter.circle_filled(at, line.width * 0.9, soft);
    };
    match look {
        Look::Queued => glow(c - vec2(0.0, r)),
        Look::Busy(Some(made)) => {
            let span = eased(ctx, id, made) * std::f32::consts::TAU;
            arc(painter, (c, r), (0.0, span), line);
            glow(c + r * Vec2::angled(span - std::f32::consts::FRAC_PI_2));
        }
        Look::Busy(None) => {
            let from = turn(ctx) * std::f32::consts::TAU;
            arc(painter, (c, r), (from, std::f32::consts::FRAC_PI_2), line);
        }
        Look::Done => {
            arc(painter, (c, r), (0.0, std::f32::consts::TAU), line);
            let marks = [vec2(-3.5, 0.0), vec2(-1.0, 2.5), vec2(3.5, -2.5)];
            let tick = marks.map(|v| c + v * k);
            painter.line(tick.to_vec(), Stroke::new(2.0 * k, fade(t.accent)));
        }
        Look::Failed => {}
    }
}

/// How far round a ring of unknown share is, 0 to 1.
fn turn(ctx: &Context) -> f32 {
    if motion::reduced(ctx) || !motion::enabled(ctx) {
        return 0.0;
    }
    ctx.request_repaint();
    let now = ctx.input(|i| i.time);
    ((now / TURN).fract()) as f32
}

/// An arc of `span` radians from `from`, starting at the top.
fn arc(painter: &Painter, (c, r): (Pos2, f32), (from, span): (f32, f32), stroke: Stroke) {
    let steps = 40;
    let at = |i: usize| {
        let a = from - std::f32::consts::FRAC_PI_2 + span * i as f32 / steps as f32;
        c + r * Vec2::angled(a)
    };
    let points: Vec<_> = (0..=steps).map(at).collect();
    painter.add(Shape::line(points, stroke));
}
