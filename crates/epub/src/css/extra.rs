//! Extended CSS declarations and computed values (multicol, transforms, RGBA).
use mareader_reflow::{Easing, Edge, Length, MultiCol, Repeat, Transform2d, TransitionSpec};

use super::anim::{self, AnimDecl};
use super::{BASE_WIDTH, BorderStyle, Decl, Len, ROOT_PX, border_style, border_width, len, words};
use crate::decor;

/// Extended declaration fields, boxed lazily for zero cost on plain text.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ExtraDecl {
    pub(crate) fill_rgba: Option<[u8; 4]>,
    pub(crate) border_box: Option<bool>,
    pub(crate) max_width: Option<Len>,
    pub(crate) min_width: Option<Len>,
    pub(crate) max_height: Option<Len>,
    pub(crate) min_height: Option<Len>,
    pub(crate) clip: Option<bool>,
    pub(crate) repeat_axis: Option<Repeat>,
    pub(crate) multicol_count: Option<u8>,
    pub(crate) multicol_width: Option<Len>,
    pub(crate) multicol_gap: Option<Len>,
    pub(crate) multicol_rule_width: Option<Len>,
    pub(crate) multicol_rule_style: Option<BorderStyle>,
    pub(crate) multicol_rule_colour: Option<[u8; 4]>,
    pub(crate) column_span: Option<bool>,
    pub(crate) transform_2d: Option<Transform2d>,
    pub(crate) transform_origin: Option<[f32; 2]>,
    pub(crate) transition: Option<TransitionSpec>,
    pub(crate) transition_duration: Option<f32>,
    pub(crate) transition_delay: Option<f32>,
    pub(crate) transition_easing: Option<Easing>,
    pub(crate) animation: Option<AnimDecl>,
}

/// Extended computed style fields, boxed lazily.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ExtraComputed {
    pub(crate) fill_rgba: Option<[u8; 4]>,
    pub(crate) border_box: bool,
    pub(crate) max_width: Option<Len>,
    pub(crate) min_width: Option<Len>,
    pub(crate) max_height: Option<Len>,
    pub(crate) min_height: Option<Len>,
    pub(crate) clip: bool,
    pub(crate) repeat_axis: Option<Repeat>,
    pub(crate) multicol: Option<MultiCol>,
    pub(crate) column_span: bool,
    pub(crate) transform_2d: Option<Transform2d>,
    pub(crate) transition: Option<TransitionSpec>,
    pub(crate) animation: Option<AnimDecl>,
}

/// Strips browser/EPUB vendor prefixes so prefixed rules cascade natively.
pub(crate) fn strip_vendor(name: &str) -> &str {
    for prefix in ["-webkit-", "-moz-", "-o-", "-ms-", "-epub-"] {
        if let Some(stripped) = name.strip_prefix(prefix) {
            return stripped;
        }
    }
    name
}

/// Parses a CSS colour token with 8-bit alpha (`rgba()`, hex, named).
pub(crate) fn parse_rgba(v: &str) -> Option<[u8; 4]> {
    let (rgb, alpha) = decor::tint(v)?;
    let [r, g, b] = rgb?;
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some([r, g, b, a])
}

/// Parses CSS `background-repeat` into directional `(x, y)` repeat booleans.
pub(crate) fn parse_repeat(v: &str) -> Option<Repeat> {
    let lower = v.to_ascii_lowercase();
    let ws = words(&lower);
    match ws.as_slice() {
        ["repeat"] => Some(Repeat::BOTH),
        ["repeat-x"] => Some(Repeat { x: true, y: false }),
        ["repeat-y"] => Some(Repeat { x: false, y: true }),
        ["no-repeat"] => Some(Repeat::NONE),
        ["space"] | ["round"] => Some(Repeat::BOTH),
        [a, b, ..] => {
            let axis = |w: &str| match w {
                "repeat" | "space" | "round" => Some(true),
                "no-repeat" => Some(false),
                _ => None,
            };
            Some(Repeat {
                x: axis(a)?,
                y: axis(b)?,
            })
        }
        _ => None,
    }
}

