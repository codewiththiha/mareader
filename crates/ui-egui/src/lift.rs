//! A lifted pane: its space closes up, and it rides the pointer as a card.

use egui::{
    Align2, Area, Color32, Context, CornerRadius, CursorIcon, Frame, Id, Key, LayerId, Margin,
    Modifiers, Order, Pos2, Rect, RichText, Stroke, StrokeKind, pos2, vec2,
};
use mareader_workspace::{Bounds, LiftTarget, PaneId, lift_target};

use crate::icons::Icon;
use crate::theme::{self, Tokens};

/// How long the card takes to shrink to its carried size.
const SHRINK_SECS: f64 = 0.18;
/// The carried card's size, as a share of the pane's own box.
const CARRIED: f32 = 0.5;

/// A held pane, as plain data: where it was taken from, and when.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lift {
    pub pane: PaneId,
    /// The press that lifted it; the card shrinks about this point.
    pub origin: Pos2,
    /// The pane's box before it was lifted.
    pub home: Rect,
    pub since: f64,
}

/// What the lifted pane did this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifted {
    Held,
    /// Let go off any target, or Escape: it stays put.
    Cancelled,
    Dropped(LiftTarget),
}

/// Draws the lift and reads the release; `rest` is laid out without it.
pub fn show(ctx: &Context, lift: &Lift, rest: &[(PaneId, Rect)], title: &str) -> Lifted {
    let t = Tokens::get(ctx);
    let boxes: Vec<(PaneId, Bounds)> = rest.iter().map(|(id, r)| (*id, bounds(*r))).collect();
    let pointer = ctx.pointer_latest_pos();
    let target = pointer.and_then(|p| lift_target(lift.pane, (p.x, p.y), &boxes));
    ctx.set_cursor_icon(CursorIcon::Grabbing);
    let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("lift")));
    let lit = target.and_then(|target| {
        let (_, rect) = rest.iter().find(|(id, _)| *id == target.pane())?;
        let b = target.predicted(bounds(*rect));
        let lit = Rect::from_min_size(pos2(b.x, b.y), vec2(b.w, b.h)).shrink(6.0);
        painter.rect_filled(lit, 10.0, t.accent.gamma_multiply(0.14));
        painter.rect_stroke(lit, 10.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
        Some((lit, target.describe()))
    });
    card(ctx, lift, pointer.unwrap_or(lift.origin), title, &t);
    if let Some((lit, words)) = lit {
        chip(ctx, (lit.center(), Align2::CENTER_CENTER), words, &t);
    }
    let escape = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    let released = !ctx.input(|i| i.pointer.any_down());
    match (escape, released, target) {
        (true, ..) | (false, true, None) => Lifted::Cancelled,
        (false, true, Some(target)) => Lifted::Dropped(target),
        (false, false, _) => Lifted::Held,
    }
}

/// The pane's box shrunk about the press, then moved with the pointer.
pub fn carried(lift: &Lift, pointer: Pos2, scale: f32) -> Rect {
    let o = lift.origin;
    let min = o + (lift.home.min - o) * scale + (pointer - o);
    Rect::from_min_size(min, lift.home.size() * scale)
}

/// How far the card has shrunk, eased; at once without motion.
fn shrink(ctx: &Context, since: f64) -> f32 {
    if !crate::motion::enabled(ctx) {
        return 1.0;
    }
    let x = ((ctx.input(|i| i.time) - since) / SHRINK_SECS).clamp(0.0, 1.0) as f32;
    if x < 1.0 {
        ctx.request_repaint();
    }
    1.0 - (1.0 - x).powi(3)
}

/// The carried pane: its paper, an accent ring, and what it holds.
fn card(ctx: &Context, lift: &Lift, pointer: Pos2, title: &str, t: &Tokens) {
    let scale = 1.0 - (1.0 - CARRIED) * shrink(ctx, lift.since);
    let rect = carried(lift, pointer, scale);
    let radius = CornerRadius::same(12);
    Area::new(Id::new("lift-card"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .interactable(false)
        .show(ctx, |ui| {
            let painter = ui.painter();
            let shadow = egui::Shadow {
                offset: [0, 18],
                blur: 48,
                spread: 0,
                color: Color32::from_black_alpha(80),
            };
            painter.add(shadow.as_shape(rect, radius));
            painter.rect_filled(rect, radius, t.paper.gamma_multiply(0.92));
            let ring = Stroke::new(2.0, t.accent);
            painter.rect_stroke(rect, radius, ring, StrokeKind::Outside);
            let inner = rect.shrink(12.0);
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                ui.set_clip_rect(rect);
                ui.vertical_centered(|ui| {
                    ui.add_space((inner.height() / 2.0 - 30.0).max(0.0));
                    ui.add(Icon::Book.image(ui, 28.0).tint(t.accent));
                    ui.add_space(6.0);
                    let text = format!("Moving {title}");
                    let rich = RichText::new(text).font(theme::medium(13.0)).color(t.ink);
                    ui.add(egui::Label::new(rich).truncate());
                });
            });
        });
}

/// A pill of words, read aloud too; it never takes the pointer.
fn chip(ctx: &Context, (at, pivot): (Pos2, Align2), words: &str, t: &Tokens) {
    Area::new(Id::new("lift-target"))
        .order(Order::Tooltip)
        .pivot(pivot)
        .fixed_pos(at)
        .interactable(false)
        .show(ctx, |ui| {
            let frame = Frame::new()
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(14.0)
                .inner_margin(Margin::symmetric(10, 5));
            frame.show(ui, |ui| {
                let text = RichText::new(words).font(theme::medium(12.5)).color(t.ink);
                ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
            });
        });
}

fn bounds(r: Rect) -> Bounds {
    Bounds::new(r.left(), r.top(), r.width(), r.height())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_shrinks_about_the_press_and_rides_the_pointer() {
        let lift = Lift {
            pane: PaneId(1),
            origin: pos2(200.0, 100.0),
            home: Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0)),
            since: 0.0,
        };
        // At full size and still, it sits where the pane was.
        assert_eq!(carried(&lift, lift.origin, 1.0), lift.home);
        let half = carried(&lift, pos2(900.0, 400.0), 0.5);
        assert_eq!(half.size(), vec2(300.0, 400.0));
        // The press keeps its place on the card, now under the pointer.
        assert_eq!(half.min, pos2(800.0, 350.0));
    }
}
