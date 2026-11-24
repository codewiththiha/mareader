//! A pane's corner stack of the AI's work.
//!
//! Folded, it is the newest job's ring in the pane's bottom right corner,
//! rising over the page bar while that is up, with up to two more peeking
//! from behind it; where the deck leaves no room there, it moves to the
//! top right, gliding between them. A hover fans it out into a line a job,
//! each saying what it does, with its buttons beside its ring.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Context, FontId, Galley, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use mareader_engine::activity::{End, Key, Row, Stage};
use mareader_workspace::PaneId;

use super::ring::{self, Look, SIDE};
use crate::motion;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

/// The folded stack's name, and its buttons', for screen readers and tests.
pub const STACK: &str = "AI activity";
pub const VIEW: &str = "View";
/// A deck's stop; organizing's says *Pause*.
pub const CANCEL: &str = "Cancel";
pub const DISMISS: &str = "Dismiss";
/// A continuous run's, when it asks whether to carry on here.
pub const CONTINUE: &str = "Continue";
/// The stack's room from the pane's top and sides; above the page bar.
const INSET: f32 = 14.0;
const LIFT: f32 = 12.0;
/// Room left above the page counter, which the stack sits on.
const ON_COUNTER: f32 = 6.0;
/// The least room kept between the stack and the deck.
const KEEP: f32 = 2.0;
/// Room between open lines; how far each folded one peeks out.
const GAP: f32 = 6.0;
const PEEK: f32 = 4.0;
const PEEKS: usize = 2;
/// Seconds to fan out and to fold; a lingering line's last, fading.
const OPEN: f32 = 0.2;
const FADE: f64 = 0.4;

/// A button on a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    View,
    Cancel,
    Dismiss,
    Continue,
}

impl Button {
    /// Its words on `line`: a job names its own stop.
    fn label(self, line: &Line) -> &'static str {
        match self {
            Self::View => VIEW,
            Self::Cancel => line.key.stop_word(),
            Self::Dismiss => DISMISS,
            Self::Continue => CONTINUE,
        }
    }
}

/// One job's line, as the stack shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub key: Key,
    pub said: String,
    pub look: Look,
    pub buttons: &'static [Button],
    pub alpha: f32,
}

impl Line {
    /// `row`'s line at `now`, fading in a lingering line's last moment.
    pub fn of(row: &Row, now: f64) -> Self {
        let a = &row.activity;
        let (said, look, buttons): (_, _, &'static [Button]) = match row.end {
            End::Going if a.asks => (
                a.said.clone(),
                going(a.stage),
                &[Button::Continue, Button::Cancel],
            ),
            End::Going => (a.said.clone(), going(a.stage), &[Button::Cancel]),
            End::Failed => (a.said.clone(), Look::Failed, &[Button::Dismiss]),
            End::Done if a.keeps => (a.done_said(), Look::Done, &[Button::View, Button::Dismiss]),
            End::Done => (a.done_said(), Look::Done, &[]),
        };
        let alpha = row
            .left(now)
            .map_or(1.0, |left| (left / FADE).clamp(0.0, 1.0));
        Self {
            key: a.key,
            said,
            look,
            buttons,
            alpha: alpha as f32,
        }
    }
}

fn going(stage: Stage) -> Look {
    match stage {
        Stage::Queued => Look::Queued,
        Stage::Running(made) => Look::Busy(made),
        Stage::Failed => Look::Failed,
    }
}

/// Where a stack stands: the rect whose corner it takes, and whether that
/// pane's page bar is up.
#[derive(Clone, Copy, Debug)]
pub struct At {
    /// Names the stack across frames; a pane's, so it glides on splitting.
    pub id: Id,
    pub rect: Rect,
    pub bar: bool,
    /// Where the pane's deck drew, which the stack keeps clear of.
    pub taken: Option<Rect>,
}

/// What a frame's stack did: a button pressed, and whether it is hovered.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Out {
    pub pressed: Option<(Button, Key)>,
    pub hovered: bool,
}

