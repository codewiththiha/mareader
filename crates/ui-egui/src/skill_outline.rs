//! A temporary, text-shaped outline for a selection skill in flight.

use egui::{Context, Id, Pos2, Rect, Vec2, pos2, vec2};
use mareader_engine::activity::Key;
use mareader_engine::skills::{Anchor, Ask};
use mareader_workspace::PaneId;

use crate::documents::DocumentsView;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

const AIR: f32 = 7.0;
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
        // Keep the contour's ends beyond the viewport.
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
            let points = rounded(&contour(group), group);
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

/// Round the contour, then gently bend it without moving the text.
fn rounded(points: &[Pos2], lines: &[Rect]) -> Vec<Pos2> {
    let smooth = smooth(points);
    organic(&smooth, lines)
}

/// Quadratic turns connect the sides with a continuous tangent.
fn smooth(points: &[Pos2]) -> Vec<Pos2> {
    let mut corners = Vec::new();
    for &point in points {
        if corners
            .last()
            .is_none_or(|last: &Pos2| (*last - point).length() > 0.01)
        {
            corners.push(point);
        }
    }
    if corners.len() > 1 && (corners[0] - corners[corners.len() - 1]).length() < 0.01 {
        corners.pop();
    }
    if corners.len() < 3 {
        return Vec::new();
    }
    let mut turns = Vec::new();
    for (i, at) in corners.iter().copied().enumerate() {
        let prev = corners[(i + corners.len() - 1) % corners.len()];
        let next = corners[(i + 1) % corners.len()];
        let a = prev - at;
        let b = next - at;
        let r = 10.0_f32.min(a.length() * 0.42).min(b.length() * 0.42);
        let from = at + a.normalized() * r;
        let to = at + b.normalized() * r;
        for step in 0..=5 {
            let t = step as f32 / 5.0;
            turns.push(from + (at - from) * (2.0 * t * (1.0 - t)) + (to - from) * t * t);
        }
    }
    let mut out = Vec::new();
    for (a, b) in turns.iter().zip(turns.iter().cycle().skip(1)) {
        let d = *b - *a;
        let count = ((d.length() / STEP).ceil() as usize).clamp(1, 128);
        for i in 0..count {
            out.push(*a + d * (i as f32 / count as f32));
        }
    }
    out
}

/// Two closed waves vary the line without a moving or visible seam.
fn organic(points: &[Pos2], lines: &[Rect]) -> Vec<Pos2> {
    if points.len() < 3 {
        return Vec::new();
    }
    let distances: Vec<f32> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| (*b - *a).length())
        .collect();
    let length: f32 = distances.iter().sum();
    if !length.is_finite() || length < 0.01 {
        return Vec::new();
    }
    let waves = (length / 100.0).round().clamp(1.0, 60.0);
    let mut distance = 0.0;
    let mut out = Vec::with_capacity(points.len());
    for (i, at) in points.iter().copied().enumerate() {
        let prev = points[(i + points.len() - 1) % points.len()];
        let next = points[(i + 1) % points.len()];
        let incoming = (at - prev).normalized();
        let outgoing = (next - at).normalized();
        let tangent = (incoming + outgoing).normalized();
        let normal = vec2(-tangent.y, tangent.x);
        let half_angle = ((1.0 - incoming.dot(outgoing)).max(0.0) * 0.5).sqrt();
        let arc = (at - prev).length().min((next - at).length());
        let limit = if half_angle > 0.001 {
            (arc / (2.0 * half_angle) * 0.35).min(2.3)
        } else {
            2.3
        };
        let phase = std::f32::consts::TAU * distance / length;
        let wave = 1.65 * (phase * waves + 0.35).sin() + 0.65 * (phase * (waves + 2.0) + 1.2).sin();
        let delta = normal * wave.clamp(-limit, limit);
        out.push(at + keep_clear(at, delta, lines));
        distance += distances.get(i).copied().unwrap_or(0.0);
    }
    out
}

