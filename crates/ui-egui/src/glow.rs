//! The AI glow: a soft rainbow circling any outline while the AI works.

// One small mesh per outline; frames come only as fast as colours move.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::epaint::Mesh;
use egui::{Color32, Context, Id, Painter, Pos2, Rect, Shape, Vec2, pos2};

use crate::motion;
use crate::theme::Tokens;

/// How the glow shows this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    /// 0 hidden to 1 full, as it fades in and out.
    pub strength: f32,
    /// 0 a travelling arc to 1 the whole ring lit, as the work finishes.
    pub whole: f32,
}

/// Blue, green, yellow and red, in order along the arc.
const HUES: [[f32; 3]; 4] = [
    [0.26, 0.52, 0.96],
    [0.20, 0.66, 0.33],
    [0.98, 0.70, 0.0],
    [0.92, 0.26, 0.21],
];
/// The crisp line's width, and the most the soft glow reaches past it.
const LINE: f32 = 1.4;
const REACH: f32 = 7.0;
/// Seconds a finished glow fades over, and its arc opens into a ring.
const FADE: f32 = 0.7;
const OPEN: f32 = 0.3;
/// Points a second the arc travels, its laps kept between these seconds.
const PACE: f32 = 140.0;
const LAP: (f32, f32) = (2.2, 4.0);
/// How much the arc speeds up and slows down within a lap.
const EASE: f32 = 0.3;
/// The most a frame may move the arc, in points.
const STEP: f32 = 1.5;
/// Seconds a breath takes with reduced motion, which holds the colours still.
const BREATH: f64 = 2.0;

/// How far the glow reaches outside `rect`.
pub fn spill(rect: Rect) -> f32 {
    LINE + reach(rect)
}

fn reach(rect: Rect) -> f32 {
    (rect.width().min(rect.height()) * 0.35).clamp(3.0, REACH)
}

/// The glow of something working, kept by `id`: it flares into a ring and fades
/// once the work ends. None while nothing shows.
pub fn state(ctx: &Context, id: Id, working: bool) -> Option<Glow> {
    let strength = ctx.animate_bool_with_time(id, working, motion::secs(ctx, FADE));
    let open = motion::secs(ctx, OPEN);
    let whole = ctx.animate_bool_with_time(id.with("whole"), !working, open);
    (strength > 0.0).then_some(Glow { strength, whole })
}

/// Paints `glow` round `rect`, its corners rounded by `radius`, and books the
/// next frame. With Animations off it still moves, as work in progress does.
pub fn paint(ctx: &Context, painter: &Painter, rect: Rect, radius: f32, glow: Glow) {
    let ring = Ring::new(rect, radius);
    let lap = (ring.length / PACE).clamp(LAP.0, LAP.1);
    let t = ctx.input(|i| i.time);
    let still = motion::reduced(ctx);
    let phase = (t / f64::from(lap)).rem_euclid(1.0) as f32;
    let breath = (TAU * (t / BREATH).rem_euclid(1.0) as f32).cos();
    let look = match still {
        true => Look {
            head: 0.125,
            drift: 0.0,
            strength: glow.strength * (0.75 + 0.25 * breath),
            ..Look::new(ctx, glow)
        },
        false => Look {
            head: phase + EASE * (TAU * phase).sin() / TAU,
            drift: (t * 0.04).rem_euclid(1.0) as f32 + 0.05 * (t * 0.6).sin() as f32,
            ..Look::new(ctx, glow)
        },
    };
    painter.add(Shape::mesh(ring.mesh(&look)));
    // Wake once the arc has moved a step; a breath needs fewer frames.
    let speed = ring.length / lap * (1.0 + EASE);
    let wait = match still {
        true => 1.0 / 15.0,
        false => (STEP / speed).clamp(1.0 / 60.0, 1.0 / 24.0),
    };
    ctx.request_repaint_after_secs(wait);
    count(ctx);
}

/// Glows painted in the last whole pass, for tests.
pub fn shown(ctx: &Context) -> usize {
    let n: Option<Count> = ctx.data(|d| d.get_temp(count_id()));
    let last = ctx.cumulative_pass_nr().saturating_sub(1);
    n.filter(|c| c.pass >= last).map_or(0, |c| c.n)
}