/// A laid out line: its words and its full width.
struct Laid {
    galley: Arc<Galley>,
    width: f32,
}

/// The stack of `lines`, oldest first, in `at`'s corner.
pub fn show(ctx: &Context, at: At, lines: &[Line]) -> Out {
    let id = at.id;
    let hovered = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let open = motion::slide(ctx, id.with("open"), f32::from(u8::from(hovered)), OPEN);
    let (spot, top) = spot(at.rect, at.bar, at.taken);
    let folded = glide(ctx, id, spot);
    let room = (folded.right() - at.rect.left() - INSET).max(SIDE);
    let laid: Vec<Laid> = lines.iter().map(|l| lay(ctx, l, room)).collect();
    let widths: Vec<f32> = laid.iter().map(|l| l.width).collect();
    let rects = place(folded, (top, open), &widths);
    let bounds = rects.iter().fold(folded, |all, r| all.union(*r));
    let shown = z_order::area(ctx, Tier::PageCard, id)
        .fixed_pos(bounds.min)
        .constrain(false)
        .show(ctx, |ui| {
            let (_, resp) = ui.allocate_exact_size(bounds.size(), Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, STACK));
            let pressed = paint(ui, id, (lines, &laid, &rects), open);
            Out {
                pressed,
                hovered: resp.contains_pointer(),
            }
        });
    let out = shown.inner;
    ctx.data_mut(|d| d.insert_temp(id, out.hovered));
    if open > 0.0 && open < 1.0 {
        ctx.request_repaint();
    }
    stand(ctx, shown.response.rect);
    out
}

fn standing_id() -> Id {
    Id::new("activity-stack-standing")
}

/// Says a stack stands at `rect` this pass, so toasts rise above it.
fn stand(ctx: &Context, rect: Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let kept = d.get_temp_mut_or_default::<Vec<(u64, Rect)>>(standing_id());
        kept.retain(|(at, _)| at + 1 >= pass);
        kept.push((pass, rect));
    });
}

/// How far toasts at `stage`'s bottom right rise to clear the stacks there.
pub fn toast_lift(ctx: &Context, stage: Rect) -> f32 {
    use crate::toast::{FADE, INSET, WIDTH};
    let pass = ctx.cumulative_pass_nr();
    let kept: Vec<(u64, Rect)> = ctx.data(|d| d.get_temp(standing_id())).unwrap_or_default();
    let left = stage.right() - INSET - WIDTH;
    let under = |r: &Rect| r.right() > left;
    let rise = |r: Rect| stage.bottom() - INSET - r.top() + LIFT;
    let now = kept
        .into_iter()
        .filter(|(at, _)| at + 1 >= pass)
        .map(|(_, r)| r);
    let target = now.filter(under).map(rise).fold(0.0, f32::max);
    let time = crate::motion::secs(ctx, FADE);
    ctx.animate_value_with_time(Id::new("toast-over-stack"), target, time)
}

/// Each line's place, oldest first: the newest in the corner, the rest
/// peeking behind it, or fanned out away from the corner by `open`.
fn place(folded: Rect, (top, open): (bool, f32), widths: &[f32]) -> Vec<Rect> {
    let n = widths.len();
    let step = PEEK + (SIDE + GAP - PEEK) * open;
    let away = if top { 1.0 } else { -1.0 };
    let rect = |(i, full): (usize, &f32)| {
        let back = n.saturating_sub(i + 1) as f32;
        let width = SIDE + (full - SIDE) * open;
        let y = folded.top() + away * back * step;
        Rect::from_min_size(pos2(folded.right() - width, y), vec2(width, SIDE))
    };
    widths.iter().enumerate().map(rect).collect()
}

