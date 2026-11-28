//! A rounded perimeter around connected lines of selected text.

use egui::{Pos2, Rect, pos2};

const PAD: f32 = 7.0;
const RADIUS: f32 = 6.0;

/// Split disconnected lines so a contour never bridges columns or paragraphs.
pub(crate) fn groups(lines: &[Rect]) -> Vec<&[Rect]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, pair) in lines.windows(2).enumerate() {
        let (a, b) = (pair[0], pair[1]);
        let gap = b.top() - a.bottom();
        let overlap = a.right().min(b.right()) - a.left().max(b.left());
        if b.center().y <= a.center().y
            || gap < 0.0
            || gap > a.height().max(b.height()) * 1.1 + 4.0
            || overlap <= 4.0
        {
            out.push(&lines[start..i + 1]);
            start = i + 1;
        }
    }
    if start < lines.len() {
        out.push(&lines[start..]);
    }
    out
}

/// Trace the boundary of the line bands, then round their joins.
pub(crate) fn outline(lines: &[Rect]) -> Vec<Pos2> {
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
        return Vec::new();
    };
    let mut edges = vec![
        pos2(first.left() - PAD, first.top() - PAD),
        pos2(first.right() + PAD, first.top() - PAD),
    ];
    for pair in lines.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let y = (a.bottom() + b.top()) * 0.5;
        edges.extend([pos2(a.right() + PAD, y), pos2(b.right() + PAD, y)]);
    }
    edges.extend([
        pos2(last.right() + PAD, last.bottom() + PAD),
        pos2(last.left() - PAD, last.bottom() + PAD),
    ]);
    for pair in lines.windows(2).rev() {
        let (a, b) = (pair[0], pair[1]);
        let y = (a.bottom() + b.top()) * 0.5;
        edges.extend([pos2(b.left() - PAD, y), pos2(a.left() - PAD, y)]);
    }
    bend(&round(&edges))
}

/// Circular corners stay within half of each adjoining side.
fn round(edges: &[Pos2]) -> Vec<Pos2> {
    let mut corners = Vec::new();
    for &p in edges {
        if corners
            .last()
            .is_none_or(|q: &Pos2| (*q - p).length_sq() > 1e-5)
        {
            corners.push(p);
        }
    }
    if corners.len() > 1
        && corners
            .first()
            .zip(corners.last())
            .is_some_and(|(a, b)| (*a - *b).length_sq() < 1e-5)
    {
        corners.pop();
    }
    if corners.len() < 3 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let previous = corners.iter().cycle().skip(corners.len() - 1);
    let following = corners.iter().cycle().skip(1);
    for ((&prev, &p), &next) in previous.zip(&corners).zip(following) {
        let a = p - prev;
        let b = next - p;
        let (u, v) = (a.normalized(), b.normalized());
        let turn = u.x * v.y - u.y * v.x;
        if turn.abs() < 1e-4 {
            out.push(p);
            continue;
        }
        let r = RADIUS.min(a.length() * 0.45).min(b.length() * 0.45);
        let center = p - u * r + v * r;
        let start = p - u * r - center;
        for step in 0..=6 {
            let angle = turn.signum() * std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
            let (sin, cos) = angle.sin_cos();
            out.push(
                center + egui::vec2(start.x * cos - start.y * sin, start.x * sin + start.y * cos),
            );
        }
    }
    out
}

/// Bow long sides gently outward, with different handles on each side.
fn bend(points: &[Pos2]) -> Vec<Pos2> {
    let mut out = Vec::new();
    for (i, (a, b)) in points.iter().zip(points.iter().cycle().skip(1)).enumerate() {
        out.push(*a);
        let side = *b - *a;
        let len = side.length();
        if len < 35.0 {
            continue;
        }
        let normal = egui::vec2(side.y, -side.x) / len;
        let k = ((i as u32)
            .wrapping_mul(37)
            .wrapping_add((len as u32).wrapping_mul(11)))
            % 17;
        let first = 0.45 + (k % 5) as f32 * 0.22;
        let second = 0.45 + ((k * 7 + 3) % 5) as f32 * 0.22;
        let steps = ((len / 25.0).ceil() as usize).clamp(2, 64);
        for j in 1..steps {
            let t = j as f32 / steps as f32;
            let curve = 3.0 * (1.0 - t) * t * ((1.0 - t) * first + t * second);
            out.push(*a + side * t + normal * curve);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn separated_columns_and_far_lines_do_not_connect() {
        let lines = [
            Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 14.0)),
            Rect::from_min_size(pos2(200.0, 18.0), vec2(80.0, 14.0)),
            Rect::from_min_size(pos2(0.0, 200.0), vec2(80.0, 14.0)),
        ];
        assert_eq!(groups(&lines).len(), 3);
        assert!(outline(&[]).is_empty());
    }

    #[test]
    fn rounded_line_bands_follow_the_shorter_line() {
        let lines = [
            Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 18.0)),
            Rect::from_min_size(pos2(25.0, 21.0), vec2(60.0, 18.0)),
        ];
        let shape = outline(&lines);
        assert!(shape.len() > 20);
        assert!(shape.iter().all(|p| p.is_finite()));
        assert!(shape.iter().any(|p| p.x > 120.0));
        assert!(
            shape
                .iter()
                .any(|p| p.x > 85.0 && p.x < 100.0 && p.y > 30.0)
        );
    }
    fn contains(path: &[Pos2], p: Pos2) -> bool {
        let crosses = path
            .iter()
            .zip(path.iter().cycle().skip(1))
            .filter(|(a, b)| {
                (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y)
            })
            .count();
        crosses % 2 == 1
    }

    #[test]
    fn long_sides_bow_outward_without_mirroring() {
        let straight = [
            pos2(0.0, 0.0),
            pos2(200.0, 0.0),
            pos2(200.0, 30.0),
            pos2(0.0, 30.0),
        ];
        let path = bend(&straight);
        assert!(path.iter().any(|p| p.y < -0.3));
        assert!(path.iter().any(|p| p.y > 30.3));
        assert_eq!(path, bend(&straight));
    }

    #[test]
    fn rounded_indents_enclose_every_selected_line() {
        for gap in [0.2, 1.0, 4.0, 10.0] {
            for (x, w) in [(0.0, 110.0), (25.0, 70.0), (85.0, 60.0)] {
                let lines = [
                    Rect::from_min_size(pos2(0.0, 0.0), vec2(110.0, 12.0)),
                    Rect::from_min_size(pos2(x, 12.0 + gap), vec2(w, 12.0)),
                ];
                let path = outline(&lines);
                for r in lines {
                    for j in 0..=4 {
                        for i in 0..=20 {
                            let p = pos2(
                                r.left() + r.width() * i as f32 / 20.0,
                                r.top() + r.height() * j as f32 / 4.0,
                            );
                            assert!(contains(&path, p), "gap {gap}, x {x}, w {w}, point {p:?}");
                        }
                    }
                }
            }
        }
    }
}