/// Shorten a bend before it reaches a selected line.
fn keep_clear(at: Pos2, delta: Vec2, lines: &[Rect]) -> Vec2 {
    let end = at + delta;
    let low = at.y.min(end.y);
    let high = at.y.max(end.y);
    let start = lines.partition_point(|r| r.bottom() < low);
    let mut share: f32 = 1.0;
    for line in &lines[start..] {
        if line.top() > high {
            break;
        }
        if let Some(entry) = enters(at, delta, *line) {
            share = share.min((entry * 0.98).max(0.0));
        }
    }
    delta * share
}

/// How far a short segment travels before entering a rectangle.
fn enters(at: Pos2, delta: Vec2, rect: Rect) -> Option<f32> {
    let mut low: f32 = 0.0;
    let mut high: f32 = 1.0;
    for (p, d, a, b) in [
        (at.x, delta.x, rect.left(), rect.right()),
        (at.y, delta.y, rect.top(), rect.bottom()),
    ] {
        if d.abs() < 1e-5 {
            if p < a || p > b {
                return None;
            }
        } else {
            let (x, y) = ((a - p) / d, (b - p) / d);
            low = low.max(x.min(y));
            high = high.min(x.max(y));
        }
    }
    (low <= high && high >= 0.0 && low <= 1.0).then_some(low)
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
            p.contains(&pos2(57.0, 30.0)),
            "last line's short edge: {p:?}"
        );
        assert!(p.contains(&pos2(23.0, 14.0)), "first line's indent: {p:?}");
        assert_eq!(groups(&lines).len(), 1);
        assert!(!rounded(&p, &lines).is_empty());
    }

    #[test]
    fn turns_are_rounded_and_the_deformation_is_bounded_and_stable() {
        let corners = [
            pos2(0.0, 0.0),
            pos2(140.0, 0.0),
            pos2(140.0, 40.0),
            pos2(0.0, 40.0),
        ];
        let smooth = smooth(&corners);
        let bent = organic(&smooth, &[]);
        assert_eq!(smooth.len(), bent.len());
        assert!(
            corners.iter().all(|c| !smooth.contains(c)),
            "no square corner"
        );
        let steps: Vec<_> = smooth
            .iter()
            .zip(smooth.iter().cycle().skip(1))
            .map(|(a, b)| (*b - *a).normalized())
            .collect();
        assert!(
            steps
                .iter()
                .zip(steps.iter().cycle().skip(1))
                .all(|(a, b)| a.dot(*b) > 0.8),
            "no sharp turns"
        );
        let shifts: Vec<f32> = smooth
            .iter()
            .zip(&bent)
            .map(|(a, b)| (*a - *b).length())
            .collect();
        assert!(shifts.iter().all(|s| *s <= 2.31), "small drift");
        assert!(shifts.iter().any(|s| *s > 1.0), "actually hand-shaped");
        assert_eq!(
            bent,
            organic(&smooth, &[]),
            "no random frame-to-frame jitter"
        );
    }

    #[test]
    fn the_contour_stays_clear_of_its_lines() {
        let lines = [
            line(30.0, 0.0, 80.0),
            line(0.0, 16.0, 110.0),
            line(0.0, 32.0, 50.0),
        ];
        let outline = rounded(&contour(&lines), &lines);
        assert!(!outline.is_empty());
        assert!(
            outline
                .iter()
                .all(|p| lines.iter().all(|line| !line.contains(*p)))
        );
    }

    #[test]
    fn narrow_gaps_and_indents_keep_the_curve_off_the_text() {
        for gap in [0.2, 1.0, 4.0, 10.0] {
            for (x, w) in [(0.0, 110.0), (25.0, 70.0), (85.0, 60.0)] {
                let lines = [line(0.0, 0.0, 110.0), line(x, 12.0 + gap, w)];
                let outline = rounded(&contour(&lines), &lines);
                assert!(!outline.is_empty());
                let inside: Vec<_> = outline
                    .iter()
                    .filter(|p| lines.iter().any(|l| l.contains(**p)))
                    .collect();
                assert!(inside.is_empty(), "gap {gap} x {x} w {w}: {inside:?}");
            }
        }
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
        assert!(rounded(&[], &[]).is_empty());
    }
}
