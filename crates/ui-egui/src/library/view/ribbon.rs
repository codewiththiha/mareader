//! How far through a book, on a silk bookmark tucked under its cover.

use egui::epaint::Shadow;
use egui::{Color32, Painter, Rect, Stroke, pos2};

use super::face::shade;
use crate::theme::{self, Tokens};

/// How far through, as "89%" on a silk bookmark tucked under the top.
pub(super) fn ribbon(p: &Painter, face: Rect, done: f32, t: &Tokens, fade: f32) {
    let w = face.width();
    let size = (w / 16.0).clamp(9.0, 11.5);
    let percent = format!("{}%", percent(done));
    let ink = theme::ink_on(t.accent).gamma_multiply(fade);
    let said = p.layout_no_wrap(percent, theme::medium(size), ink);
    let wide = (said.size().x + size * 0.9).max(w * 0.14).round();
    let notch = (wide * 0.3).round();
    let body = (said.size().y + size * 1.3).max(wide * 1.2).round();
    let (left, top) = ((face.right() - w * 0.1 - wide).round(), face.top());
    let (right, mid) = (left + wide, left + wide / 2.0);
    let foot = top + body + notch;
    // From the top's middle, so a fan from it fills the swallowtail.
    let outline = vec![
        pos2(mid, top),
        pos2(right, top),
        pos2(right, foot),
        pos2(mid, top + body),
        pos2(left, foot),
        pos2(left, top),
    ];
    let cast = Shadow {
        offset: [0, 2],
        blur: 6,
        spread: 0,
        color: t.shadow.gamma_multiply(0.9 * fade),
    };
    let laid = Rect::from_x_y_ranges(left..=right, top..=top + body + notch * 0.5);
    p.add(cast.as_shape(laid, 0));
    // Silk: lit along its middle, deeper at its edges.
    let silk = t.accent.gamma_multiply(fade);
    let lit = t
        .accent
        .lerp_to_gamma(Color32::WHITE, 0.18)
        .gamma_multiply(fade);
    shade(p, &outline, |at| match (at.x - mid).abs() < 0.5 {
        true => lit,
        false => silk,
    });
    let edge = t
        .accent
        .lerp_to_gamma(Color32::BLACK, 0.3)
        .gamma_multiply(fade);
    p.add(egui::epaint::PathShape::closed_line(
        outline,
        Stroke::new(1.0, edge),
    ));
    // Tucked under the cover's top edge.
    let tuck = Rect::from_x_y_ranges(left..=right, top..=top + 2.0);
    p.rect_filled(
        tuck,
        0.0,
        Color32::from_black_alpha(60).gamma_multiply(fade),
    );
    let at = pos2(
        mid - said.size().x / 2.0,
        top + (body - said.size().y) * 0.62,
    );
    p.galley(at, said, ink);
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
