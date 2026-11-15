//! When conditional rules apply: `@media`, `@supports` and `@container`
//! (`docs/epub-support.md`).

use crate::calc::closing;
use crate::css::{self, VIEW_HEIGHT, VIEW_WIDTH, split_top};

/// Brackets within brackets one condition may hold.
const MAX_NESTING: usize = 16;

/// A condition's truth; `None` is unknown, which applies as false.
type Truth = Option<bool>;

/// Whether a media query list applies; an empty one does.
pub(crate) fn media(list: &str) -> bool {
    let list = list.trim().to_ascii_lowercase();
    if list.is_empty() {
        return true;
    }
    split_top(&list, ',')
        .iter()
        .any(|q| one_media(q.trim()) == Some(true))
}

/// One query: `screen and (min-width: 30em)`, `not print`, `(color)`.
fn one_media(q: &str) -> Truth {
    if q.starts_with('(') || (starts_word(q, "not") && after_word(q, "not").starts_with('(')) {
        return condition(q, &media_feature, 0);
    }
    let (negated, rest) = match () {
        _ if starts_word(q, "not") => (true, after_word(q, "not")),
        _ if starts_word(q, "only") => (false, after_word(q, "only")),
        _ => (false, q),
    };
    let kind = rest.split_ascii_whitespace().next().unwrap_or_default();
    let typed = matches!(kind, "all" | "screen");
    let tail = rest.get(kind.len()..).unwrap_or_default().trim();
    let met = match tail {
        "" => Some(typed),
        _ if starts_word(tail, "and") => and(
            Some(typed),
            condition(after_word(tail, "and"), &media_feature, 0),
        ),
        _ => None,
    };
    if negated { met.map(|m| !m) } else { met }
}

/// Whether an `@supports` condition holds.
pub(crate) fn supports(test: &str) -> bool {
    condition(&test.trim().to_ascii_lowercase(), &support_feature, 0) == Some(true)
}

/// Whether a container query holds, the column standing for the box.
pub(crate) fn container(prelude: &str) -> bool {
    let prelude = prelude.trim().to_ascii_lowercase();
    // A container's name may come first.
    let named = prelude
        .split_ascii_whitespace()
        .next()
        .filter(|w| !w.starts_with('(') && !w.contains('(') && *w != "not");
    let rest = match named {
        Some(name) => prelude.get(name.len()..).unwrap_or_default().trim(),
        None => prelude.as_str(),
    };
    condition(rest, &media_feature, 0) == Some(true)
}

fn starts_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word)
        .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_whitespace() || c == '('))
}

fn after_word<'t>(text: &'t str, word: &str) -> &'t str {
    text.get(word.len()..).unwrap_or_default().trim_start()
}

fn and(a: Truth, b: Truth) -> Truth {
    match (a, b) {
        (Some(false), _) | (_, Some(false)) => Some(false),
        (Some(true), Some(true)) => Some(true),
        _ => None,
    }
}

