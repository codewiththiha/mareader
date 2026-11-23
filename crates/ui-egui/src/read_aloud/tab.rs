//! The player's tab: a toast's words grown out of the pill's top in egui.
//!
//! It is one body with the pill: the same fill and edge, a concave curve
//! where the two meet, the pill's top edge hidden under the join, and its
//! shadow cast before the pill's fill so none falls across the seam.

use std::f32::consts::FRAC_PI_2;

use egui::{Align2, Color32, CornerRadius, Mesh, Painter, Pos2, Rect, Shadow, Shape, Stroke, pos2};

use super::ease_in_out;
use crate::theme::Tokens;
use crate::toast::{IslandNote, island_font};

/// The concave curve where the tab meets the pill, at most.
const FILLET: f32 = 9.0;
/// The tab's top corners, at most.
const ROUND: f32 = 11.0;
/// Points along each quarter curve.
const STEPS: usize = 8;
/// The words wait until the tab has grown this far.
const WORDS_FROM: f32 = 0.55;

/// Where the tab stands on `body`'s top as `note` grows; none while flat.
pub(super) fn rect(body: Rect, note: &IslandNote) -> Option<Rect> {
    let k = ease_in_out(note.shown);
    let tall = note.size.y * k;
    if tall < 1.0 {
        return None;
    }
    let half = note.size.x * (0.7 + 0.3 * k) / 2.0;
    let x = body.center().x;
    Some(Rect::from_min_max(
        pos2(x - half, body.top() - tall),
        pos2(x + half, body.top()),
    ))
}

/// The tab's shadow, cast like the pill's, before the pill's fill.
pub(super) fn shadow(painter: &Painter, tab: Rect, shadow: Shadow) {
    painter.add(shadow.as_shape(tab, top_round(tab)));
}

/// Its top corners round; its foot is the pill's.
fn top_round(tab: Rect) -> CornerRadius {
    let r = ROUND.min(tab.height()).min(tab.width() / 2.0) as u8;
    CornerRadius {
        nw: r,
        ne: r,
        sw: 0,
        se: 0,
    }
}

/// The body and edge over the pill's top edge, then the words.
pub(super) fn body(painter: &Painter, tab: Rect, note: &IslandNote, t: &Tokens) {
    let fillet = FILLET.min(tab.height() * 0.6);
    let base = tab.bottom();
    // Down past the pill's edge, so its hairline breaks under the join.
    painter.rect_filled(tab.with_max_y(base + 1.5), top_round(tab), t.surface);
    let seam = Rect::from_min_max(
        pos2(tab.left() - fillet, base),
        pos2(tab.right() + fillet, base + 1.5),
    );
    painter.rect_filled(seam, 0.0, t.surface);
    for side in [-1.0, 1.0] {
        painter.add(fillet_mesh(tab, side, fillet, t.surface));
    }
    painter.add(Shape::line(outline(tab, fillet), Stroke::new(1.0, t.line)));
    let words = ((note.shown - WORDS_FROM) / (1.0 - WORDS_FROM)).clamp(0.0, 1.0);
    if words > 0.0 {
        let font = island_font(painter.ctx(), t.text_small);
        let ink = t.ink.gamma_multiply(words);
        let clipped = painter.with_clip_rect(tab.intersect(painter.clip_rect()));
        clipped.text(tab.center(), Align2::CENTER_CENTER, &note.text, font, ink);
    }
}

/// The concave curve at one foot: `side` is -1 at the left, 1 at the right.
fn fillet_mesh(tab: Rect, side: f32, r: f32, fill: Color32) -> Mesh {
    let x = if side < 0.0 { tab.left() } else { tab.right() };
    let corner = pos2(x, tab.bottom());
    let centre = pos2(x + side * r, tab.bottom() - r);
    let mut mesh = Mesh::default();
    mesh.colored_vertex(corner, fill);
    for i in 0..=STEPS {
        let a = FRAC_PI_2 * i as f32 / STEPS as f32;
        mesh.colored_vertex(centre + egui::vec2(-side * r * a.cos(), r * a.sin()), fill);
        if i > 0 {
            let at = i as u32;
            mesh.add_triangle(0, at, at + 1);
        }
    }
    mesh
}

/// The edge from the pill's top at the left, round the tab, back down.
fn outline(tab: Rect, fillet: f32) -> Vec<Pos2> {
    // Half a point in, as the pill's own inside edge is drawn.
    let (left, right) = (tab.left() + 0.5, tab.right() - 0.5);
    let (top, foot) = (tab.top() + 0.5, tab.bottom() + 0.5);
    let r = ROUND.min(tab.height()).min(tab.width() / 2.0);
    let arc = |c: Pos2, rad: f32, from: f32| {
        (0..=STEPS).map(move |i| {
            let a = from + FRAC_PI_2 * i as f32 / STEPS as f32;
            c + egui::vec2(rad * a.cos(), rad * a.sin())
        })
    };
    let mut points: Vec<Pos2> = Vec::with_capacity(4 * (STEPS + 1));
    // Each foot's curve runs from the pill's edge up to the tab's side.
    let foot_left = pos2(left - fillet, foot - fillet);
    points.extend(arc(foot_left, fillet, 0.0).rev());
    points.extend(arc(pos2(left + r, top + r), r, 2.0 * FRAC_PI_2));
    points.extend(arc(pos2(right - r, top + r), r, 3.0 * FRAC_PI_2));
    let foot_right = pos2(right + fillet, foot - fillet);
    points.extend(arc(foot_right, fillet, FRAC_PI_2).rev());
    points
}
