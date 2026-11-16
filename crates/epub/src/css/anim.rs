//! CSS 2D transforms, transitions, easing, `@keyframes`, and `:hover` parsing.
use std::sync::Arc;

use mareader_reflow::{
    AnimationSpec, Easing, HoverDelta, KeyframeStop, Shadow, TextShadow, Transform2d,
    TransitionSpec,
};

use super::{Decl, Len, ROOT_PX, Stylesheet, len, split_top, words};
use crate::decor;

/// Parsed `animation*` declaration fields before resolving `@keyframes`.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AnimDecl {
    pub(crate) name: Option<Arc<str>>,
    pub(crate) duration: Option<f32>,
    pub(crate) delay: Option<f32>,
    pub(crate) easing: Option<Easing>,
    pub(crate) iterations: Option<f32>,
    pub(crate) alternate: Option<bool>,
    pub(crate) forwards: Option<bool>,
}

impl AnimDecl {
    pub(crate) fn layer(&mut self, top: &Self) {
        if top.name.is_some() {
            self.name = top.name.clone();
        }
        if top.duration.is_some() {
            self.duration = top.duration;
        }
        if top.delay.is_some() {
            self.delay = top.delay;
        }
        if top.easing.is_some() {
            self.easing = top.easing;
        }
        if top.iterations.is_some() {
            self.iterations = top.iterations;
        }
        if top.alternate.is_some() {
            self.alternate = top.alternate;
        }
        if top.forwards.is_some() {
            self.forwards = top.forwards;
        }
    }

    /// Resolves this animation against `@keyframes` in `sheets`.
    pub(crate) fn resolve(&self, sheets: &[Arc<Stylesheet>]) -> Option<AnimationSpec> {
        let name = self.name.as_deref()?;
        if name.eq_ignore_ascii_case("none") {
            return None;
        }
        let duration = self.duration.unwrap_or(0.0);
        if duration <= 0.0 {
            return None;
        }
        let stops = sheets
            .iter()
            .rev()
            .find_map(|s| s.keyframes.get(name))?
            .clone();
        if stops.is_empty() {
            return None;
        }
        Some(AnimationSpec {
            duration_ms: (duration * 1000.0).round().clamp(0.0, 600_000.0) as u32,
            delay_ms: (self.delay.unwrap_or(0.0).max(0.0) * 1000.0)
                .round()
                .clamp(0.0, 600_000.0) as u32,
            easing: self.easing.unwrap_or(Easing::EASE),
            iterations: self.iterations.unwrap_or(1.0),
            alternate: self.alternate.unwrap_or(false),
            reverse: false,
            forwards: self.forwards.unwrap_or(false),
            stops: Arc::from(stops),
        })
    }
}

/// Parses a CSS time value (`250ms`, `0.25s`) into seconds.
pub(crate) fn parse_time(v: &str) -> Option<f32> {
    let s = v.trim().to_ascii_lowercase();
    if let Some(ms) = s.strip_suffix("ms") {
        let n: f32 = ms.trim().parse().ok()?;
        return n.is_finite().then_some((n / 1000.0).clamp(0.0, 600.0));
    }
    if let Some(sec) = s.strip_suffix('s') {
        let n: f32 = sec.trim().parse().ok()?;
        return n.is_finite().then_some(n.clamp(0.0, 600.0));
    }
    None
}

/// Parses a CSS angle (`45deg`, `0.25turn`, `1rad`, `100grad`) into degrees.
pub(crate) fn parse_angle_deg(v: &str) -> Option<f32> {
    let s = v.trim().to_ascii_lowercase();
    if let Some(d) = s.strip_suffix("deg") {
        let n: f32 = d.trim().parse().ok()?;
        return n.is_finite().then_some(n);
    }
    if let Some(t) = s.strip_suffix("turn") {
        let n: f32 = t.trim().parse().ok()?;
        return n.is_finite().then_some(n * 360.0);
    }
    if let Some(g) = s.strip_suffix("grad") {
        let n: f32 = g.trim().parse().ok()?;
        return n.is_finite().then_some(n * 0.9);
    }
    if let Some(r) = s.strip_suffix("rad") {
        let n: f32 = r.trim().parse().ok()?;
        return n.is_finite().then_some(n.to_degrees());
    }
    let n: f32 = s.parse().ok()?;
    (n == 0.0).then_some(0.0)
}