/// The lines, back to front; the button pressed, if any.
fn paint(
    ui: &mut Ui,
    id: Id,
    (lines, laid, rects): (&[Line], &[Laid], &[Rect]),
    open: f32,
) -> Option<(Button, Key)> {
    let t = Tokens::get(ui.ctx());
    let n = lines.len();
    let mut pressed = None;
    for (i, ((line, l), rect)) in lines.iter().zip(laid).zip(rects).enumerate() {
        let back = n.saturating_sub(i + 1);
        let peek = match back {
            0 => 1.0,
            b if b <= PEEKS => 0.7,
            _ => 0.0,
        };
        let alpha = line.alpha * (peek + (1.0 - peek) * open);
        if alpha <= 0.0 {
            continue;
        }
        ring::paper(ui.painter(), *rect, &t, alpha);
        let cell = Rect::from_min_size(pos2(rect.right() - SIDE, rect.top()), Vec2::splat(SIDE));
        let ring_id = id.with(("made", line.key));
        ring::paint(ui.ctx(), ui.painter(), (cell, ring_id), line.look, alpha);
        if open > 0.0 {
            let row = (*rect, id.with(line.key));
            pressed = words(ui, row, (line, l), open * alpha).or(pressed);
        }
    }
    pressed
}

/// A fanned line's words and buttons, right to left from its ring.
fn words(
    ui: &mut Ui,
    (rect, id): (Rect, Id),
    (line, l): (&Line, &Laid),
    alpha: f32,
) -> Option<(Button, Key)> {
    let t = Tokens::get(ui.ctx());
    let mut right = rect.right() - SIDE;
    let mut pressed = None;
    for &button in line.buttons.iter().rev() {
        let width = button_width(ui.ctx(), button.label(line));
        let at = Rect::from_min_max(
            pos2(right - width, rect.top() + 4.0),
            pos2(right, rect.bottom() - 4.0),
        );
        right -= width + 2.0;
        if press(ui, (at, id.with(button.label(line))), (button, line), alpha) {
            pressed = Some((button, line.key));
        }
    }
    let size = l.galley.rect.size();
    let top = pos2(right - 4.0 - size.x, rect.center().y - size.y / 2.0);
    let painter = ui.painter_at(rect.shrink(1.0));
    painter.galley_with_override_text_color(top, l.galley.clone(), t.ink.gamma_multiply(alpha));
    let row = ui.interact(rect, id, Sense::hover());
    let said = line.said.clone();
    row.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &said));
    pressed
}

/// A quiet button in `at`; true once clicked.
fn press(ui: &mut Ui, (at, id): (Rect, Id), (button, line): (Button, &Line), alpha: f32) -> bool {
    let t = Tokens::get(ui.ctx());
    let resp = ui.interact(at, id, Sense::click());
    let label = button.label(line);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let painter = ui.painter();
    if resp.hovered() {
        painter.rect_filled(at, at.height() / 2.0, t.hover.gamma_multiply(alpha));
    }
    let ink = match button {
        Button::View | Button::Continue => t.accent,
        Button::Cancel | Button::Dismiss => t.muted,
    }
    .gamma_multiply(alpha);
    if button == Button::Dismiss {
        cross(painter, at.center(), Stroke::new(1.5, ink));
    } else {
        let font = FontId::proportional(t.text_small);
        painter.text(at.center(), egui::Align2::CENTER_CENTER, label, font, ink);
    }
    resp.clicked()
}

/// The × of Dismiss, centred at `c`.
fn cross(painter: &egui::Painter, c: Pos2, stroke: Stroke) {
    let r = 3.5;
    painter.line_segment([c + vec2(-r, -r), c + vec2(r, r)], stroke);
    painter.line_segment([c + vec2(-r, r), c + vec2(r, -r)], stroke);
}

fn button_width(ctx: &Context, label: &str) -> f32 {
    if label == DISMISS {
        return 22.0;
    }
    let font = FontId::proportional(Tokens::get(ctx).text_small);
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(label.to_owned(), font, egui::Color32::WHITE));
    galley.rect.width() + 14.0
}