/// Records `fill_rgba` / `repeat_axis` from `background` shorthand words.
pub(crate) fn record_background_shorthand(decl: &mut Decl, last_plain: &str, at_words: &[&str]) {
    if let Some(rgba) = words(last_plain).iter().find_map(|w| parse_rgba(w)) {
        decl.extra_mut().fill_rgba = Some(rgba);
    }
    if let Some(rep) = at_words.iter().find_map(|w| parse_repeat(w)) {
        decl.extra_mut().repeat_axis = Some(rep);
    }
}

/// Handles extended CSS properties on `decl`; returns true when consumed.
pub(crate) fn apply_extra(decl: &mut Decl, name: &str, value: &str, lower: &str) -> bool {
    apply_box_extra(decl, name, lower)
        || apply_multicol_extra(decl, name, lower)
        || apply_anim_extra(decl, name, value, lower)
}

fn apply_box_extra(decl: &mut Decl, name: &str, lower: &str) -> bool {
    match name {
        "box-sizing" => {
            let bb = match lower {
                "border-box" => Some(true),
                "content-box" => Some(false),
                _ => None,
            };
            if bb.is_some() {
                decl.extra_mut().border_box = bb;
            }
            true
        }
        "max-width" => {
            decl.extra_mut().max_width = if lower == "none" {
                Some(Len::Auto)
            } else {
                len(lower).or(decl.extra_mut().max_width)
            };
            true
        }
        "min-width" => {
            if let Some(l) = len(lower) {
                decl.extra_mut().min_width = Some(l);
            }
            true
        }
        "max-height" => {
            decl.extra_mut().max_height = if lower == "none" {
                Some(Len::Auto)
            } else {
                len(lower).or(decl.extra_mut().max_height)
            };
            true
        }
        "min-height" => {
            if let Some(l) = len(lower) {
                decl.extra_mut().min_height = Some(l);
            }
            true
        }
        "overflow" | "overflow-x" | "overflow-y" => {
            let clip = match lower {
                "hidden" | "clip" => Some(true),
                "visible" | "auto" | "scroll" => Some(false),
                _ => None,
            };
            if clip.is_some() {
                decl.extra_mut().clip = clip;
            }
            true
        }
        _ => false,
    }
}

fn apply_multicol_extra(decl: &mut Decl, name: &str, lower: &str) -> bool {
    match name {
        "column-count" => {
            if lower == "auto" {
                decl.extra_mut().multicol_count = Some(0);
            } else if let Ok(n) = lower.parse::<u8>() {
                decl.extra_mut().multicol_count = Some(n);
            }
            true
        }
        "column-width" => {
            if let Some(l) = len(lower) {
                decl.extra_mut().multicol_width = Some(l);
            }
            true
        }
        "columns" => {
            for w in words(lower) {
                if let Ok(n) = w.parse::<u8>() {
                    decl.extra_mut().multicol_count = Some(n);
                } else if let Some(l) = len(w) {
                    decl.extra_mut().multicol_width = Some(l);
                }
            }
            true
        }
        "column-gap" => {
            let gap = if lower == "normal" {
                Some(Len::Em(1.0))
            } else {
                len(lower)
            };
            if gap.is_some() {
                decl.extra_mut().multicol_gap = gap;
            }
            true
        }
        "column-rule" => {
            let e = decl.extra_mut();
            for w in words(lower) {
                if let Some(st) = border_style(w) {
                    e.multicol_rule_style = Some(st);
                } else if let Some(bw) = border_width(w) {
                    e.multicol_rule_width = Some(bw);
                } else if let Some(c) = parse_rgba(w) {
                    e.multicol_rule_colour = Some(c);
                }
            }
            true
        }
        "column-rule-width" => {
            if let Some(bw) = border_width(lower) {
                decl.extra_mut().multicol_rule_width = Some(bw);
            }
            true
        }
        "column-rule-style" => {
            if let Some(st) = border_style(lower) {
                decl.extra_mut().multicol_rule_style = Some(st);
            }
            true
        }
        "column-rule-color" => {
            if let Some(c) = parse_rgba(lower) {
                decl.extra_mut().multicol_rule_colour = Some(c);
            }
            true
        }
        "column-span" => {
            decl.extra_mut().column_span = Some(lower == "all");
            true
        }
        _ => false,
    }
}

