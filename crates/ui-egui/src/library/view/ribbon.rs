//! How far through a book, on a silk bookmark tucked under its cover.

use egui::epaint::{PathShape, Shadow};
use egui::{Color32, CornerRadius, Painter, Rect, Stroke, pos2};

use super::face::shade;
use crate::theme::{self, Tokens};

/// How far through, as "89%" on a silk bookmark over the cover's head.
pub(super) fn ribbon(p: &Painter, face: Rect, done: f32, t: &Tokens, fade: f32) {
    let w = face.width();
    let size = (w / 16.0).clamp(9.0, 11.5);
    let silk = silk(t);
    let ink = theme::ink_on(silk).gamma_multiply(fade);
    let said = p.layout_no_wrap(format!("{}%", percent(done)), theme::medium(size), ink);
    let wide = (said.size().x + size * 1.1).max(w * 0.14).round();
    let notch = (wide * 0.32).round();
    let body = (said.size().y + size * 1.5).max(wide * 1.25).round();
    let (left, top) = ((face.right() - w * 0.1 - wide).round(), face.top());
    let (right, mid) = (left + wide, left + wide / 2.0);
    let (waist, foot) = (top + body, top + body + notch);
    let cast = Shadow {
        offset: [0, 2],
        blur: 5,
        spread: 0,
        color: t.shadow.gamma_multiply(0.8 * fade),
    };
    let laid = Rect::from_x_y_ranges(left + 1.0..=right - 1.0, top..=waist + notch * 0.4);
    p.add(cast.as_shape(laid, 0));
    // Over the head: the silk comes from inside the book, folded down.
    let fold = (size * 0.3).clamp(2.5, 3.5);
    let over = Rect::from_x_y_ranges(left..=right, top - fold..=top);
    let tone = |to: Color32, k: f32| silk.lerp_to_gamma(to, k).gamma_multiply(fade);
    let head = CornerRadius {
        nw: 2,
        ne: 2,
        ..Default::default()
    };
    p.rect_filled(over, head, tone(Color32::BLACK, 0.38));
    // Silk: a sheen down its middle, falling off toward both edges.
    let (edge, sheen) = (tone(Color32::BLACK, 0.16), tone(Color32::WHITE, 0.16));
    let halves = [
        [
            pos2(left, top),
            pos2(mid, top),
            pos2(mid, waist),
            pos2(left, foot),
        ],
        [
            pos2(mid, top),
            pos2(right, top),
            pos2(right, foot),
            pos2(mid, waist),
        ],
    ];
    for half in halves {
        shade(p, &half, |at| match (at.x - mid).abs() < 0.5 {
            true => sheen,
            false => edge,
        });
    }
    let outline = vec![
        pos2(left, top),
        pos2(right, top),
        pos2(right, foot),
        pos2(mid, waist),
        pos2(left, foot),
    ];
    let rim = tone(Color32::BLACK, 0.3).gamma_multiply(0.8);
    p.add(PathShape::closed_line(outline, Stroke::new(1.0, rim)));
    // The cover's head casts a little shade onto the silk below it.
    let tuck = [
        pos2(left, top),
        pos2(right, top),
        pos2(right, top + 4.0),
        pos2(left, top + 4.0),
    ];
    let dark = Color32::from_black_alpha(56).gamma_multiply(fade);
    shade(p, &tuck, |at| match at.y <= top {
        true => dark,
        false => Color32::TRANSPARENT,
    });
    let x = mid - said.size().x / 2.0;
    let at = pos2(x, top + (body - said.size().y) * 0.58);
    p.galley(at, said, ink);
}

/// The accent, deepened so it sits with cloth: more on a dark page.
fn silk(t: &Tokens) -> Color32 {
    let deeper = if t.dark { 0.34 } else { 0.1 };
    t.accent.lerp_to_gamma(Color32::BLACK, deeper)
}

/// A whole percent; any reading at all shows at least 1.
fn percent(done: f32) -> u32 {
    ((done.clamp(0.0, 1.0) * 100.0).round() as u32).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ribbon_says_a_whole_percent() {
        assert_eq!(percent(0.001), 1);
        assert_eq!(percent(0.894), 89);
        assert_eq!(percent(1.0), 100);
        assert_eq!(percent(2.0), 100);
    }
}