fn or(a: Truth, b: Truth) -> Truth {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

/// `not (a)`, `(a) and (b)`, `(a) or (b)`: features or nested groups.
fn condition(text: &str, feature: &dyn Fn(&str) -> Truth, depth: usize) -> Truth {
    let text = text.trim();
    if depth > MAX_NESTING {
        return None;
    }
    if starts_word(text, "not") {
        return condition(after_word(text, "not"), feature, depth + 1).map(|t| !t);
    }
    let (mut rest, mut joiner, mut truth) = (text, None, None);
    loop {
        let (group, after) = group(rest)?;
        let met = match group {
            Group::Paren(inner) if nested(inner) => condition(inner, feature, depth + 1),
            Group::Paren(inner) | Group::Call(inner) => feature(inner.trim()),
        };
        truth = match (joiner, truth) {
            (None, _) => met,
            (Some(true), t) => and(t, met),
            (Some(false), t) => or(t, met),
        };
        let after = after.trim_start();
        if after.is_empty() {
            return truth;
        }
        let word = after
            .split(|c: char| c.is_ascii_whitespace() || c == '(')
            .next()?;
        let is_and = match word {
            "and" => true,
            "or" => false,
            _ => return None,
        };
        // `and` and `or` don't mix without brackets.
        if joiner.is_some_and(|j| j != is_and) {
            return None;
        }
        joiner = Some(is_and);
        rest = after.get(word.len()..)?;
    }
}

/// Whether a bracket's inside is a condition itself, not one feature.
fn nested(inner: &str) -> bool {
    let inner = inner.trim_start();
    inner.starts_with('(') || starts_word(inner, "not")
}

enum Group<'t> {
    /// `( … )`: the inside.
    Paren(&'t str),
    /// `selector( … )` and the like: the whole call.
    Call(&'t str),
}

/// The first bracketed group of `text`, and what follows it.
fn group(text: &str) -> Option<(Group<'_>, &str)> {
    let text = text.trim_start();
    let open = text.find('(')?;
    let name = text.get(..open)?;
    let inner_start = open + 1;
    let close = inner_start + closing(text.get(inner_start..)?)?;
    let after = text.get(close + 1..)?;
    match name {
        "" => Some((Group::Paren(text.get(inner_start..close)?), after)),
        n if n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
            Some((Group::Call(text.get(..=close)?), after))
        }
        _ => None,
    }
}

/// `(display: grid)` holds if the cascade reads it; `selector()` too.
fn support_feature(f: &str) -> Truth {
    if let Some(inner) = f.strip_prefix("selector(") {
        return Some(css::selector_understood(inner.strip_suffix(')')?));
    }
    // Font formats and technologies wait for their fonts (E5).
    if f.starts_with("font-tech(") || f.starts_with("font-format(") {
        return Some(false);
    }
    let (name, value) = f.split_once(':')?;
    Some(css::understood(name, value))
}

/// What a media feature is on our page.
#[derive(Clone, Copy)]
enum Value {
    /// Lengths in px, ratios as width over height, resolutions in dppx.
    Number(f32, Kind),
    Word(&'static str),
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Length,
    Ratio,
    Resolution,
    Plain,
}

/// The page as queries see it: the text box upright, on a light screen.
fn page_value(name: &str) -> Option<Value> {
    use Kind::{Length, Plain, Ratio, Resolution};
    use Value::{Number, Word};
    Some(match name {
        "width" | "device-width" | "inline-size" => Number(VIEW_WIDTH, Length),
        "height" | "device-height" | "block-size" => Number(VIEW_HEIGHT, Length),
        "aspect-ratio" | "device-aspect-ratio" => Number(VIEW_WIDTH / VIEW_HEIGHT, Ratio),
        "resolution" => Number(1.0, Resolution),
        "-webkit-device-pixel-ratio" | "-moz-device-pixel-ratio" | "device-pixel-ratio" => {
            Number(1.0, Plain)
        }
        "color" => Number(8.0, Plain),
        "color-index" | "monochrome" | "grid" => Number(0.0, Plain),
        "orientation" => Word("portrait"),
        // Theme colours adapt the book's light design (`docs/epub-support.md`).
        "prefers-color-scheme" => Word("light"),
        "prefers-reduced-motion"
        | "prefers-contrast"
        | "prefers-reduced-transparency"
        | "prefers-reduced-data" => Word("no-preference"),
        "forced-colors" | "inverted-colors" => Word("none"),
        "hover" | "any-hover" => Word("hover"),
        "pointer" | "any-pointer" => Word("fine"),
        "scan" => Word("progressive"),
        "update" => Word("fast"),
        "overflow-block" => Word("paged"),
        "overflow-inline" => Word("none"),
        "color-gamut" => Word("srgb"),
        "dynamic-range" | "video-dynamic-range" => Word("standard"),
        "display-mode" => Word("browser"),
        // No script runs in a chapter.
        "scripting" => Word("none"),
        _ => return None,
    })
}

/// A feature with its `min-` or `max-` taken off: -1 min, 1 max, 0 neither.
fn bound(name: &str) -> (i8, String) {
    for (prefix, kept, bound) in [
        ("min-", "", -1),
        ("max-", "", 1),
        ("-webkit-min-", "-webkit-", -1),
        ("-webkit-max-", "-webkit-", 1),
    ] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return (bound, format!("{kept}{rest}"));
        }
    }
    (0, name.to_owned())
}

fn media_feature(f: &str) -> Truth {
    if let Some(truth) = range(f) {
        return truth;
    }
    let (name, value) = match f.split_once(':') {
        Some((n, v)) => (n.trim(), Some(v.trim())),
        None => (f.trim(), None),
    };
    let (bound, base) = bound(name);
    let actual = page_value(&base)?;
    let Some(value) = value else {
        // Alone, a feature holds unless it is zero or none.
        return (bound == 0).then_some(match actual {
            Value::Number(n, _) => n != 0.0,
            Value::Word(w) => !matches!(w, "none" | "no-preference"),
        });
    };
    match actual {
        Value::Word(w) => (bound == 0).then_some(w == value),
        Value::Number(n, kind) => {
            let want = number(value, kind)?;
            Some(match bound {
                -1 => n >= want,
                1 => n <= want,
                _ => (n - want).abs() < 0.001,
            })
        }
    }
}

/// `width >= 600px` or `400px <= width < 700px`; `None` if not a range.
fn range(f: &str) -> Option<Truth> {
    let ops = ["<=", ">=", "<", ">", "="];
    let mut parts: Vec<&str> = Vec::new();
    let mut signs: Vec<&str> = Vec::new();
    let mut rest = f;
    while let Some((at, op)) = ops
        .iter()
        .filter_map(|op| rest.find(op).map(|at| (at, *op)))
        .min_by_key(|(at, op)| (*at, std::cmp::Reverse(op.len())))
    {
        parts.push(rest.get(..at)?.trim());
        signs.push(op);
        rest = rest.get(at + op.len()..)?;
    }
    if signs.is_empty() {
        return None;
    }
    parts.push(rest.trim());
    let named = parts.iter().position(|p| page_value(p).is_some());
    let Some(Value::Number(actual, kind)) = named.and_then(|i| page_value(parts.get(i)?)) else {
        return Some(None);
    };
    let compare = |a: f32, op: &str, b: f32| match op {
        "<=" => a <= b,
        ">=" => a >= b,
        "<" => a < b,
        ">" => a > b,
        _ => (a - b).abs() < 0.001,
    };
    let at = |i: usize| -> Option<f32> {
        let p = parts.get(i)?;
        if page_value(p).is_some() {
            return Some(actual);
        }
        number(p, kind)
    };
    let mut met = true;
    for (i, op) in signs.iter().enumerate() {
        match (at(i), at(i + 1)) {
            (Some(a), Some(b)) => met &= compare(a, op, b),
            _ => return Some(None),
        }
    }
    Some(Some(met))
}