#[derive(Clone, Copy, Default)]
struct Count {
    pass: u64,
    n: usize,
}

fn count_id() -> Id {
    Id::new("ai-glow-count")
}

fn count(ctx: &Context) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let c: &mut Count = d.get_temp_mut_or_default(count_id());
        c.n = if c.pass == pass { c.n + 1 } else { 1 };
        c.pass = pass;
    });
}

/// Where the arc is this frame and how it is coloured.
struct Look {
    /// The arc's middle, as a share of the way round.
    head: f32,
    /// The colours' slow wander, in turns of the palette.
    drift: f32,
    strength: f32,
    whole: f32,
    dark: bool,
}

impl Look {
    fn new(ctx: &Context, glow: Glow) -> Self {
        Self {
            head: 0.0,
            drift: 0.0,
            strength: glow.strength,
            whole: glow.whole,
            dark: Tokens::get(ctx).dark,
        }
    }

    /// The colour and opacity at `share` of the way round a ring `length` long.
    fn at(&self, share: f32, length: f32) -> ([f32; 3], f32) {
        // Short rings light all round; long ones carry a travelling arc.
        let half = (90.0 / length).clamp(0.22, 0.5);
        // A faint line keeps the whole shape readable behind the arc.
        let floor = (1.0 - length / 220.0).clamp(0.12, 0.7);
        let laps = if half > 0.34 { 1.0 } else { 2.0 };
        let off = share - self.head;
        let d = ((off + 0.5).rem_euclid(1.0) - 0.5).abs();
        let lit = match d < half {
            true => (FRAC_PI_2 * d / half).cos().powi(2),
            false => 0.0,
        };
        let level = floor + (1.0 - floor) * lit;
        let level = level + (0.85 - level) * self.whole;
        (hue(self.drift + laps * off), level * self.strength)
    }

    /// `rgb` at `alpha`; on dark pages lifted, and partly added as light.
    fn color(&self, rgb: [f32; 3], alpha: f32) -> Color32 {
        let a = alpha.clamp(0.0, 1.0);
        let (lift, add) = if self.dark { (0.18, 0.4) } else { (0.0, 0.0) };
        let [r, g, b] = rgb.map(|v| ((v + (1.0 - v) * lift) * a * 255.0).round() as u8);
        Color32::from_rgba_premultiplied(r, g, b, (a * (1.0 - add) * 255.0).round() as u8)
    }
}

/// The palette at `u` turns, eased between its colours.
fn hue(u: f32) -> [f32; 3] {
    let n = HUES.len();
    let x = u.rem_euclid(1.0) * n as f32;
    let i = (x.floor() as usize).min(n - 1);
    let f = x - i as f32;
    let f = f * f * (3.0 - 2.0 * f);
    let (a, b) = (HUES[i], HUES[(i + 1) % n]);
    [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * f)
}

/// An outline's points with their outward normals and shares of the way round.
struct Ring {
    points: Vec<(Pos2, Vec2)>,
    shares: Vec<f32>,
    /// Points round the ring's middle.
    length: f32,
    reach: f32,
}

