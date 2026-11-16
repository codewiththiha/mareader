//! CSS initial-value table and `@keyframes` / `:hover` cascade helpers.
use std::sync::Arc;

use super::{Ctx, Decl, Rule, Stylesheet, anim, cascade};

/// Cascades matching `:hover` rules in `sheets` for `path`.
pub(crate) fn declared_hover(
    sheets: &[Arc<Stylesheet>],
    path: &[Ctx<'_>],
    vars: &crate::calc::Vars,
) -> Option<Decl> {
    if !sheets.iter().any(|s| s.has_hover) {
        return None;
    }
    let mut found: Vec<(u16, u32, usize, usize, &Rule)> = Vec::new();
    for (n, sheet) in sheets.iter().enumerate() {
        sheet.matching_hover(path, &mut |r| {
            found.push((r.rank, r.specificity, n, r.order, r));
        });
    }
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(|&(rank, spec, sheet, order, _)| (rank, spec, sheet, order));
    let mut layers: Vec<&Decl> = Vec::with_capacity(2 * found.len());
    layers.extend(found.iter().map(|f| &f.4.normal));
    let mut strong: Vec<&Rule> = found.iter().map(|f| f.4).collect();
    strong.sort_by_key(|r| std::cmp::Reverse(r.rank));
    layers.extend(strong.iter().map(|r| &r.important));
    Some(cascade(&layers, vars))
}

impl Stylesheet {
    pub(super) fn matching_hover<'s>(&'s self, path: &[Ctx<'_>], found: &mut impl FnMut(&'s Rule)) {
        if !self.has_hover {
            return;
        }
        let Some(el) = path.last().map(|c| c.el) else {
            return;
        };
        let by_id = el.id.as_ref().and_then(|id| self.by_id.get(id));
        let by_class = (el.classes.iter())
            .filter_map(|c| self.by_class.get(c))
            .flat_map(|b| {
                b.any
                    .iter()
                    .chain(b.tags.get(&el.tag).into_iter().flatten())
            });
        let candidates = (by_id.into_iter().flatten())
            .chain(by_class)
            .chain(self.by_tag.get(&el.tag).into_iter().flatten())
            .chain(&self.any);
        for &at in candidates {
            if let Some(rule) = self.rules.get(at)
                && rule.element.is_none()
                && rule.selector.has_hover()
                && rule.selector.matches_hover(path)
            {
                found(rule);
            }
        }
    }
}

/// Parses and stores an `@keyframes` rule if `prelude` names one.
pub(crate) fn try_keyframes(
    sheet: &mut Stylesheet,
    prelude: &str,
    lower: &str,
    inner: &str,
) -> bool {
    let Some(name) = keyframes_name(prelude, lower) else {
        return false;
    };
    let stops = anim::parse_keyframes(inner);
    if !stops.is_empty() && sheet.keyframes.len() < 128 {
        sheet.keyframes.insert(name.to_owned(), stops);
    }
    true
}

pub(crate) fn apply_flex_flow(decl: &mut Decl, v: &str) {
    for w in v.split_ascii_whitespace() {
        match w {
            "row" | "row-reverse" => decl.flex_column = Some(false),
            "column" | "column-reverse" => decl.flex_column = Some(true),
            "wrap" | "wrap-reverse" => decl.wrap = Some(true),
            "nowrap" => decl.wrap = Some(false),
            _ => {}
        }
    }
}

pub(crate) fn apply_column_gap(decl: &mut Decl, g: Option<super::Len>) {
    decl.gap = g.or(decl.gap);
    if let Some(l) = g {
        decl.extra_mut().multicol_gap = Some(l);
    }
}

pub(crate) fn apply_transform_decl(decl: &mut Decl, value: &str, lower: &str) {
    decl.rotate = Some(super::rotation(lower).unwrap_or(0.0));
    if let Some(t) = anim::parse_transform(value) {
        decl.extra_mut().transform_2d = Some(t);
    }
}

/// Extracts the keyframe name from an `@keyframes` prelude.
pub(crate) fn keyframes_name<'a>(prelude: &'a str, lower: &str) -> Option<&'a str> {
    for prefix in [
        "@keyframes",
        "@-webkit-keyframes",
        "@-moz-keyframes",
        "@-o-keyframes",
    ] {
        if lower.starts_with(prefix) {
            let name = prelude
                .get(prefix.len()..)?
                .trim()
                .trim_matches(['"', '\'']);
            return (!name.is_empty()).then_some(name);
        }
    }
    None
}

