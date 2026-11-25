//! A temporary, text-shaped outline for a selection skill in flight.

use egui::epaint::Mesh;
use egui::{Context, Id, Pos2, Rect, Shape, Vec2, pos2, vec2};
use mareader_engine::activity::Key;
use mareader_engine::skills::{Anchor, Ask};
use mareader_workspace::PaneId;

use crate::documents::DocumentsView;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

const AIR: f32 = 4.0;
const MAX_LINES: usize = 256;
const STEP: f32 = 12.0;

/// Draw the visible pages of a running selection and report its own visual.
pub(crate) fn show(ctx: &Context, ask: &Ask, panes: &[(PaneId, Rect)], docs: DocumentsView<'_>) {
    let Anchor::Text(spans) = &ask.anchor else {
        return;
    };
    if ask.answer.landed() {
        return;
    }
    let Some(room) = panes.iter().find(|(p, _)| *p == ask.pane).map(|(_, r)| *r) else {
        return;
    };
    let tokens = Tokens::get(ctx);
    let painter = z_order::painter(ctx, Tier::PaneMark, Id::new(("skill-outline", ask.id.0)));
    z_order::register(ctx, Tier::PaneMark, painter.layer_id());
    let mut visible = false;
    for sheet in docs.sheets(ask.pane) {
        let clip = room.intersect(sheet.shown);
        if !clip.is_positive() {
            continue;
        }
        let Some((_, range)) = spans.iter().find(|(page, _)| *page == sheet.page) else {
            continue;
        };
        let Some(layer) = docs.cached_layer(ask.pane, sheet.page) else {
            continue;
        };
        // Retain a margin beyond the clip so a contour doesn't close at the viewport edge.
        let near = clip.expand(80.0);
        let lines: Vec<_> = layer
            .text
            .line_boxes(range.clone())
            .map(|b| sheet.to_screen(b))
            .filter(|r| r.is_finite() && r.is_positive() && near.intersects(*r))
            .take(MAX_LINES)
            .collect();
        let painter = painter.with_clip_rect(clip);
        for group in groups(&lines) {
            let points = contour(group);
            visible |= paint(ctx, &painter, &tokens, &points, clip);
        }
    }
    if visible {
        crate::activity::seen::in_view(
            ctx,
            Key::Skill {
                pane: ask.pane,
                ask: ask.id,
            },
        );
    }
}

/// Join only neighbouring, overlapping horizontal lines, never distant columns.
fn groups(lines: &[Rect]) -> Vec<&[Rect]> {
    let mut groups = Vec::new();
    let mut start = 0;
    for (i, pair) in lines.windows(2).enumerate() {
        let a = pair[0];
        let b = pair[1];
        let gap = b.top() - a.bottom();
        let overlap = a.right().min(b.right()) - a.left().max(b.left());
        let joins = gap >= 0.0 && gap <= a.height().max(b.height()) * 1.1 + 4.0 && overlap > 4.0;
        if !joins {
            groups.push(&lines[start..i + 1]);
            start = i + 1;
        }
    }
    if start < lines.len() {
        groups.push(&lines[start..]);
    }
    groups
}

/// A stepped perimeter: each sideways step lies between its two lines.
fn contour(lines: &[Rect]) -> Vec<Pos2> {
    let Some(first) = lines.first() else {
        return Vec::new();
    };
    let Some(last) = lines.last() else {
        return Vec::new();
    };
    let mut p = vec![
        pos2(first.left() - AIR, first.top() - AIR),
        pos2(first.right() + AIR, first.top() - AIR),
    ];
    for pair in lines.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let y = (a.bottom() + b.top()) * 0.5;
        p.push(pos2(a.right() + AIR, y));
        p.push(pos2(b.right() + AIR, y));
    }
    p.push(pos2(last.right() + AIR, last.bottom() + AIR));
    p.push(pos2(last.left() - AIR, last.bottom() + AIR));
    for pair in lines.windows(2).rev() {
        let (a, b) = (pair[0], pair[1]);
        let y = (a.bottom() + b.top()) * 0.5;
        p.push(pos2(b.left() - AIR, y));
        p.push(pos2(a.left() - AIR, y));
    }
    p
}

