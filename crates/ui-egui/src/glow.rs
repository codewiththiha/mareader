//! The AI glow: rainbow light flaring round any outline while the AI works.

// One small mesh per outline; frames come only as fast as colours move.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::epaint::Mesh;
use egui::{Color32, Context, Id, Painter, Pos2, Rect, Shape, Vec2, pos2};

use mareader_appearance::GlowPalette;

use crate::motion;
use crate::theme::Tokens;

mod path;

/// How the glow shows this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glow {
    /// 0 hidden to 1 full, as it fades in and out.
    pub strength: f32,
    /// 0 a travelling arc to 1 the whole ring lit, as the work finishes.
    pub whole: f32,
}

/// The most the light reaches past its outline, in points.
const REACH: f32 = 18.0;
/// Rings of light outward, as shares of the reach, and how bright each is.
const FALLOFF: [(f32, f32); 6] = [
    (0.0, 1.0),
    (0.15, 0.86),
    (0.35, 0.56),
    (0.6, 0.26),
    (0.82, 0.08),
    (1.0, 0.0),
];
const SOFT_FALLOFF: [(f32, f32); 6] = [
    (0.0, 0.70),
    (0.15, 0.78),
    (0.35, 0.56),
    (0.6, 0.28),
    (0.82, 0.09),
    (1.0, 0.0),
];
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
    reach(rect)
}

fn reach(rect: Rect) -> f32 {
    (rect.width().min(rect.height()) * 0.55).clamp(4.0, REACH)
}

/// The glow of something working, kept by `id`: it flares into a ring and fades
/// once the work ends. None while nothing shows.
pub fn state(ctx: &Context, id: Id, working: bool) -> Option<Glow> {
    let strength = ctx.animate_bool_with_time(id, working, motion::secs(ctx, FADE));
    let open = motion::secs(ctx, OPEN);
    let whole = ctx.animate_bool_with_time(id.with("whole"), !working, open);
    (strength > 0.0).then_some(Glow { strength, whole })
}

/// Paints `glow` round `rect` in the colours of `tokens`, its corners rounded
/// by `radius`, and books the next frame. With Animations off it still moves.
pub fn paint(
    ctx: &Context,
    painter: &Painter,
    tokens: &Tokens,
    rect: Rect,
    radius: f32,
    glow: Glow,
) {
    let time = ctx.input(|i| i.time);
    paint_ring(ctx, painter, tokens, Ring::new(rect, radius), glow, time);
}

/// Paint the same flare around a closed, sampled selection contour.
/// Returns false when no part of the glow reaches the painter's clip.
pub(crate) fn paint_path(
    ctx: &Context,
    painter: &Painter,
    tokens: &Tokens,
    points: &[Pos2],
    reference: Rect,
    glow: Glow,
    travel_time: f64,
) -> bool {
    let Some(ring) = Ring::path(points, reach(reference) * 1.2) else {
        return false;
    };
    let clip = painter.clip_rect();
    let visible = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .any(|(a, b)| {
            Rect::from_two_pos(*a, *b)
                .expand(ring.reach)
                .intersects(clip)
        });
    if visible {
        paint_ring(ctx, painter, tokens, ring, glow, travel_time);
    }
    visible
}