/// A property's initial value, whether it is literal, whether it inherits.
pub(super) fn initial_value(name: &str) -> Option<(&'static str, bool, bool)> {
    Some(match name {
        "color" => ("black", true, true),
        "font-family" => ("serif", false, true),
        "font" => ("16px serif", false, true),
        "quotes" => ("none", false, true),
        "font-style" | "font-weight" | "font-variant" | "font-variant-caps" => {
            ("normal", true, true)
        }
        "font-variant-numeric"
        | "font-variant-ligatures"
        | "font-variant-position"
        | "font-feature-settings" => ("normal", true, true),
        "font-kerning" | "text-align-last" => ("auto", true, true),
        "direction" => ("ltr", true, true),
        "writing-mode" | "-epub-writing-mode" | "-webkit-writing-mode" => {
            ("horizontal-tb", true, true)
        }
        "text-orientation" | "-epub-text-orientation" | "-webkit-text-orientation" => {
            ("mixed", true, true)
        }
        "text-combine-upright" | "-epub-text-combine" | "-webkit-text-combine" => {
            ("none", true, false)
        }
        "orphans" | "widows" => ("2", true, true),
        "break-inside" | "page-break-inside" => ("auto", true, false),
        "line-height" | "letter-spacing" | "word-spacing" => ("normal", true, true),
        "font-size" => ("medium", true, true),
        "text-indent" => ("0", true, true),
        "text-align" => ("start", true, true),
        "text-transform" | "text-shadow" | "list-style-image" => ("none", true, true),
        "visibility" => ("visible", true, true),
        "list-style" | "list-style-type" => ("disc", true, true),
        "display" => ("inline", true, false),
        "width" | "height" | "flex-basis" => ("auto", true, false),
        "background-color" => ("transparent", true, false),
        "opacity" => ("1", true, false),
        "vertical-align" => ("baseline", true, false),
        "content" => ("normal", true, false),
        "flex-direction" => ("row", true, false),
        "flex-wrap" => ("nowrap", true, false),
        "justify-content" => ("normal", true, false),
        "flex-grow" | "gap" | "border-radius" => ("0", true, false),
        "page-break-before" | "page-break-after" | "break-before" | "break-after" => {
            ("auto", true, false)
        }
        "position" => ("static", true, false),
        "top" | "right" | "bottom" | "left" | "inset" | "z-index" => ("auto", true, false),
        "float"
        | "text-decoration"
        | "text-decoration-line"
        | "box-shadow"
        | "outline"
        | "transform"
        | "filter"
        | "background"
        | "background-image"
        | "border-image"
        | "counter-reset"
        | "counter-increment"
        | "counter-set"
        | "border"
        | "border-top"
        | "border-right"
        | "border-bottom"
        | "border-left" => ("none", true, false),
        n if n.starts_with("margin") || n.starts_with("padding") => ("0", true, false),
        n if n.starts_with("border-") && n.ends_with("-width") => ("medium", true, false),
        n if n.starts_with("border-") && n.ends_with("-style") => ("none", true, false),
        n if n.starts_with("border-") && n.ends_with("-color") => ("currentcolor", true, false),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use crate::css::anim::{parse_keyframes, parse_transform, parse_transition};
    use mareader_reflow::{Easing, TransitionSpec};

    #[test]
    fn parses_famouspaintings_transitions_and_transforms() {
        let tr = parse_transition("all 0.25s ease-in-out");
        assert_eq!(
            tr,
            Some(TransitionSpec {
                duration_ms: 250,
                delay_ms: 0,
                easing: Easing::EASE_IN_OUT,
            })
        );
        let tf = parse_transform("scale(1.25) translate(16px, -8px) rotate(15deg)");
        assert!(tf.is_some());
        if let Some(t) = tf {
            assert!((t.scale[0] - 1.25).abs() < 1e-4);
            assert!((t.scale[1] - 1.25).abs() < 1e-4);
            assert!((t.translate_em[0] - 1.0).abs() < 1e-4);
            assert!((t.translate_em[1] + 0.5).abs() < 1e-4);
            assert!((t.rotate_deg - 15.0).abs() < 1e-4);
        }
    }

    #[test]
    fn parses_keyframes_from_to_and_percent() {
        let stops = parse_keyframes(
            "from { opacity: 0; transform: scale(0.8); } \
             50% { opacity: 0.6; } \
             to { opacity: 1; transform: scale(1); }",
        );
        assert_eq!(stops.len(), 3);
        assert_eq!(stops.first().map(|s| s.at), Some(0.0));
        assert_eq!(stops.get(1).and_then(|s| s.opacity), Some(0.6));
        assert_eq!(stops.get(2).map(|s| s.at), Some(1.0));
    }
}
