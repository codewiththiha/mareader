//! Books the AI is organizing: a pass of light and a caption, no badge.

use std::collections::BTreeSet;
use std::f32::consts::TAU;

use egui::{Color32, CornerRadius, FontId, Pos2, Rect, Response, Ui, pos2};
use mareader_library::BookId;

use super::{Cx, face, line};
use crate::library::{Bound, Proposal};
use crate::motion;
use mareader_library::picks::Pick;

/// A pass of light every so often, each card a beat after the last.
const PERIOD: f64 = 2.6;
const PASS: f64 = 1.15;
const BEAT: f64 = 0.09;
// Each pass leaves a rest before the next.
const _: () = assert!(PASS < PERIOD);
/// How many cards the wave runs through before it starts over.
const WAVE: usize = 24;

/// The run as the cards see it.
pub(super) struct Held<'a> {
    pub books: &'a BTreeSet<BookId>,
    /// The plan under review, once it's in.
    pub plan: Option<&'a Proposal>,
    /// 0 to 1 just after the plan comes in, else 1.
    pub settle: f32,
}

impl Held<'_> {
    pub fn contains(&self, id: &BookId) -> bool {
        self.books.contains(id)
    }

    /// Whether a run is working: covers keep moving.
    pub fn working(&self) -> bool {
        !self.books.is_empty() && self.plan.is_none()
    }
}

/// The pass of light over a held book's `face`.
pub(super) fn cover(ui: &Ui, face_at: Rect, round: CornerRadius, id: BookId, cx: &Cx<'_>) {
    if !cx.held.contains(&id) || !motion::enabled(ui.ctx()) {
        return;
    }
    let at = match cx.held.plan {
        // All at once, as the plan lands.
        Some(_) if cx.held.settle < 1.0 => Some(ease(cx.held.settle)),
        Some(_) => None,
        None => {
            let order = cx.sequence.iter().position(|p| *p == Pick::Book(id));
            let order = order.unwrap_or(0) % WAVE;
            let now = ui.input(|i| i.time) - order as f64 * BEAT;
            let local = now.rem_euclid(PERIOD);
            (local < PASS).then(|| ease((local / PASS) as f32))
        }
    };
    if let Some(at) = at {
        face::sweep(ui.painter(), face_at, round, at, 1.0, cx.t);
    }
}

/// A held book's caption at `at`, `width` wide; its height, if held.
pub(super) fn caption(ui: &Ui, id: BookId, at: Pos2, width: f32, cx: &Cx<'_>) -> Option<f32> {
    let (said, color) = said(id, cx)?;
    let p = ui.painter();
    let font = FontId::proportional(12.0);
    if !cx.held.working() {
        return Some(line(p, &said, font, color, (at.x, at.y), width));
    }
    let g = p.layout_no_wrap(said, font, color);
    let size = g.size();
    p.galley(at, g, color);
    if size.x + DOTS_W < width {
        dots(ui, pos2(at.x + size.x + 3.5, at.y + size.y * 0.72), color);
    }
    Some(size.y)
}

/// How wide [`dots`] are.
pub(crate) const DOTS_W: f32 = 13.0;

/// Three dots from `start`, breathing in turn while a run works.
pub(crate) fn dots(ui: &Ui, start: Pos2, color: Color32) {
    let moving = motion::enabled(ui.ctx());
    let now = ui.input(|i| i.time) as f32;
    for i in 0..3 {
        let wave = (now * TAU / 1.2 - i as f32 * 0.9).sin() * 0.5 + 0.5;
        let share = if moving { 0.3 + 0.7 * wave } else { 1.0 };
        let dot = start + egui::vec2(i as f32 * 4.5, 0.0);
        ui.painter()
            .circle_filled(dot, 1.25, color.gamma_multiply(share));
    }
    if moving {
        ui.ctx().request_repaint();
    }
}

/// What a held book's caption says, and in what colour.
pub(super) fn said(id: BookId, cx: &Cx<'_>) -> Option<(String, Color32)> {
    if !cx.held.contains(&id) {
        return None;
    }
    let Some(plan) = cx.held.plan else {
        return Some(("Finding a shelf".to_owned(), cx.t.accent));
    };
    match plan.bound(cx.model, id) {
        Some(Bound::Shelf(name)) => Some((format!("→ {name}"), cx.t.accent)),
        _ => Some(("Stays where it is".to_owned(), cx.t.muted)),
    }
}

/// Hovered, a held card says what's happening to it.
pub(super) fn tip(resp: &Response, id: BookId, cx: &Cx<'_>) {
    if !cx.held.contains(&id) {
        return;
    }
    let why = match cx.held.plan.map(|plan| plan.bound(cx.model, id)) {
        None => "The AI is finding this book a shelf. It opens as usual, \
                 and moves once you've seen the plan."
            .to_owned(),
        Some(Some(Bound::Shelf(name))) => {
            format!("The plan files it on {name}. Nothing moves until you apply it.")
        }
        Some(_) => "The plan leaves it where it is.".to_owned(),
    };
    resp.clone().on_hover_text(why);
}

/// Slow in, slow out.
fn ease(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// A list row's thumb gets the same pass, smaller.
pub(super) fn thumb(ui: &Ui, face_at: Rect, id: BookId, cx: &Cx<'_>) {
    cover(ui, face_at, face::corners(true, cx.t), id, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pass_eases_from_spine_to_edge() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(near(ease(-1.0), 0.0) && near(ease(0.5), 0.5) && near(ease(2.0), 1.0));
        assert!(ease(0.1) < 0.1 && ease(0.9) > 0.9);
    }
}
