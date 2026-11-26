//! A temporary, text-shaped outline for a selection skill in flight.

use egui::{Context, Id, Pos2, Rect, Vec2, pos2, vec2};
use mareader_engine::activity::Key;
use mareader_workspace::PaneId;

mod feedback;

use crate::documents::DocumentsView;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

const AIR: f32 = 7.0;
const MAX_LINES: usize = 256;
const STEP: f32 = 6.0;

pub(crate) use feedback::{clear, show};

/// Draw the visible pages of a selection, reporting its visual while working.
fn paint(
    ctx: &Context,
    place: &feedback::Place,
    panes: &[(PaneId, Rect)],
    docs: DocumentsView<'_>,
    (working, glow): (bool, crate::glow::Glow),
) {
    let Some(room) = panes
        .iter()
        .find(|(p, _)| *p == place.pane)
        .map(|(_, r)| *r)
    else {
        return;
    };
    let tokens = Tokens::get(ctx);
    let painter = z_order::painter(ctx, Tier::PaneMark, Id::new(("skill-outline", place.id.0)));
    z_order::register(ctx, Tier::PaneMark, painter.layer_id());
    let mut visible = false;
    for sheet in docs.sheets(place.pane) {
        let clip = room.intersect(sheet.shown);
        if !clip.is_positive() {
            continue;
        }
        let Some((_, range)) = place.spans.iter().find(|(page, _)| *page == sheet.page) else {
            continue;
        };
        let Some(layer) = docs.cached_layer(place.pane, sheet.page) else {
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
            let seed = place.id.0
                ^ (sheet.page as u64).wrapping_mul(0x9e3779b97f4a7c15)
                ^ (range.start as u64).wrapping_mul(0xbf58476d1ce4e5b9)
                ^ (range.end as u64).wrapping_mul(0x94d049bb133111eb);
            let points = rounded(&contour(group), group, seed);
            if let Some(line) = group.first() {
                visible |= crate::glow::paint_path(
                    ctx,
                    &painter,
                    &tokens,
                    &points,
                    line.expand(0.5),
                    glow,
                );
            }
        }
    }
    if working && visible {
        crate::activity::seen::in_view(
            ctx,
            Key::Skill {
                pane: place.pane,
                ask: place.id,
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
fn rounded(points: &[Pos2], lines: &[Rect], seed: u64) -> Vec<Pos2> {
    let smooth = smooth(points, seed);
    organic(&smooth, lines, seed)
}

/// Quadratic turns connect the sides with a continuous tangent.
fn smooth(points: &[Pos2], seed: u64) -> Vec<Pos2> {
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
        let radius = 6.0 + 4.0 * random_unit(seed ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let r = radius.min(a.length() * 0.42).min(b.length() * 0.42);
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

/// Irregular, closed bends stay fixed while the light moves.
fn organic(points: &[Pos2], lines: &[Rect], seed: u64) -> Vec<Pos2> {
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
    let drift = Drift::new(seed, length);
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
        let bend = drift.at(distance / length);
        let delta = normal * bend.clamp(-limit, limit);
        out.push(at + keep_clear(at, delta, lines));
        distance += distances.get(i).copied().unwrap_or(0.0);
    }
    out
}

/// A repeatable mix, independent of frame time and screen position.
fn random_unit(seed: u64) -> f32 {
    let mut x = seed.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    let bits = (x ^ (x >> 31)) >> 40;
    bits as f32 / 16_777_215.0
}

/// Unevenly spaced bends, joined smoothly around the closed contour.
struct Drift {
    knots: Vec<(f32, f32)>,
}

impl Drift {
    fn new(seed: u64, length: f32) -> Self {
        let count = (length / 95.0).round().clamp(5.0, 64.0) as usize;
        let mut knots = Vec::with_capacity(count);
        let mut at = 0.0;
        for i in 0..count {
            let id = seed ^ (i as u64).wrapping_mul(0x9e3779b97f4a7c15);
            let amount = random_unit(id ^ 0x7f4a7c15);
            let sign = if random_unit(id ^ 0x94d049bb) < 0.5 {
                -1.0
            } else {
                1.0
            };
            knots.push((at, sign * (0.4 + 0.6 * amount) * 2.3));
            at += 0.6 + 0.8 * random_unit(id ^ 0xbf58476d);
        }
        for (position, _) in &mut knots {
            *position /= at;
        }
        Self { knots }
    }

    fn at(&self, share: f32) -> f32 {
        let share = share.rem_euclid(1.0);
        let next = self.knots.partition_point(|(x, _)| *x <= share);
        let Some(&(from, a)) = self.knots.get(next.saturating_sub(1)) else {
            return 0.0;
        };
        let Some(&(_, first)) = self.knots.first() else {
            return 0.0;
        };
        let (to, b) = self.knots.get(next).copied().unwrap_or((1.0, first));
        let t = ((share - from) / (to - from)).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        a + (b - a) * eased
    }
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
        assert!(!rounded(&p, &lines, 1).is_empty());
    }

    #[test]
    fn turns_are_rounded_and_the_deformation_is_bounded_and_stable() {
        let corners = [
            pos2(0.0, 0.0),
            pos2(140.0, 0.0),
            pos2(140.0, 40.0),
            pos2(0.0, 40.0),
        ];
        let smooth = smooth(&corners, 1);
        let bent = organic(&smooth, &[], 1);
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
            organic(&smooth, &[], 1),
            "no random frame-to-frame jitter"
        );
    }

    #[test]
    fn bends_are_seamless_irregular_and_seeded() {
        let a = Drift::new(72, 800.0);
        let b = Drift::new(73, 800.0);
        assert!((a.at(0.0) - a.at(1.0)).abs() < 1e-5, "the seam closes");
        assert!(
            (a.at(0.00001) - a.at(0.99999)).abs() < 0.001,
            "the seam is smooth"
        );
        let samples: Vec<_> = (0..20).map(|i| a.at(i as f32 / 20.0)).collect();
        assert!(samples.iter().all(|v| v.abs() <= 2.3));
        assert!(
            samples
                .iter()
                .enumerate()
                .any(|(i, v)| (v - samples[(i + 10) % 20]).abs() > 0.5),
            "not mirrored"
        );
        assert!(
            (0..20).any(|i| (a.at(i as f32 / 20.0) - b.at(i as f32 / 20.0)).abs() > 0.5),
            "different selections vary"
        );
        assert_eq!(
            a.knots,
            Drift::new(72, 800.0).knots,
            "stable between frames"
        );
    }

    #[test]
    fn the_contour_stays_clear_of_its_lines() {
        let lines = [
            line(30.0, 0.0, 80.0),
            line(0.0, 16.0, 110.0),
            line(0.0, 32.0, 50.0),
        ];
        let outline = rounded(&contour(&lines), &lines, 1);
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
                for seed in 0..16 {
                    let outline = rounded(&contour(&lines), &lines, seed);
                    assert!(!outline.is_empty());
                    let inside: Vec<_> = outline
                        .iter()
                        .filter(|p| lines.iter().any(|l| l.contains(**p)))
                        .collect();
                    assert!(
                        inside.is_empty(),
                        "gap {gap} x {x} w {w} seed {seed}: {inside:?}"
                    );
                }
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
        assert!(rounded(&[], &[], 1).is_empty());
    }
}
