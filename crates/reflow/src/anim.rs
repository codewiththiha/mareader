//! CSS transforms, transitions, hover states, keyframes, and multicol.
use std::sync::Arc;

use vello_cpu::kurbo::Affine;

use crate::block::{Edge, Length, Shadow, TextShadow};

/// Horizontal spacing inside an inline run, in ems.
#[derive(Debug, Clone, Copy)]
pub struct InlineSpace {
    /// Byte offset inside [`crate::block::Text::text`] where the gap sits.
    pub at: usize,
    /// Width of the gap in ems of the enclosing text size.
    pub width: f32,
}

impl PartialEq for InlineSpace {
    fn eq(&self, other: &Self) -> bool {
        self.at == other.at && self.width.to_bits() == other.width.to_bits()
    }
}

impl Eq for InlineSpace {}

/// Independent horizontal and vertical background tiling (`background-repeat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Repeat {
    pub x: bool,
    pub y: bool,
}

impl Default for Repeat {
    fn default() -> Self {
        Self::BOTH
    }
}

impl Repeat {
    pub const BOTH: Self = Self { x: true, y: true };
    pub const NONE: Self = Self { x: false, y: false };
    pub const X: Self = Self { x: true, y: false };
    pub const Y: Self = Self { x: false, y: true };

    pub fn any(self) -> bool {
        self.x || self.y
    }
}

/// CSS multi-column box settings (`column-count`, `width`, `gap`, `rule`).
#[derive(Debug, Clone, PartialEq)]
pub struct MultiCol {
    /// Explicit column count (`0` means auto, derived from `width`).
    pub count: u8,
    /// Preferred column width when `count` is auto or as a minimum guide.
    pub width: Option<Length>,
    /// Gap between adjacent columns (default `1em`).
    pub gap: Length,
    /// Column rule width in root ems (`0.0` draws no rule).
    pub rule_width: f32,
    pub rule_style: Edge,
    pub rule_colour: Option<[u8; 4]>,
}

impl Default for MultiCol {
    fn default() -> Self {
        Self {
            count: 0,
            width: None,
            gap: Length::Em(1.0),
            rule_width: 0.0,
            rule_style: Edge::None,
            rule_colour: None,
        }
    }
}

/// 2D CSS transform around `origin` (`[0.5, 0.5]` = centre).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform2d {
    pub translate_em: [f32; 2],
    pub translate_pct: [f32; 2],
    pub scale: [f32; 2],
    pub rotate_deg: f32,
    pub skew_deg: [f32; 2],
    pub origin: [f32; 2],
}

impl Default for Transform2d {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform2d {
    pub const IDENTITY: Self = Self {
        translate_em: [0.0, 0.0],
        translate_pct: [0.0, 0.0],
        scale: [1.0, 1.0],
        rotate_deg: 0.0,
        skew_deg: [0.0, 0.0],
        origin: [0.5, 0.5],
    };

    pub fn is_identity(&self) -> bool {
        self.translate_em[0].abs() < 1e-4
            && self.translate_em[1].abs() < 1e-4
            && self.translate_pct[0].abs() < 1e-4
            && self.translate_pct[1].abs() < 1e-4
            && (self.scale[0] - 1.0).abs() < 1e-4
            && (self.scale[1] - 1.0).abs() < 1e-4
            && self.rotate_deg.abs() < 1e-4
            && self.skew_deg[0].abs() < 1e-4
            && self.skew_deg[1].abs() < 1e-4
    }

    pub fn lerp(&self, to: &Self, t: f32) -> Self {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        Self {
            translate_em: [
                mix(self.translate_em[0], to.translate_em[0]),
                mix(self.translate_em[1], to.translate_em[1]),
            ],
            translate_pct: [
                mix(self.translate_pct[0], to.translate_pct[0]),
                mix(self.translate_pct[1], to.translate_pct[1]),
            ],
            scale: [
                mix(self.scale[0], to.scale[0]),
                mix(self.scale[1], to.scale[1]),
            ],
            rotate_deg: mix(self.rotate_deg, to.rotate_deg),
            skew_deg: [
                mix(self.skew_deg[0], to.skew_deg[0]),
                mix(self.skew_deg[1], to.skew_deg[1]),
            ],
            origin: [
                mix(self.origin[0], to.origin[0]),
                mix(self.origin[1], to.origin[1]),
            ],
        }
    }

