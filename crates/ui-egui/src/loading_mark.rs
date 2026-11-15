//! The app's loading mark: three dots stepping round a square.

// Animations off, it still moves, as in mareader; reduced motion fades it.

use egui::{Color32, Rect, Ui, Vec2, vec2};

use crate::motion;

/// A dot's side, as a share of the mark's.
const DOT: f32 = 0.4;
/// The far corner's offset: what's left after a dot.
const FAR: f32 = 1.0 - DOT;
/// Seconds for one lap of hops; for one breath of the fade.
const LAP: f64 = 1.0;
const BREATH: f64 = 1.6;

/// Where each dot's top left sits at `t` seconds, in shares of the side.
fn hops(t: f64) -> [Vec2; 3] {
    let q = (t / LAP).rem_euclid(1.0) as f32 * 4.0;
    // How far through quarter `n` the lap is, 0 to 1.
    let through = |n: f32| (q - n).clamp(0.0, 1.0);
    let a = vec2(FAR * through(0.0), FAR * through(3.0));
    let b = vec2(0.0, FAR * (1.0 - through(1.0)));
    let c = vec2(FAR * (1.0 - through(2.0)), FAR);
    [a, b, c]
}

/// The fade's opacity at `t` seconds: 0.3 at rest, 1 mid-breath.
fn breath(t: f64) -> f32 {
    let phase = (t / BREATH).rem_euclid(1.0) as f32;
    0.3 + 0.7 * (0.5 - 0.5 * (phase * std::f32::consts::TAU).cos())
}

/// Paints the mark in `rect`'s largest centred square.
pub fn paint(ui: &Ui, rect: Rect, color: Color32) {
    let ctx = ui.ctx();
    let t = ctx.input(|i| i.time);
    let side = rect.width().min(rect.height());
    let square = Rect::from_center_size(rect.center(), Vec2::splat(side));
    let reduced = motion::reduced(ctx);
    let (spots, alpha) = if reduced {
        ([vec2(0.0, 0.0), vec2(0.0, FAR), vec2(FAR, FAR)], breath(t))
    } else {
        (hops(t), 1.0)
    };
    let r = side * DOT / 2.0 * 0.9;
    for at in spots {
        let centre = square.min + (at + Vec2::splat(DOT / 2.0)) * side;
        ui.painter()
            .circle_filled(centre, r, color.gamma_multiply(alpha));
    }
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_lap_ends_where_it_began_with_the_dots_traded() {
        let start = hops(0.0);
        let end = hops(LAP - 1e-6);
        // Every resting corner is taken again, by another dot.
        for spot in start {
            assert!(end.iter().any(|e| near(*e, spot)), "{spot:?} in {end:?}");
        }
        assert!(!near(start[0], end[0]), "the first dot moved on");
    }

    #[test]
    fn one_dot_moves_per_quarter() {
        let moved = |from: f64, to: f64| {
            let (a, b) = (hops(from), hops(to));
            (0..3).filter(|&i| !near(a[i], b[i])).count()
        };
        for q in 0..4 {
            let at = f64::from(q) * LAP / 4.0;
            assert_eq!(moved(at + 0.01, at + 0.2), 1, "quarter {q}");
        }
    }

    #[test]
    fn the_fade_breathes_between_its_floor_and_full() {
        assert!((breath(0.0) - 0.3).abs() < 1e-4);
        assert!((breath(BREATH / 2.0) - 1.0).abs() < 1e-4);
        assert!((breath(BREATH) - 0.3).abs() < 1e-3);
    }
}