/// A query's number in the feature's terms: px, a ratio, dppx.
fn number(v: &str, kind: Kind) -> Option<f32> {
    let v = v.trim();
    let n = match kind {
        Kind::Length => css::len(v)?.px(css::ROOT_PX, 0.0)?,
        Kind::Ratio => match v.split_once('/') {
            Some((w, h)) => w.trim().parse::<f32>().ok()? / h.trim().parse::<f32>().ok()?,
            None => v.parse().ok()?,
        },
        Kind::Resolution => {
            let split = v.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(v.len());
            let (n, unit) = v.split_at(split);
            let n: f32 = n.trim().parse().ok()?;
            match unit {
                "dppx" | "x" => n,
                "dpi" => n / 96.0,
                "dpcm" => n * 2.54 / 96.0,
                _ => return None,
            }
        }
        Kind::Plain => v.parse().ok()?,
    };
    n.is_finite().then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_types_and_lists() {
        assert!(media(""));
        assert!(media("screen") && media("all") && media("only screen"));
        assert!(!media("print") && !media("amzn-kf8") && !media("speech"));
        assert!(media("not print") && !media("not screen"));
        assert!(media("print, screen"));
        assert!(!media("amzn-mobi, amzn-kf8"));
    }

    #[test]
    fn media_features_read_the_page() {
        assert!(media("screen and (min-width: 300px)"));
        assert!(!media("screen and (max-width: 480px)"));
        assert!(media("(max-width: 60em)") && !media("(min-width: 60em)"));
        assert!(media("(orientation: portrait)") && !media("(orientation: landscape)"));
        assert!(media("(prefers-color-scheme: light)"));
        assert!(!media("(prefers-color-scheme: dark)"));
        assert!(!media("(prefers-reduced-motion)") && media("(color)"));
        assert!(!media("(monochrome)") && media("(scripting: none)"));
        assert!(!media("(-webkit-min-device-pixel-ratio: 2)"));
        assert!(media("(-webkit-min-device-pixel-ratio: 1)"));
        assert!(!media("(min-resolution: 2dppx)") && media("(max-resolution: 192dpi)"));
        assert!(media("(max-aspect-ratio: 1/1)") && !media("(min-aspect-ratio: 16/9)"));
        // Unknown features never apply, even negated.
        assert!(!media("(frobs: 3)") && !media("not (frobs: 3)"));
        assert!(!media("(min-orientation: portrait)"));
    }

    #[test]
    fn media_conditions_combine() {
        assert!(media("(min-width: 1px) and (max-width: 9999px)"));
        assert!(!media("(min-width: 1px) and (prefers-color-scheme: dark)"));
        assert!(media("(max-width: 1px) or (color)"));
        assert!(media("not (prefers-color-scheme: dark)"));
        assert!(media("not all and (monochrome)"));
        assert!(media("screen and ((color) or (monochrome))"));
        // `and` and `or` don't mix unbracketed.
        assert!(!media("(color) and (color) or (color)"));
    }

    #[test]
    fn ranges_compare_like_bounds() {
        assert!(media("(width >= 600px)") && !media("(width > 700px)"));
        assert!(media("(400px <= width <= 700px)") && !media("(700px <= width)"));
        assert!(media("(width < 50em)") && media("(height > 500px)"));
        assert!(!media("(frobs > 3)"));
    }

    #[test]
    fn supports_asks_the_cascade() {
        assert!(supports("(display: grid)") && supports("(display: flex)"));
        assert!(!supports("(display: run-in)") && supports("(writing-mode: vertical-rl)"));
        assert!(supports("not (display: run-in)"));
        assert!(supports("(color: red) and (opacity: 0.5)"));
        assert!(supports("(frob: 1) or (--x: y)"));
        assert!(supports("selector(p > em)") && !supports("selector(p[)"));
        // Pseudo-classes the cascade can't match count as unsupported.
        assert!(supports("selector(li:nth-child(2n+1))"));
        assert!(!supports("selector(p:has(> img))") && !supports("selector(:not(a:hover))"));
        assert!(!supports("font-format(woff2)"));
        assert!(!supports("(display: grid) and"));
        let deep = format!("{}(color: red){}", "(".repeat(40), ")".repeat(40));
        assert!(!supports(&deep));
    }

    #[test]
    fn containers_read_like_the_column() {
        assert!(container("(min-width: 20em)") && container("card (width > 300px)"));
        assert!(!container("sidebar (max-width: 10em)"));
        assert!(!container("style(--theme: dark)"));
    }
}