/// Parses a CSS easing / timing function keyword or `cubic-bezier(...)`.
pub(crate) fn parse_easing(v: &str) -> Option<Easing> {
    let s = v.trim().to_ascii_lowercase();
    match s.as_str() {
        "linear" => return Some(Easing::Linear),
        "ease" => return Some(Easing::EASE),
        "ease-in" => return Some(Easing::EASE_IN),
        "ease-out" => return Some(Easing::EASE_OUT),
        "ease-in-out" => return Some(Easing::EASE_IN_OUT),
        "step-start" | "step-end" => return Some(Easing::Steps(1)),
        _ => {}
    }
    if let Some(inner) = s
        .strip_prefix("cubic-bezier(")
        .and_then(|r| r.strip_suffix(')'))
    {
        let nums: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().parse::<f32>().ok().filter(|n| n.is_finite()))
            .collect();
        if let [x1, y1, x2, y2] = nums.as_slice() {
            return Some(Easing::CubicBezier(
                x1.clamp(0.0, 1.0),
                *y1,
                x2.clamp(0.0, 1.0),
                *y2,
            ));
        }
    }
    if let Some(inner) = s.strip_prefix("steps(").and_then(|r| r.strip_suffix(')')) {
        let first = inner.split(',').next()?.trim();
        let n: u16 = first.parse().ok()?;
        if n > 0 {
            return Some(Easing::Steps(n));
        }
    }
    None
}

/// Parses CSS `transform` into a composed [`Transform2d`].
pub(crate) fn parse_transform(value: &str) -> Option<Transform2d> {
    let trimmed = value.trim();
    if trimmed.eq_ignore_ascii_case("none") {
        return Some(Transform2d::IDENTITY);
    }
    let mut out = Transform2d::IDENTITY;
    let mut matched = false;
    for token in words(trimmed) {
        let Some((raw_fn, rest)) = token.split_once('(') else {
            continue;
        };
        let fname = raw_fn.trim().to_ascii_lowercase();
        let inner = rest.trim_end_matches(')').trim();
        let args: Vec<&str> = inner.split(',').map(str::trim).collect();
        if apply_transform_fn(&mut out, &fname, &args).is_some() {
            matched = true;
        }
    }
    matched.then_some(out)
}

fn apply_transform_fn(out: &mut Transform2d, fname: &str, args: &[&str]) -> Option<()> {
    let add_tr = |dst_em: &mut f32, dst_pct: &mut f32, s: &str| -> Option<()> {
        match len(s)? {
            Len::Percent(p) => *dst_pct += p,
            other => *dst_em += other.px(ROOT_PX, super::BASE_WIDTH)? / ROOT_PX,
        }
        Some(())
    };
    let num = |s: &str| s.parse::<f32>().ok().filter(|n| n.is_finite());
    match fname {
        "scale" | "scale3d" => {
            let sx = args.first().and_then(|s| num(s))?;
            let sy = args.get(1).and_then(|s| num(s)).unwrap_or(sx);
            out.scale[0] *= sx;
            out.scale[1] *= sy;
        }
        "scalex" => out.scale[0] *= args.first().and_then(|s| num(s))?,
        "scaley" => out.scale[1] *= args.first().and_then(|s| num(s))?,
        "translate" | "translate3d" => {
            add_tr(
                &mut out.translate_em[0],
                &mut out.translate_pct[0],
                args.first()?,
            )?;
            if let Some(y) = args.get(1) {
                let _ = add_tr(&mut out.translate_em[1], &mut out.translate_pct[1], y);
            }
        }
        "translatex" => add_tr(
            &mut out.translate_em[0],
            &mut out.translate_pct[0],
            args.first()?,
        )?,
        "translatey" => add_tr(
            &mut out.translate_em[1],
            &mut out.translate_pct[1],
            args.first()?,
        )?,
        "rotate" | "rotatez" => out.rotate_deg += args.first().and_then(|s| parse_angle_deg(s))?,
        "skew" => {
            out.skew_deg[0] += args.first().and_then(|s| parse_angle_deg(s))?;
            out.skew_deg[1] += args.get(1).and_then(|s| parse_angle_deg(s)).unwrap_or(0.0);
        }
        "skewx" => out.skew_deg[0] += args.first().and_then(|s| parse_angle_deg(s))?,
        "skewy" => out.skew_deg[1] += args.first().and_then(|s| parse_angle_deg(s))?,
        "matrix" if args.len() == 6 => {
            let vals: Vec<f32> = args.iter().filter_map(|s| num(s)).collect();
            let [a, b, c, d, e, f] = vals.as_slice() else {
                return None;
            };
            out.scale[0] *= a.hypot(*b);
            out.scale[1] *= c.hypot(*d);
            out.rotate_deg += b.atan2(*a).to_degrees();
            out.translate_em[0] += e / ROOT_PX;
            out.translate_em[1] += f / ROOT_PX;
        }
        _ => return None,
    }
    Some(())
}

