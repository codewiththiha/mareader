//! A cover drawn like a bound edition, and a pass of light.

use std::f32::consts::{FRAC_PI_2, PI};

use egui::epaint::{Mesh, Shadow};
use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use mareader_library::{Book, CoverFit};

use super::SHAPES;
use super::cloth::cloth;
use crate::library::covers::{self, Cover, Covers};
use crate::motion;
use crate::theme::Tokens;

/// How a face is drawn: a thumb has no fit; `lift` raises it on hover.
#[derive(Clone, Copy)]
pub(super) struct Look {
    pub fit: Option<CoverFit>,
    pub fade: f32,
    pub lift: f32,
}

/// Tight at the spine, softer at the fore-edge, as a bound book.
pub(super) fn corners(small: bool, t: &Tokens) -> CornerRadius {
    let edge = if small { 3.0 } else { t.radius.clamp(3.0, 6.0) };
    let spine = (edge * 0.4).round() as u8;
    let edge = edge.round() as u8;
    CornerRadius {
        nw: spine,
        sw: spine,
        ne: edge,
        se: edge,
    }
}

/// Art if read, else a placeholder or cloth; returns where the face lies.
pub(super) fn paint(
    ui: &Ui,
    bounds: Rect,
    book: &Book,
    (covers, t): (&mut Covers, &Tokens),
    look: Look,
) -> Rect {
    let small = look.fit.is_none();
    let round = corners(small, t);
    let (p, fade) = (ui.painter(), look.fade);
    match covers.get(ui.ctx(), book, motion::enabled(ui.ctx())) {
        Cover::Art { texture, alpha } => {
            let shape = texture.size.y / texture.size.x;
            let size = vec2(1.0, shape.clamp(*SHAPES.start(), *SHAPES.end()));
            let rect = match look.fit {
                Some(CoverFit::Fit) => covers::fit(bounds, size),
                _ => bounds,
            };
            if alpha < 1.0 {
                p.rect_filled(rect, round, t.hover.gamma_multiply(fade));
            }
            shadow(
                p,
                rect,
                (round, small),
                look.lift,
                t.shadow.gamma_multiply(fade * alpha),
            );
            egui::Image::from_texture(texture)
                .uv(covers::crop(rect, texture.size))
                .corner_radius(round)
                .tint(Color32::WHITE.gamma_multiply(fade * alpha))
                .paint_at(ui, rect);
            finish(p, rect, (round, small), fade * alpha, t);
            rect
        }
        Cover::Waiting => {
            p.rect_filled(bounds, round, t.hover.gamma_multiply(fade));
            bounds
        }
        Cover::Drawn => {
            shadow(
                p,
                bounds,
                (round, small),
                look.lift,
                t.shadow.gamma_multiply(fade),
            );
            cloth(p, bounds, book, (round, small), fade, t);
            finish(p, bounds, (round, small), fade, t);
            bounds
        }
    }
}

/// A contact shadow under a soft one; a lifted cover casts it further.
fn shadow(
    p: &Painter,
    rect: Rect,
    (round, small): (CornerRadius, bool),
    lift: f32,
    color: Color32,
) {
    let layers: &[(f32, f32, f32)] = match small {
        true => &[(1.0, 3.0, 0.8)],
        false => &[
            (1.0, 2.0, 0.7),
            (6.0 + 4.0 * lift, 18.0 + 10.0 * lift, 0.55),
        ],
    };
    for &(down, blur, share) in layers {
        let cast = Shadow {
            offset: [0, down.round() as i8],
            blur: blur.round() as u8,
            spread: 0,
            color: color.gamma_multiply(share),
        };
        p.add(cast.as_shape(rect, round));
    }
}

/// The hairline edge, and on a full cover the hinge by its spine.
fn finish(p: &Painter, rect: Rect, (round, small): (CornerRadius, bool), fade: f32, t: &Tokens) {
    let edge = match t.dark {
        true => Color32::from_white_alpha(24),
        false => Color32::from_black_alpha(28),
    };
    p.rect_stroke(
        rect,
        round,
        Stroke::new(1.0, edge.gamma_multiply(fade)),
        StrokeKind::Inside,
    );
    if small {
        return;
    }
    // A groove pressed in near the spine, lit on its far side.
    let x = rect.left() + (rect.width() * 0.045).max(5.0);
    let (top, bottom) = (rect.top() + 0.5, rect.bottom() - 0.5);
    let groove = Color32::from_black_alpha(46).gamma_multiply(fade);
    p.line_segment([pos2(x, top), pos2(x, bottom)], Stroke::new(1.0, groove));
    let lit = Color32::from_white_alpha(34).gamma_multiply(fade);
    let shine = x + 1.25;
    p.line_segment(
        [pos2(shine, top), pos2(shine, bottom)],
        Stroke::new(1.5, lit),
    );
    let fall = Rect::from_x_y_ranges(shine + 0.75..=shine + 10.0, top..=bottom);
    let shade = Color32::from_black_alpha(16).gamma_multiply(fade);
    let window = rounded(rect, round);
    across(
        p,
        &clip(quad(fall), &window),
        fall,
        (shade, Color32::TRANSPARENT),
    );
}