fn paint_ring(
    ctx: &Context,
    painter: &Painter,
    tokens: &Tokens,
    ring: Ring,
    glow: Glow,
    travel_time: f64,
) {
    let lap = (ring.length / PACE).clamp(LAP.0, LAP.1);
    let t = ctx.input(|i| i.time);
    let still = motion::reduced(ctx);
    let phase = (travel_time / f64::from(lap)).rem_euclid(1.0) as f32;
    let breath = (TAU * (t / BREATH).rem_euclid(1.0) as f32).cos();
    let soft = ring.limits.is_some();
    let mut light = Light::new(&tokens.glow);
    if soft {
        light.strength *= 0.78;
        for color in &mut light.hues {
            let grey = (color[0] + color[1] + color[2]) / 3.0;
            for channel in color {
                *channel = grey + (*channel - grey) * 0.72;
            }
        }
    }
    let look = match still {
        true => Look {
            head: 0.125,
            drift: 0.0,
            strength: glow.strength
                * (0.75 + 0.25 * breath)
                * if soft { light.strength } else { 1.0 },
            ..Look::new(light, glow)
        },
        false => Look {
            head: head(phase),
            drift: drift(t),
            ..Look::new(light, glow)
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

/// The arc's middle at `phase` of its lap, quick then slow.
fn head(phase: f32) -> f32 {
    phase + EASE * (TAU * phase).sin() / TAU
}

/// How far the colours have wandered by `t`, in turns of the palette.
fn drift(t: f64) -> f32 {
    (t * 0.04).rem_euclid(1.0) as f32 + 0.05 * (t * 0.6).sin() as f32
}

/// The colour `share` of the way round a short glow now, as a pane's
/// glow wears it: the AI stack's folded ring matches the panes so.
pub fn tint(ctx: &Context, tokens: &Tokens, share: f32) -> Color32 {
    let light = Light::new(&tokens.glow);
    let t = ctx.input(|i| i.time);
    let phase = (t / f64::from(LAP.0)).rem_euclid(1.0) as f32;
    let (at, wander) = match motion::reduced(ctx) {
        true => (0.0, 0.0),
        false => (head(phase), drift(t)),
    };
    let rgb = light.hue(wander + share - at);
    let [r, g, b] = rgb.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color32::from_rgb(r, g, b)
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
    light: Light,
}

impl Look {
    fn new(light: Light, glow: Glow) -> Self {
        Self {
            head: 0.0,
            drift: 0.0,
            strength: glow.strength * light.strength,
            whole: glow.whole,
            light,
        }
    }

    /// The colour and brightness `share` of the way round a ring `length` long.
    fn at(&self, share: f32, length: f32) -> ([f32; 3], f32) {
        // Short rings light all round; long ones carry a travelling arc.
        let half = (110.0 / length).clamp(0.25, 0.5);
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
        (self.light.hue(self.drift + laps * off), level)
    }

    /// `rgb` at `alpha`, partly added as light where the look asks.
    fn color(&self, rgb: [f32; 3], alpha: f32) -> Color32 {
        let a = alpha.clamp(0.0, 1.0);
        let [r, g, b] = rgb.map(|v| (v * a * 255.0).round() as u8);
        let cover = a * (1.0 - self.light.add);
        Color32::from_rgba_premultiplied(r, g, b, (cover * 255.0).round() as u8)
    }
}

/// The look's glow colours, ready to blend.
#[derive(Clone, Copy)]
struct Light {
    hues: [[f32; 3]; 4],
    add: f32,
    strength: f32,
}

impl Light {
    fn new(p: &GlowPalette) -> Self {
        Self {
            hues: p.hues.map(|c| c.to_unit()),
            add: p.add.clamp(0.0, 1.0),
            strength: p.strength,
        }
    }

    /// The palette at `u` turns, eased between its colours.
    fn hue(&self, u: f32) -> [f32; 3] {
        let n = self.hues.len();
        let x = u.rem_euclid(1.0) * n as f32;
        let i = (x.floor() as usize).min(n - 1);
        let f = x - i as f32;
        let f = f * f * (3.0 - 2.0 * f);
        let (a, b) = (self.hues[i], self.hues[(i + 1) % n]);
        [0, 1, 2].map(|c| a[c] + (b[c] - a[c]) * f)
    }
}

/// An outline's points with their outward normals and shares of the way round.
struct Ring {
    points: Vec<(Pos2, Vec2)>,
    shares: Vec<f32>,
    /// Points round the ring's middle.
    length: f32,
    reach: f32,
    limits: Option<Vec<(f32, f32)>>,
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
        Self::from_points(points, reach)
    }

    /// A sampled closed path, with outward normals for either winding.
    fn path(path: &[Pos2], reach: f32) -> Option<Self> {
        if path.len() < 3 || path.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let bounds = path.iter().fold(Rect::NOTHING, |mut r, p| {
            r.extend_with(*p);
            r
        });
        if !bounds.is_positive() {
            return None;
        }
        let area: f64 = path
            .iter()
            .zip(path.iter().cycle().skip(1))
            .map(|(a, b)| f64::from(a.x) * f64::from(b.y) - f64::from(b.x) * f64::from(a.y))
            .sum();
        if !area.is_finite() || area.abs() < 0.01 {
            return None;
        }
        let winding = area.signum() as f32;
        let mut points = Vec::with_capacity(path.len());
        for (i, at) in path.iter().enumerate() {
            let prev = path[(i + path.len() - 1) % path.len()];
            let next = path[(i + 1) % path.len()];
            let before = (*at - prev).normalized();
            let after = (next - *at).normalized();
            let tangent = (before + after).normalized();
            let normal = egui::vec2(tangent.y, -tangent.x) * winding;
            points.push((*at, normal));
        }
        let limits = path::limits(&points, winding, reach);
        let mut ring = Self::from_points(points, reach);
        ring.limits = Some(limits);
        Some(ring)
    }

    fn from_points(points: Vec<(Pos2, Vec2)>, reach: f32) -> Self {
        let mid = reach * 0.25;
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
            limits: None,
        }
    }

    /// Light from the outline outward, reaching further where it burns hot.
    fn mesh(&self, look: &Look) -> Mesh {
        let k = FALLOFF.len() as u32 + 1;
        let n = self.points.len() as u32;
        let mut mesh = Mesh::default();
        mesh.reserve_vertices(self.points.len() * k as usize);
        mesh.reserve_triangles(self.points.len() * (k as usize - 1) * 2);
        for (i, ((p, normal), share)) in self.points.iter().zip(&self.shares).enumerate() {
            let (rgb, level) = look.at(*share, self.length);
            let (alpha, mut flare) = (level * look.strength, self.reach * (0.45 + 0.55 * level));
            let (inset, bands) =
                if let Some((inside, outside)) = self.limits.as_ref().and_then(|v| v.get(i)) {
                    flare = flare.min(*outside);
                    (*inside, SOFT_FALLOFF)
                } else {
                    (0.5, FALLOFF)
                };
            mesh.colored_vertex(*p - *normal * inset, Color32::TRANSPARENT);
            for (off, keep) in bands {
                mesh.colored_vertex(*p + *normal * off * flare, look.color(rgb, alpha * keep));
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
mod tests;
