//! Authored timeline sampling and easing.

use crate::generated::{EASINGS, Ease};
use crate::spring::evaluate as spring;
use crate::{Pack, Part, Prop, Seg};

#[derive(Clone, Copy)]
pub(super) struct Vals {
    pub(super) x: f32,
    pub(super) y: f32,
    pub(super) scale: f32,
    pub(super) scale_x: f32,
    pub(super) scale_y: f32,
    pub(super) rot: f32,
    pub(super) skew_x: f32,
    pub(super) skew_y: f32,
    pub(super) op: f32,
    pub(super) len: f32,
    pub(super) off: f32,
    pub(super) width: f32,
}

impl Vals {
    fn rest(part: &Part) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rot: 0.0,
            skew_x: 0.0,
            skew_y: 0.0,
            op: 1.0,
            len: 1.0,
            off: 0.0,
            width: part.width,
        }
    }
}

pub(super) fn values(pack: &Pack, time: f32) -> Vec<Vals> {
    let mut vals: Vec<Vals> = pack.parts.iter().map(Vals::rest).collect();
    for seg in pack.segs {
        let Some(value) = vals.get_mut(seg.part as usize) else {
            continue;
        };
        let x = at(seg, clock(seg, time));
        match seg.prop {
            Prop::X => value.x = x,
            Prop::Y => value.y = x,
            Prop::Scale => value.scale = x,
            Prop::ScaleX => value.scale_x = x,
            Prop::ScaleY => value.scale_y = x,
            Prop::Rotate => value.rot = x,
            Prop::SkewX => value.skew_x = x,
            Prop::SkewY => value.skew_y = x,
            Prop::Opacity => value.op = x,
            Prop::PathLen => value.len = x,
            Prop::PathOff => value.off = x,
            Prop::StrokeW => value.width = x,
        }
    }
    vals
}

fn clock(seg: &Seg, time: f32) -> f32 {
    if seg.dur <= 0.0 {
        return if time >= seg.t0 { 1.0 } else { 0.0 };
    }
    ((time - seg.t0) / seg.dur).clamp(0.0, 1.0)
}

fn at(seg: &Seg, progress: f32) -> f32 {
    let count = seg.keys.len();
    match count {
        0 => 0.0,
        1 => seg.keys[0],
        _ => {
            let gaps = count - 1;
            let (index, fraction) = if seg.times.len() == count {
                let mut index = 0usize;
                while index + 2 < count && seg.times[index + 1] <= progress {
                    index += 1;
                }
                let span = (seg.times[index + 1] - seg.times[index]).max(1.0e-6);
                (
                    index,
                    ((progress - seg.times[index]) / span).clamp(0.0, 1.0),
                )
            } else {
                let position = (progress * gaps as f32).clamp(0.0, gaps as f32);
                let index = (position as usize).min(gaps - 1);
                (index, (position - index as f32).clamp(0.0, 1.0))
            };
            let easing = seg
                .eases
                .get(index.min(seg.eases.len().saturating_sub(1)))
                .copied()
                .unwrap_or(0);
            let eased = match EASINGS.get(easing as usize).copied() {
                Some(Ease::Bezier(points)) => curve(points, fraction),
                Some(Ease::Spring(zeta)) => spring(zeta, fraction),
                _ => fraction,
            };
            seg.keys[index] + (seg.keys[index + 1] - seg.keys[index]) * eased
        }
    }
}

fn curve([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let axis = |a: f32, b: f32, time: f32| {
        let rest = 1.0 - time;
        3.0 * rest * rest * time * a + 3.0 * rest * time * time * b + time * time * time
    };
    let (mut low, mut high) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let middle = (low + high) * 0.5;
        if axis(x1, x2, middle) < x {
            low = middle;
        } else {
            high = middle;
        }
    }
    axis(y1, y2, (low + high) * 0.5)
}

#[cfg(test)]
mod timeline_tests {
    use super::*;

    #[test]
    fn curves_keep_their_endpoints_and_shape() {
        let linear = [0.0, 0.0, 1.0, 1.0];
        assert_eq!(curve(linear, 0.0), 0.0);
        assert_eq!(curve(linear, 1.0), 1.0);
        assert!((curve(linear, 0.3) - 0.3).abs() < 1.0e-3);
        assert!(curve([0.2, 0.9, 0.2, 1.0], 0.3) > 0.5);
        assert!(curve([0.34, 1.56, 0.64, 1.0], 0.5) > 1.0);
    }

    #[test]
    fn springs_overshoot_and_settle() {
        assert_eq!(spring(0.5, 0.0), 0.0);
        assert_eq!(spring(0.5, 1.0), 1.0);
        assert!(spring(0.5, 0.4) > 1.0);
        assert!((spring(0.5, 0.99) - 1.0).abs() < 0.01);
        assert!(spring(0.78, 0.4) < spring(0.5, 0.4));
    }

    #[test]
    fn segments_hold_before_and_after_themselves() {
        let segment = Seg {
            part: 0,
            prop: Prop::X,
            t0: 0.1,
            dur: 0.2,
            keys: &[0.0, -6.0],
            times: &[],
            eases: &[0],
        };
        assert_eq!(at(&segment, clock(&segment, 0.0)), 0.0);
        assert_eq!(at(&segment, clock(&segment, 0.2)), -3.0);
        assert_eq!(at(&segment, clock(&segment, 0.9)), -6.0);
        let snap = Seg {
            dur: 0.0,
            keys: &[1.0, 0.0],
            ..segment
        };
        assert_eq!(at(&snap, clock(&snap, 0.09)), 1.0);
        assert_eq!(at(&snap, clock(&snap, 0.1)), 0.0);
    }

    #[test]
    fn explicit_key_times_place_each_gap() {
        let segment = Seg {
            part: 0,
            prop: Prop::Scale,
            t0: 0.0,
            dur: 0.4,
            keys: &[1.0, 0.6, 1.0],
            times: &[0.0, 0.6, 1.0],
            eases: &[0, 0],
        };
        let down = at(&segment, clock(&segment, 0.2));
        assert!((down - (1.0 - 0.4 * (0.5 / 0.6))).abs() < 1.0e-4);
        assert!((at(&segment, clock(&segment, 0.24)) - 0.6).abs() < 1.0e-5);
        assert!((at(&segment, clock(&segment, 0.4)) - 1.0).abs() < 1.0e-5);
    }
}