/// Parses CSS `transform-origin` into `[ox, oy]` fractions in `0..=1`.
pub(crate) fn parse_transform_origin(value: &str) -> Option<[f32; 2]> {
    let parts = words(value);
    if parts.is_empty() {
        return None;
    }
    let axis = |w: &str, horiz: bool| -> Option<f32> {
        let low = w.to_ascii_lowercase();
        match low.as_str() {
            "center" => Some(0.5),
            "left" if horiz => Some(0.0),
            "right" if horiz => Some(1.0),
            "top" if !horiz => Some(0.0),
            "bottom" if !horiz => Some(1.0),
            _ => {
                if let Some(p) = low.strip_suffix('%') {
                    p.parse::<f32>().ok().map(|n| (n / 100.0).clamp(0.0, 1.0))
                } else {
                    None
                }
            }
        }
    };
    let x = axis(parts.first()?, true).unwrap_or(0.5);
    let y = parts.get(1).and_then(|w| axis(w, false)).unwrap_or(0.5);
    Some([x, y])
}

/// Parses CSS `transition` shorthand (picks the primary duration item).
pub(crate) fn parse_transition(value: &str) -> Option<TransitionSpec> {
    let mut best: Option<TransitionSpec> = None;
    for part in split_top(value, ',') {
        let mut dur = None;
        let mut delay = 0.0f32;
        let mut easing = Easing::EASE;
        for w in words(part) {
            if let Some(t) = parse_time(w) {
                if dur.is_none() {
                    dur = Some(t);
                } else {
                    delay = t;
                }
            } else if let Some(e) = parse_easing(w) {
                easing = e;
            }
        }
        if let Some(duration) = dur {
            let candidate = TransitionSpec {
                duration_ms: (duration * 1000.0).round().clamp(0.0, 600_000.0) as u32,
                delay_ms: (delay * 1000.0).round().clamp(0.0, 600_000.0) as u32,
                easing,
            };
            if best.is_none_or(|b| candidate.duration_ms > b.duration_ms) {
                best = Some(candidate);
            }
        }
    }
    best
}

/// Parses CSS `animation` shorthand into an [`AnimDecl`].
pub(crate) fn parse_animation(value: &str) -> AnimDecl {
    let mut out = AnimDecl::default();
    let Some(first) = split_top(value, ',').into_iter().next() else {
        return out;
    };
    for w in words(first) {
        let low = w.to_ascii_lowercase();
        if let Some(t) = parse_time(&low) {
            if out.duration.is_none() {
                out.duration = Some(t);
            } else if out.delay.is_none() {
                out.delay = Some(t);
            }
        } else if let Some(e) = parse_easing(&low) {
            out.easing = Some(e);
        } else if low == "infinite" {
            out.iterations = Some(f32::INFINITY);
        } else if let Ok(n) = low.parse::<f32>()
            && n.is_finite()
            && n >= 0.0
        {
            out.iterations = Some(n);
        } else if matches!(low.as_str(), "alternate" | "alternate-reverse") {
            out.alternate = Some(true);
        } else if matches!(low.as_str(), "normal" | "reverse") {
            out.alternate = Some(false);
        } else if matches!(low.as_str(), "forwards" | "both") {
            out.forwards = Some(true);
        } else if matches!(low.as_str(), "backwards") {
            out.forwards = Some(false);
        } else if matches!(low.as_str(), "running" | "paused") {
            // Ignored for now; animations run live.
        } else if out.name.is_none() {
            out.name = Some(Arc::from(w.trim()));
        }
    }
    out
}

