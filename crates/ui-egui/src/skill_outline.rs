//! A temporary, text-shaped outline for a selection skill in flight.

use egui::{Context, Id, Pos2, Rect, pos2, vec2};
use mareader_engine::activity::Key;
use mareader_engine::skills::{Anchor, Ask};
use mareader_workspace::PaneId;

use crate::documents::DocumentsView;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

const AIR: f32 = 4.0;
const MAX_LINES: usize = 256;
const STEP: f32 = 6.0;

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
    let Some(glow) = crate::glow::state(ctx, Id::new(("skill-glow", ask.id.0)), true) else {
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
            let points = rounded(&contour(group));
            visible |= crate::glow::paint_path(ctx, &painter, &tokens, &points, glow);
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