/// Round each turn, then sample long sides for a smooth moving colour.
fn rounded(points: &[Pos2]) -> Vec<Pos2> {
    if points.len() < 3 {
        return Vec::new();
    }
    let mut turns = Vec::new();
    for i in 0..points.len() {
        let prev = points[(i + points.len() - 1) % points.len()];
        let at = points[i];
        let next = points[(i + 1) % points.len()];
        let a = prev - at;
        let b = next - at;
        let r = 2.0_f32.min(a.length() * 0.4).min(b.length() * 0.4);
        if r < 0.01 {
            continue;
        }
        let from = at + a.normalized() * r;
        let to = at + b.normalized() * r;
        turns.extend([from, from + (at - from) * 0.75 + (to - at) * 0.25, to]);
    }
    let mut out = Vec::new();
    for (a, b) in turns.iter().zip(turns.iter().cycle().skip(1)) {
        let d = *b - *a;
        let count = ((d.length() / STEP).ceil() as usize).clamp(1, 128);
        let normal = vec2(-d.y, d.x).normalized();
        for i in 0..count {
            let t = i as f32 / count as f32;
            let wave = (std::f32::consts::PI * t).sin()
                * (std::f32::consts::TAU * t * d.length() / 85.0).sin()
                * 0.45;
            out.push(*a + d * t + normal * wave);
        }
    }
    out
}

/// A narrow rainbow stroke and a softer halo, clipped to the page.
fn paint(
    ctx: &Context,
    painter: &egui::Painter,
    tokens: &Tokens,
    outline: &[Pos2],
    clip: Rect,
) -> bool {
    let points = rounded(outline);
    if points.len() < 3 {
        return false;
    }
    let lengths: Vec<f32> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| (*b - *a).length())
        .collect();
    let total: f32 = lengths.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return false;
    }
    let mut mesh = Mesh::default();
    let mut distance = 0.0;
    let mut visible = false;
    let tint = crate::glow::tint_fn(ctx, tokens);
    for ((a, b), length) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .zip(lengths)
    {
        if length < 0.01 {
            continue;
        }
        let normal = vec2(-(b.y - a.y), b.x - a.x) / length;
        let start = distance;
        distance += length;
        let bounds = Rect::from_two_pos(*a, *b).expand(3.5);
        if !bounds.intersects(clip) {
            continue;
        }
        let color = tint(start / total);
        let next = tint(distance / total);
        visible = true;
        segment(&mut mesh, *a, *b, normal, color, next);
    }
    if visible {
        painter.add(Shape::mesh(mesh));
        if !crate::motion::reduced(ctx) {
            ctx.request_repaint_after_secs(1.0 / 30.0);
        }
    }
    visible
}

fn segment(mesh: &mut Mesh, a: Pos2, b: Pos2, n: Vec2, color: egui::Color32, next: egui::Color32) {
    let base = mesh.vertices.len() as u32;
    for (p, c) in [(a, color), (b, next)] {
        for (off, alpha) in [(-3.5, 0.0), (-1.0, 0.85), (1.0, 0.85), (3.5, 0.0)] {
            mesh.colored_vertex(p + n * off, c.gamma_multiply(alpha));
        }
    }
    for i in 0..3 {
        mesh.add_triangle(base + i, base + i + 1, base + i + 4);
        mesh.add_triangle(base + i + 1, base + i + 5, base + i + 4);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: f32, y: f32, w: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, 12.0))
    }

    #[test]
    fn the_outline_follows_shorter_lines_instead_of_a_rectangle() {
        let lines = [
            line(30.0, 0.0, 80.0),
            line(0.0, 16.0, 110.0),
            line(0.0, 32.0, 50.0),
        ];
        let p = contour(&lines);
        assert!(
            p.contains(&pos2(54.0, 30.0)),
            "last line's short edge: {p:?}"
        );
        assert!(p.contains(&pos2(26.0, 14.0)), "first line's indent: {p:?}");
        assert_eq!(groups(&lines).len(), 1);
        assert!(!rounded(&p).is_empty());
    }

    #[test]
    fn distant_or_side_by_side_lines_do_not_bridge() {
        let lines = [
            line(0.0, 0.0, 80.0),
            line(200.0, 16.0, 80.0),
            line(0.0, 200.0, 80.0),
        ];
        assert_eq!(groups(&lines).len(), 3);
        assert!(groups(&[]).is_empty());
        assert!(rounded(&[]).is_empty());
    }
}