/// Parses the body of an `@keyframes` block into sorted [`KeyframeStop`]s.
pub(crate) fn parse_keyframes(inner: &str) -> Vec<KeyframeStop> {
    const OPEN: char = '\u{7b}';
    let mut stops = Vec::new();
    let mut rest = inner;
    while let Some(open) = rest.find(OPEN) {
        let selector = rest.get(..open).unwrap_or_default().trim();
        let after = rest.get(open + 1..).unwrap_or_default();
        let Some(close) = super::matching_brace(after) else {
            break;
        };
        let body = after.get(..close).unwrap_or_default();
        let decl = Decl::parse(body);
        let transform = decl
            .extra
            .as_deref()
            .and_then(|e| e.transform_2d)
            .or_else(|| {
                decl.rotate.map(|r| Transform2d {
                    rotate_deg: r,
                    ..Transform2d::IDENTITY
                })
            });
        let opacity = decl.opacity;
        let fill = decl
            .extra
            .as_deref()
            .and_then(|e| e.fill_rgba)
            .or_else(|| decl.fill.flatten().map(|[r, g, b]| [r, g, b, 255]));
        for part in selector.split(',') {
            let p = part.trim().to_ascii_lowercase();
            let at = match p.as_str() {
                "from" => Some(0.0),
                "to" => Some(1.0),
                s => s
                    .strip_suffix('%')
                    .and_then(|n| n.trim().parse::<f32>().ok())
                    .map(|n| (n / 100.0).clamp(0.0, 1.0)),
            };
            if let Some(at) = at {
                stops.push(KeyframeStop {
                    at,
                    transform,
                    opacity,
                    fill,
                });
            }
        }
        rest = after.get(close + 1..).unwrap_or_default();
    }
    stops.sort_by(|a, b| a.at.total_cmp(&b.at));
    stops
}

/// Converts a resolved `:hover` declaration into a [`HoverDelta`].
pub(crate) fn build_hover_delta(hover: &Decl, base_size: f32) -> HoverDelta {
    let em = |l: Len| l.px(base_size, super::BASE_WIDTH).unwrap_or(0.0) / ROOT_PX;
    let tint_rgba = |(rgb, alpha): decor::Tint| -> [u8; 4] {
        let [r, g, b] = rgb.unwrap_or([0, 0, 0]);
        let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
        [r, g, b, a]
    };
    let mut transform = hover.extra.as_deref().and_then(|e| e.transform_2d);
    if transform.is_none()
        && let Some(r) = hover.rotate
    {
        transform = Some(Transform2d {
            rotate_deg: r,
            ..Transform2d::IDENTITY
        });
    }
    if let (Some(t), Some(orig)) = (
        &mut transform,
        hover.extra.as_deref().and_then(|e| e.transform_origin),
    ) {
        t.origin = orig;
    }
    let fill = hover
        .extra
        .as_deref()
        .and_then(|e| e.fill_rgba)
        .or_else(|| hover.fill.flatten().map(|[r, g, b]| [r, g, b, 255]));
    let border_colour = hover
        .border_colour
        .iter()
        .flatten()
        .flatten()
        .next()
        .map(|&[r, g, b]| [r, g, b, 255]);
    let shadows: Vec<Shadow> = hover
        .shadows
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|s| Shadow {
            at: s.at.map(em),
            blur: em(s.blur).max(0.0),
            spread: em(s.spread),
            strength: s.tint.1.clamp(0.0, 1.0),
            inset: s.inset,
            colour: Some(tint_rgba(s.tint)),
        })
        .collect();
    let text_colour = hover.ink.map(|[r, g, b]| [r, g, b, 255]);
    let text_shadow = hover
        .text_shadow
        .as_ref()
        .and_then(|opt| opt.as_ref())
        .map(|s| {
            let share = |l: Len| {
                l.px(base_size, super::BASE_WIDTH).unwrap_or(0.0) / base_size.max(1.0) * 100.0
            };
            let rgba = tint_rgba(s.tint);
            TextShadow {
                at: s.at.map(|l| share(l).round().clamp(-127.0, 127.0) as i8),
                blur: share(s.blur).round().clamp(0.0, 255.0) as u8,
                strength: rgba[3],
                colour: Some(rgba),
            }
        });
    HoverDelta {
        transform,
        opacity: hover.opacity,
        fill,
        border_colour,
        shadows,
        text_colour,
        text_shadow,
    }
}