    /// The 2D affine matrix around `rect`'s origin.
    pub fn affine(&self, rect: [f32; 4], em: f32) -> Affine {
        if self.is_identity() {
            return Affine::IDENTITY;
        }
        let [x, y, w, h] = rect;
        let ox = f64::from(x + w * self.origin[0]);
        let oy = f64::from(y + h * self.origin[1]);
        let tx = f64::from(self.translate_em[0] * em + w * self.translate_pct[0] / 100.0);
        let ty = f64::from(self.translate_em[1] * em + h * self.translate_pct[1] / 100.0);
        let sx = f64::from(self.scale[0]);
        let sy = f64::from(self.scale[1]);
        let rot = f64::from(self.rotate_deg).to_radians();
        let kx = f64::from(self.skew_deg[0]).to_radians().tan();
        let ky = f64::from(self.skew_deg[1]).to_radians().tan();
        let skew = Affine::new([1.0, ky, kx, 1.0, 0.0, 0.0]);
        Affine::translate((ox + tx, oy + ty))
            * Affine::rotate(rot)
            * skew
            * Affine::scale_non_uniform(sx, sy)
            * Affine::translate((-ox, -oy))
    }

    /// Affine coefficients `[a, b, c, d, tx, ty]` in `rect`'s units.
    pub fn affine_coeffs(&self, rect: [f32; 4], em: f32) -> [f64; 6] {
        self.affine(rect, em).as_coeffs()
    }
}

/// CSS timing function (`linear`, `ease`, `cubic-bezier`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Linear,
    CubicBezier(f32, f32, f32, f32),
    Steps(u16),
}

impl Default for Easing {
    fn default() -> Self {
        Self::EASE
    }
}

impl Easing {
    pub const EASE: Self = Self::CubicBezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Self = Self::CubicBezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Self = Self::CubicBezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Self = Self::CubicBezier(0.42, 0.0, 0.58, 1.0);

    /// Evaluates the easing curve at progress `t` in `0.0..=1.0`.
    pub fn eval(self, t: f32) -> f32 {
        let x = t.clamp(0.0, 1.0);
        if let Self::Steps(n) = self {
            let steps = f32::from(n.max(1));
            return if x >= 1.0 {
                1.0
            } else {
                (x * steps).floor() / steps
            };
        }
        let Self::CubicBezier(x1, y1, x2, y2) = self else {
            return x;
        };
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let sample = |a: f32, b: f32, u: f32| {
            let v = 1.0 - u;
            3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
        };
        let slope = |a: f32, b: f32, u: f32| {
            let v = 1.0 - u;
            3.0 * v * v * a + 6.0 * v * u * (b - a) + 3.0 * u * u * (1.0 - b)
        };
        let (mut lo, mut hi, mut u) = (0.0f32, 1.0f32, x);
        for _ in 0..8 {
            let err = sample(x1, x2, u) - x;
            if err.abs() < 1e-5 {
                break;
            }
            if err < 0.0 {
                lo = u;
            } else {
                hi = u;
            }
            let d = slope(x1, x2, u);
            let next = if d.abs() > 1e-5 {
                u - err / d
            } else {
                (lo + hi) * 0.5
            };
            u = if next > lo && next < hi {
                next
            } else {
                (lo + hi) * 0.5
            };
        }
        sample(y1, y2, u)
    }
}

/// CSS `transition` timing specification.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionSpec {
    pub duration_ms: u32,
    pub delay_ms: u32,
    pub easing: Easing,
}

impl Default for TransitionSpec {
    fn default() -> Self {
        Self {
            duration_ms: 0,
            delay_ms: 0,
            easing: Easing::EASE,
        }
    }
}

/// Visual overrides active when the pointer hovers an element (`:hover`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HoverDelta {
    pub transform: Option<Transform2d>,
    pub opacity: Option<f32>,
    pub fill: Option<[u8; 4]>,
    pub border_colour: Option<[u8; 4]>,
    pub text_colour: Option<[u8; 4]>,
    pub shadows: Vec<Shadow>,
    pub text_shadow: Option<TextShadow>,
}

impl HoverDelta {
    pub fn is_empty(&self) -> bool {
        self.transform.is_none()
            && self.opacity.is_none()
            && self.fill.is_none()
            && self.border_colour.is_none()
            && self.text_colour.is_none()
            && self.shadows.is_empty()
            && self.text_shadow.is_none()
    }
}

/// One stop of a CSS `@keyframes` rule (`from` = 0.0, `to` = 1.0).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeStop {
    pub at: f32,
    pub transform: Option<Transform2d>,
    pub opacity: Option<f32>,
    pub fill: Option<[u8; 4]>,
}

/// A resolved CSS `animation` bound to its `@keyframes` stops.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationSpec {
    pub duration_ms: u32,
    pub delay_ms: u32,
    pub easing: Easing,
    /// `f32::INFINITY` represents `infinite`.
    pub iterations: f32,
    pub alternate: bool,
    pub reverse: bool,
    pub forwards: bool,
    pub stops: Arc<[KeyframeStop]>,
}

/// One sampled frame of an active `@keyframes` animation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationSample {
    pub transform: Option<Transform2d>,
    pub opacity: Option<f32>,
    pub fill: Option<[u8; 4]>,
    pub active: bool,
}