/// A soft band of light across `face`, `at` 0 to 1 from spine to edge.
pub(super) fn sweep(
    p: &Painter,
    face: Rect,
    round: CornerRadius,
    at: f32,
    strength: f32,
    t: &Tokens,
) {
    if strength <= 0.0 || face.width() < 1.0 {
        return;
    }
    let slant = face.height() * 0.3;
    let half = face.width() * 0.3;
    let travel = face.width() + 2.0 * half + slant;
    let mid = face.left() - half + travel * at.clamp(0.0, 1.0);
    let lean = slant / face.height().max(1.0);
    let along = |q: Pos2| q.x + (q.y - face.top()) * lean;
    let white = if t.dark { 0.5 } else { 0.4 };
    let light = t.accent.lerp_to_gamma(Color32::WHITE, white);
    let peak = strength * if t.dark { 0.34 } else { 0.5 };
    let window = rounded(face, round);
    let (top, bottom) = (face.top(), face.bottom());
    for (from, to) in [(mid - half, mid), (mid, mid + half)] {
        let band = vec![
            pos2(from, top),
            pos2(to, top),
            pos2(to - slant, bottom),
            pos2(from - slant, bottom),
        ];
        shade(p, &clip(band, &window), |q| {
            let k = 1.0 - ((along(q) - mid).abs() / half).min(1.0);
            light.gamma_multiply(peak * k)
        });
    }
}

/// A rect's corners, clockwise from its top left.
pub(super) fn quad(rect: Rect) -> Vec<Pos2> {
    vec![
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
    ]
}

/// A rounded rect as a convex outline, clockwise on screen.
pub(super) fn rounded(rect: Rect, round: CornerRadius) -> Vec<Pos2> {
    let arcs = [
        (round.nw, rect.left_top(), vec2(1.0, 1.0), PI),
        (round.ne, rect.right_top(), vec2(-1.0, 1.0), PI + FRAC_PI_2),
        (round.se, rect.right_bottom(), vec2(-1.0, -1.0), 0.0),
        (round.sw, rect.left_bottom(), vec2(1.0, -1.0), FRAC_PI_2),
    ];
    let most = rect.width().min(rect.height()) / 2.0;
    let mut out = Vec::with_capacity(20);
    for (r, corner, inward, from) in arcs {
        let r = f32::from(r).min(most);
        if r <= 0.0 {
            out.push(corner);
            continue;
        }
        let centre = corner + inward * r;
        for i in 0..=4 {
            let a = from + FRAC_PI_2 * i as f32 / 4.0;
            out.push(centre + vec2(a.cos(), a.sin()) * r);
        }
    }
    out
}

/// What of `shape` lies inside the convex, clockwise `window`.
pub(super) fn clip(shape: Vec<Pos2>, window: &[Pos2]) -> Vec<Pos2> {
    let mut out = shape;
    for (i, &a) in window.iter().enumerate() {
        let Some(&b) = window.get(i + 1).or(window.first()) else {
            break;
        };
        let side = |q: Pos2| (b - a).x * (q - a).y - (b - a).y * (q - a).x;
        let input = std::mem::take(&mut out);
        for (j, &q) in input.iter().enumerate() {
            let Some(&r) = input.get(j + 1).or(input.first()) else {
                break;
            };
            let (sq, sr) = (side(q), side(r));
            if sq >= 0.0 {
                out.push(q);
            }
            if (sq >= 0.0) != (sr >= 0.0) {
                out.push(q + (r - q) * (sq / (sq - sr)));
            }
        }
        if out.is_empty() {
            break;
        }
    }
    out
}

/// Fills a convex outline, each corner its own colour, blended between.
pub(super) fn shade(p: &Painter, outline: &[Pos2], color: impl Fn(Pos2) -> Color32) {
    if outline.len() < 3 {
        return;
    }
    let mut mesh = Mesh::default();
    for &at in outline {
        mesh.colored_vertex(at, color(at));
    }
    for i in 1..outline.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(Shape::mesh(mesh));
}

/// Fills an outline with `colors` blended left to right across `span`.
pub(super) fn across(p: &Painter, outline: &[Pos2], span: Rect, colors: (Color32, Color32)) {
    let w = span.width().max(1.0);
    shade(p, outline, |at| {
        let k = ((at.x - span.left()) / w).clamp(0.0, 1.0);
        colors.0.lerp_to_gamma(colors.1, k)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(outline: &[Pos2]) -> f32 {
        let pairs = outline.iter().zip(outline.iter().cycle().skip(1));
        pairs.map(|(a, b)| a.x * b.y - b.x * a.y).sum::<f32>() / 2.0
    }

    #[test]
    fn a_band_is_clipped_to_the_rounded_face() {
        let face = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 140.0));
        let window = rounded(face, CornerRadius::same(6));
        // Wider than the face: only the face is left, its corners rounded.
        let wide = quad(face.expand(20.0));
        let kept = clip(wide, &window);
        let full = area(&kept);
        assert!(
            full < 100.0 * 140.0 && full > 100.0 * 140.0 - 40.0,
            "{full}"
        );
        // A band half off the face keeps its half.
        let half = quad(Rect::from_min_max(pos2(50.0, -10.0), pos2(150.0, 150.0)));
        let kept = area(&clip(half, &window));
        assert!((kept - 50.0 * 140.0).abs() < 20.0, "{kept}");
        let away = quad(Rect::from_min_size(pos2(200.0, 0.0), vec2(10.0, 10.0)));
        assert!(clip(away, &window).is_empty());
    }
}