impl Ring {
    fn new(rect: Rect, radius: f32) -> Self {
        let reach = reach(rect);
        let r = radius.clamp(0.0, rect.width().min(rect.height()) / 2.0);
        let step = ((rect.width() + rect.height()) / 60.0).clamp(2.0, 6.0);
        // Square corners bend too: the glow rounds them.
        let bend = (((r + reach) * FRAC_PI_2 / 3.0).ceil() as usize).clamp(3, 12);
        let (a, b) = (rect.min + Vec2::splat(r), rect.max - Vec2::splat(r));
        let corners = [
            (pos2(b.x, a.y), -FRAC_PI_2),
            (b, 0.0),
            (pos2(a.x, b.y), FRAC_PI_2),
            (a, PI),
        ];
        let mut points = Vec::new();
        let mut from = pos2(a.x, rect.top());
        for (centre, start) in corners {
            let normal = Vec2::angled(start);
            let to = centre + normal * r;
            // A side used up by its corners adds no point.
            let cuts = ((to - from).length() / step).ceil() as usize;
            for i in 0..cuts {
                points.push((from + (to - from) * (i as f32 / cuts as f32), normal));
            }
            for i in 0..bend {
                let n = Vec2::angled(start + FRAC_PI_2 * i as f32 / bend as f32);
                points.push((centre + n * r, n));
            }
            from = centre + Vec2::angled(start + FRAC_PI_2) * r;
        }
        let mid = LINE + reach * 0.3;
        let at = |(p, n): (Pos2, Vec2)| p + n * mid;
        let next = points.iter().cycle().skip(1);
        let mut shares = Vec::with_capacity(points.len());
        let mut run = 0.0;
        for (p, q) in points.iter().zip(next) {
            shares.push(run);
            run += (at(*q) - at(*p)).length();
        }
        let length = run.max(1.0);
        shares.iter_mut().for_each(|s| *s /= length);
        Self {
            points,
            shares,
            length,
            reach,
        }
    }

    /// Bands outward from inside the line, each keeping a share of the colour.
    fn mesh(&self, look: &Look) -> Mesh {
        let reach = self.reach;
        let bands = [
            (-0.7, 0.0),
            (0.0, 1.0),
            (LINE, 1.0),
            (LINE + 0.9, 0.42),
            (LINE + reach * 0.5, 0.14),
            (LINE + reach, 0.0),
        ];
        let k = bands.len() as u32;
        let n = self.points.len() as u32;
        let mut mesh = Mesh::default();
        mesh.reserve_vertices(self.points.len() * bands.len());
        mesh.reserve_triangles(self.points.len() * (bands.len() - 1) * 2);
        for ((p, normal), share) in self.points.iter().zip(&self.shares) {
            let (rgb, level) = look.at(*share, self.length);
            for (off, keep) in bands {
                mesh.colored_vertex(*p + *normal * off, look.color(rgb, level * keep));
            }
        }
        for i in 0..n {
            let j = (i + 1) % n;
            for band in 0..k - 1 {
                let (a, b) = (i * k + band, j * k + band);
                mesh.add_triangle(a, a + 1, b);
                mesh.add_triangle(a + 1, b + 1, b);
            }
        }
        mesh
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn look(head: f32) -> Look {
        Look {
            head,
            drift: 0.0,
            strength: 1.0,
            whole: 0.0,
            dark: false,
        }
    }

    #[test]
    fn the_palette_turns_without_a_seam() {
        let close = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.02);
        assert!(close(hue(0.0), hue(1.0)));
        assert!(close(hue(0.999), hue(0.0)));
        assert!(close(hue(-0.25), hue(0.75)));
    }

    #[test]
    fn a_ring_closes_and_its_shares_climb_once() {
        let sizes = [
            (vec2(8.0, 14.0), 0.0),
            (vec2(60.0, 18.0), 3.0),
            (vec2(300.0, 40.0), 20.0),
        ];
        for (size, radius) in sizes {
            let ring = Ring::new(Rect::from_min_size(Pos2::ZERO, size), radius);
            assert!(ring.shares.windows(2).all(|w| w[0] < w[1]), "{size:?}");
            assert!(ring.shares.last().is_some_and(|s| *s < 1.0));
            let first = ring.points.first().map(|p| p.0);
            assert_eq!(first, Some(pos2(radius, 0.0)), "{size:?}");
        }
    }

    #[test]
    fn a_small_ring_lights_all_round_and_a_long_one_an_arc() {
        let small = (0..20).map(|i| look(0.0).at(i as f32 / 20.0, 60.0).1);
        assert!(small.fold(1.0_f32, f32::min) > 0.5);
        let far = look(0.0).at(0.5, 800.0).1;
        let near = look(0.0).at(0.0, 800.0).1;
        assert!(far < 0.15 && near > 0.99, "{far} {near}");
    }

    #[test]
    fn a_finish_lights_the_whole_ring() {
        let done = Look {
            whole: 1.0,
            ..look(0.0)
        };
        assert!((done.at(0.5, 800.0).1 - 0.85).abs() < 1e-4);
    }
}