/// `line`'s words, cut to fit `room` with its ring and buttons.
fn lay(ctx: &Context, line: &Line, room: f32) -> Laid {
    let t = Tokens::get(ctx);
    let buttons: f32 = line
        .buttons
        .iter()
        .map(|&b| button_width(ctx, b.label(line)) + 2.0)
        .sum();
    let left = (room - SIDE - buttons - 16.0).max(0.0);
    let font = FontId::proportional(t.text_small);
    let mut job = LayoutJob::simple_singleline(line.said.clone(), font, t.ink);
    job.wrap = TextWrapping::truncate_at_width(left);
    let galley = ctx.fonts_mut(|f| f.layout_job(job));
    let width = SIDE + buttons + galley.rect.width() + 16.0;
    Laid { galley, width }
}

/// The stack's name in `pane`'s corner, which the workspace's borrows.
pub fn id(pane: PaneId) -> Id {
    Id::new(("activity-stack", pane))
}

/// The folded stack in `pane`: the bottom right corner on the page counter,
/// risen over the page bar while `bar`, else the top right where only that
/// is clear of `taken`; true at the top.
fn spot(pane: Rect, bar: bool, taken: Option<Rect>) -> (Rect, bool) {
    use crate::page_bar::{BAR_H, COUNTER_INSET, COUNTER_TOP};
    let lift = if bar {
        BAR_H + LIFT
    } else {
        COUNTER_TOP + ON_COUNTER
    };
    let x = pane.right() - COUNTER_INSET - SIDE;
    let low = Rect::from_min_size(pos2(x, pane.bottom() - lift - SIDE), Vec2::splat(SIDE));
    let high = Rect::from_min_size(pos2(x, pane.top() + INSET), Vec2::splat(SIDE));
    let free = |r: &Rect| taken.is_none_or(|t| !t.intersects(r.expand(KEEP)));
    match (free(&low), free(&high)) {
        (false, true) => (high, true),
        _ => (low, false),
    }
}

/// `to`, eased there from where the stack last stood.
fn glide(ctx: &Context, id: Id, to: Rect) -> Rect {
    let secs = motion::secs(ctx, OPEN);
    let ease = |key: &str, to: f32| ctx.animate_value_with_time(id.with(key), to, secs);
    Rect::from_min_size(pos2(ease("x", to.left()), ease("y", to.top())), to.size())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_sits_bottom_right_or_moves_out_of_the_decks_way() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 250.0));
        let (roomy, top) = spot(pane, false, None);
        assert_eq!(roomy.right_bottom(), pos2(188.0, 210.0), "on the counter");
        assert!(!top);
        let deck = |y: std::ops::RangeInclusive<f32>| Some(Rect::from_x_y_ranges(0.0..=200.0, y));
        let (above, top) = spot(pane, false, deck(150.0..=230.0));
        assert_eq!(above.right_top(), pos2(188.0, 14.0), "the top right");
        assert!(top);
        let (stuck, top) = spot(pane, false, deck(10.0..=240.0));
        assert_eq!(stuck, roomy, "no room anywhere keeps the corner");
        assert!(!top);
    }

    #[test]
    fn folded_lines_peek_and_open_ones_fan_away_from_the_corner() {
        let corner = Rect::from_min_size(pos2(100.0, 200.0), Vec2::splat(SIDE));
        let lines = [120.0, 80.0];
        let folded = place(corner, (false, 0.0), &lines);
        assert_eq!(
            folded.get(1).copied(),
            Some(corner),
            "the newest in the corner"
        );
        let behind = folded.first().map(|r| r.top());
        assert_eq!(behind, Some(200.0 - PEEK));
        let open = place(corner, (false, 1.0), &lines);
        let first = open.first().copied().unwrap_or(Rect::NOTHING);
        assert_eq!(first.top(), 200.0 - SIDE - GAP, "fanned upwards");
        assert_eq!(first.width(), 120.0);
        assert_eq!(first.right(), corner.right(), "lined up at the right");
    }
}