fn apply_anim_extra(decl: &mut Decl, name: &str, value: &str, lower: &str) -> bool {
    match name {
        "transform-origin" => {
            if let Some(o) = anim::parse_transform_origin(lower) {
                decl.extra_mut().transform_origin = Some(o);
            }
            true
        }
        "transition" => {
            if let Some(t) = anim::parse_transition(lower) {
                decl.extra_mut().transition = Some(t);
            }
            true
        }
        "transition-duration" => {
            if let Some(t) = anim::parse_time(lower) {
                decl.extra_mut().transition_duration = Some(t);
            }
            true
        }
        "transition-delay" => {
            if let Some(t) = anim::parse_time(lower) {
                decl.extra_mut().transition_delay = Some(t);
            }
            true
        }
        "transition-timing-function" => {
            if let Some(e) = anim::parse_easing(lower) {
                decl.extra_mut().transition_easing = Some(e);
            }
            true
        }
        _ => apply_keyframes_prop(decl, name, value, lower),
    }
}

fn apply_keyframes_prop(decl: &mut Decl, name: &str, value: &str, lower: &str) -> bool {
    if !name.starts_with("animation") {
        return false;
    }
    let anim_decl = decl
        .extra_mut()
        .animation
        .get_or_insert_with(AnimDecl::default);
    match name {
        "animation" => anim_decl.layer(&anim::parse_animation(value)),
        "animation-name" => anim_decl.name = Some(std::sync::Arc::from(value.trim())),
        "animation-duration" => anim_decl.duration = anim::parse_time(lower).or(anim_decl.duration),
        "animation-delay" => anim_decl.delay = anim::parse_time(lower).or(anim_decl.delay),
        "animation-timing-function" => {
            anim_decl.easing = anim::parse_easing(lower).or(anim_decl.easing);
        }
        "animation-iteration-count" => {
            let it = if lower == "infinite" {
                Some(f32::INFINITY)
            } else {
                lower
                    .parse::<f32>()
                    .ok()
                    .filter(|n| n.is_finite() && *n >= 0.0)
            };
            anim_decl.iterations = it.or(anim_decl.iterations);
        }
        "animation-direction" => {
            anim_decl.alternate = Some(matches!(lower, "alternate" | "alternate-reverse"));
        }
        "animation-fill-mode" => {
            anim_decl.forwards = Some(matches!(lower, "forwards" | "both"));
        }
        _ => return false,
    }
    true
}

/// Cascades `src` extra declarations over `dst`.
pub(crate) fn layer_extra(dst: &mut Option<Box<ExtraDecl>>, src: &Option<Box<ExtraDecl>>) {
    let Some(s) = src.as_deref() else {
        return;
    };
    let d = dst.get_or_insert_with(|| Box::new(ExtraDecl::default()));
    macro_rules! take {
        ($($f:ident),*) => {
            $(if s.$f.is_some() { d.$f = s.$f; })*
        };
    }
    take!(
        fill_rgba,
        border_box,
        max_width,
        min_width,
        max_height,
        min_height,
        clip,
        repeat_axis,
        multicol_count,
        multicol_width,
        multicol_gap,
        multicol_rule_width,
        multicol_rule_style,
        multicol_rule_colour,
        column_span,
        transform_2d,
        transform_origin,
        transition,
        transition_duration,
        transition_delay,
        transition_easing
    );
    if let Some(a) = &s.animation {
        d.animation.get_or_insert_with(AnimDecl::default).layer(a);
    }
}