impl AnimationSpec {
    /// Evaluates the animation at `elapsed_s` seconds after mount.
    pub fn sample(&self, elapsed_s: f32) -> AnimationSample {
        let dur = self.duration_ms as f32 / 1000.0;
        let delay = self.delay_ms as f32 / 1000.0;
        if dur <= 1e-4 || self.stops.is_empty() || self.iterations <= 0.0 {
            return AnimationSample {
                transform: None,
                opacity: None,
                fill: None,
                active: false,
            };
        }
        let t = elapsed_s - delay;
        if t < 0.0 {
            return AnimationSample {
                transform: None,
                opacity: None,
                fill: None,
                active: true,
            };
        }
        let total = dur * self.iterations;
        let done = self.iterations.is_finite() && t >= total;
        if done && !self.forwards {
            return AnimationSample {
                transform: None,
                opacity: None,
                fill: None,
                active: false,
            };
        }
        let progress = if done { self.iterations } else { t / dur };
        let cycle =
            (progress.floor() as u32).saturating_sub(u32::from(done && progress.fract() == 0.0));
        let mut frac = if done && progress.fract() == 0.0 {
            1.0
        } else {
            progress.fract()
        };
        let backwards = self.reverse ^ (self.alternate && cycle % 2 == 1);
        if backwards {
            frac = 1.0 - frac;
        }
        let u = self.easing.eval(frac);
        let (transform, opacity, fill) = interpolate_stops(&self.stops, u);
        AnimationSample {
            transform,
            opacity,
            fill,
            active: !done,
        }
    }
}

fn interpolate_stops(
    stops: &[KeyframeStop],
    u: f32,
) -> (Option<Transform2d>, Option<f32>, Option<[u8; 4]>) {
    let pair = |has: fn(&KeyframeStop) -> bool| -> Option<(&KeyframeStop, &KeyframeStop, f32)> {
        let prev = stops.iter().rev().find(|s| s.at <= u + 1e-4 && has(s));
        let next = stops.iter().find(|s| s.at >= u - 1e-4 && has(s));
        match (prev, next) {
            (Some(a), Some(b)) => {
                let span = (b.at - a.at).abs();
                let k = if span > 1e-4 {
                    ((u - a.at) / span).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                Some((a, b, k))
            }
            (Some(a), None) => Some((a, a, 0.0)),
            (None, Some(b)) => Some((b, b, 1.0)),
            (None, None) => None,
        }
    };
    let transform = pair(|s| s.transform.is_some())
        .and_then(|(a, b, k)| Some(a.transform?.lerp(&b.transform?, k)));
    let opacity = pair(|s| s.opacity.is_some()).and_then(|(a, b, k)| {
        let (va, vb) = (a.opacity?, b.opacity?);
        Some(va + (vb - va) * k)
    });
    let fill = pair(|s| s.fill.is_some()).and_then(|(a, b, k)| {
        let ([r0, g0, b0, a0], [r1, g1, b1, a1]) = (a.fill?, b.fill?);
        let mix = |x: u8, y: u8| {
            (f32::from(x) + (f32::from(y) - f32::from(x)) * k)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        Some([mix(r0, r1), mix(g0, g1), mix(b0, b1), mix(a0, a1)])
    });
    (transform, opacity, fill)
}

/// Hover/transition and keyframe animation metadata on an element.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Interaction {
    pub transition: Option<TransitionSpec>,
    pub hover: Option<HoverDelta>,
    pub animation: Option<AnimationSpec>,
}

impl Interaction {
    pub fn has_hover(&self) -> bool {
        self.hover.as_ref().is_some_and(|h| !h.is_empty())
    }

    pub fn has_animation(&self) -> bool {
        self.animation.as_ref().is_some_and(|a| !a.stops.is_empty())
    }

    pub fn is_empty(&self) -> bool {
        !self.has_hover() && !self.has_animation()
    }
}

/// A laid-out interactive or animated region on a rendered page.
#[derive(Debug, Clone)]
pub struct InteractiveItem {
    pub id: u64,
    /// Bounding box in shown page units (hit-test & base placement rect).
    pub rect: [f32; 4],
    pub z: i32,
    /// Element em size in shown page units.
    pub em: f32,
    pub base_transform: Transform2d,
    pub base_opacity: f32,
    pub interaction: Arc<Interaction>,
    pub(crate) deco_indices: Vec<usize>,
    pub(crate) piece_indices: Vec<usize>,
    pub(crate) laid_rect: [f32; 4],
    pub(crate) laid_em: f32,
}

/// A standalone rasterized interactive item with its page-space placement rect.
#[derive(Debug, Clone)]
pub struct ItemPixmap {
    pub pixels: crate::raster::Pixels,
    /// Placement rect in shown page units (includes shadow/outline bleed).
    pub rect: [f32; 4],
}
