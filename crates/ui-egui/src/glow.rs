//! The AI glow: rainbow light flaring round any outline while the AI works.

// One small mesh per outline; frames come only as fast as colours move.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, Context, Id, Painter, Pos2, Rect, Shape, Vec2, pos2};

use mareader_appearance::GlowPalette;

use crate::motion;
use crate::theme::Tokens;

mod mesh;
pub(crate) mod outline;

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

/// The travel clock pauses in focus and resumes without a jump on retry.
#[derive(Clone, Copy)]
pub(crate) struct Clock {
    pub(crate) travel: f64,
    last: f64,
    working: bool,
}

impl Clock {
    pub(crate) fn new(now: f64, working: bool) -> Self {
        Self {
            travel: now,
            last: now,
            working,
        }
    }

    pub(crate) fn advance(&mut self, now: f64, working: bool) {
        if self.working {
            self.travel += (now - self.last).max(0.0);
        }
        self.last = now;
        self.working = working;
    }
}

/// A retained glow opens into focus, then uses the usual exit on close.
pub(crate) fn retained_state(ctx: &Context, id: Id, working: bool, focused: bool) -> Option<Glow> {
    let active = working || focused;
    let mut glow = state(ctx, id, active)?;
    let whole = ctx.animate_bool_with_time(
        id.with("focus-whole"),
        focused || !active,
        motion::secs(ctx, OPEN),
    );
    glow.whole = glow.whole.max(whole);
    Some(glow)
}

/// An outline to be lit by the shared flare renderer.
pub struct Outline<'a>(OutlineKind<'a>);

enum OutlineKind<'a> {
    Rect(Rect),
    Path {
        points: &'a [Pos2],
        reference: Rect,
        caches: &'a mut Vec<outline::Cache>,
        key: (usize, usize),
        travel: f64,
    },
}

impl From<Rect> for Outline<'_> {
    fn from(rect: Rect) -> Self {
        Self(OutlineKind::Rect(rect))
    }
}

impl<'a> Outline<'a> {
    pub(crate) fn path(
        points: &'a [Pos2],
        reference: Rect,
        caches: &'a mut Vec<outline::Cache>,
        key: (usize, usize),
        travel: f64,
    ) -> Self {
        Self(OutlineKind::Path {
            points,
            reference,
            caches,
            key,
            travel,
        })
    }
}

/// Paints the same flare around a rectangle or an irregular closed outline.
/// The radius applies to rectangles; a path supplies its own corners.
pub fn paint<'a>(
    ctx: &Context,
    painter: &Painter,
    tokens: &Tokens,
    outline: impl Into<Outline<'a>>,
    radius: f32,
    glow: Glow,
) -> bool {
    let (mesh, length) = match outline.into().0 {
        OutlineKind::Rect(rect) => {
            let ring = Ring::new(rect, radius);
            let look = look_at(ctx, tokens, glow, ring.length, ctx.input(|i| i.time));
            (ring.mesh(&look), ring.length)
        }
        OutlineKind::Path {
            points,
            reference,
            caches,
            key,
            travel,
        } => {
            let reach = reach(reference);
            let visible = points
                .iter()
                .zip(points.iter().cycle().skip(1))
                .any(|(a, b)| {
                    Rect::from_two_pos(*a, *b)
                        .expand(reach)
                        .intersects(painter.clip_rect())
                });
            if !visible {
                return false;
            }
            let Some((origin, geometry)) = outline::cached(points, reach, caches, key) else {
                return false;
            };
            let look = look_at(ctx, tokens, glow, geometry.length, travel);
            (mesh::path(geometry, origin, reach, &look), geometry.length)
        }
    };
    painter.add(Shape::mesh(mesh));
    wake(ctx, length);
    true
}

fn look_at(ctx: &Context, tokens: &Tokens, glow: Glow, length: f32, travel_time: f64) -> Look {
    let lap = (length / PACE).clamp(LAP.0, LAP.1);
    let t = ctx.input(|i| i.time);
    let phase = (travel_time / f64::from(lap)).rem_euclid(1.0) as f32;
    let breath = (TAU * (t / BREATH).rem_euclid(1.0) as f32).cos();
    let light = Light::new(&tokens.glow);
    match motion::reduced(ctx) {
        true => Look {
            head: 0.125,
            drift: 0.0,
            strength: glow.strength * (0.75 + 0.25 * breath),
            ..Look::new(light, glow)
        },
        false => Look {
            head: head(phase),
            drift: drift(t),
            ..Look::new(light, glow)
        },
    }
}

fn wake(ctx: &Context, length: f32) {
    let lap = (length / PACE).clamp(LAP.0, LAP.1);
    let speed = length / lap * (1.0 + EASE);
    let wait = match motion::reduced(ctx) {
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
        }
    }
}

#[cfg(test)]
mod tests;