/// Resolves `ExtraDecl` into `ExtraComputed` for one element.
pub(crate) fn compute_extra(d: &Decl, size: f32) -> Option<Box<ExtraComputed>> {
    let e = d.extra.as_deref()?;
    let multicol = compute_multicol(e, size);
    let mut transform_2d = e.transform_2d;
    if let (Some(t), Some(orig)) = (&mut transform_2d, e.transform_origin) {
        t.origin = orig;
    }
    let transition = if e.transition.is_some()
        || e.transition_duration.is_some()
        || e.transition_delay.is_some()
        || e.transition_easing.is_some()
    {
        let mut t = e.transition.unwrap_or_default();
        if let Some(d) = e.transition_duration {
            t.duration_ms = (d * 1000.0).round().clamp(0.0, 600_000.0) as u32;
        }
        if let Some(d) = e.transition_delay {
            t.delay_ms = (d * 1000.0).round().clamp(0.0, 600_000.0) as u32;
        }
        if let Some(ea) = e.transition_easing {
            t.easing = ea;
        }
        (t.duration_ms > 0).then_some(t)
    } else {
        None
    };
    Some(Box::new(ExtraComputed {
        fill_rgba: e.fill_rgba,
        border_box: e.border_box.unwrap_or(false),
        max_width: e
            .max_width
            .filter(|l| *l != Len::Auto)
            .map(|l| l.computed(size)),
        min_width: e
            .min_width
            .filter(|l| *l != Len::Auto)
            .map(|l| l.computed(size)),
        max_height: e
            .max_height
            .filter(|l| *l != Len::Auto)
            .map(|l| l.computed(size)),
        min_height: e
            .min_height
            .filter(|l| *l != Len::Auto)
            .map(|l| l.computed(size)),
        clip: e.clip.unwrap_or(false),
        repeat_axis: e.repeat_axis,
        multicol,
        column_span: e.column_span.unwrap_or(false),
        transform_2d,
        transition,
        animation: e.animation.clone(),
    }))
}

fn compute_multicol(e: &ExtraDecl, size: f32) -> Option<MultiCol> {
    let to_length = |l: Len| match l {
        Len::Percent(p) | Len::Vw(p) => Length::Percent(p),
        Len::Mix { px, em, pct } => Length::Mix {
            em: (px + em * size) / ROOT_PX,
            percent: pct,
        },
        other => Length::Em(other.px(size, BASE_WIDTH).unwrap_or(0.0) / ROOT_PX),
    };
    let count = e.multicol_count.unwrap_or(0);
    let width = e.multicol_width.filter(|l| *l != Len::Auto).map(to_length);
    (count >= 2 || width.is_some()).then(|| {
        let gap = e.multicol_gap.map_or(Length::Em(1.0), to_length);
        let style = e.multicol_rule_style.map_or(Edge::None, border_edge);
        let rule_width = if style != Edge::None {
            e.multicol_rule_width
                .and_then(|l| l.px(size, BASE_WIDTH))
                .unwrap_or(3.0)
                / ROOT_PX
        } else {
            0.0
        };
        MultiCol {
            count,
            width,
            gap,
            rule_width,
            rule_style: style,
            rule_colour: e.multicol_rule_colour,
        }
    })
}

pub(crate) fn border_edge(s: BorderStyle) -> Edge {
    match s {
        BorderStyle::None => Edge::None,
        BorderStyle::Solid => Edge::Solid,
        BorderStyle::Double => Edge::Double,
        BorderStyle::Dashed => Edge::Dashed,
        BorderStyle::Dotted => Edge::Dotted,
        BorderStyle::Groove => Edge::Groove,
        BorderStyle::Ridge => Edge::Ridge,
        BorderStyle::Inset => Edge::Inset,
        BorderStyle::Outset => Edge::Outset,
    }
}
