//! CSS maths and custom properties: `calc()`, `min()`, `max()`, `clamp()`
//! and `var()` (`docs/epub-support.md`).

use std::sync::Arc;

use crate::css::{BASE_WIDTH, Len, ROOT_PX, VIEW_HEIGHT};

/// An element's custom properties by name, their `var()`s resolved.
pub(crate) type Vars = Arc<[(String, String)]>;

/// Custom properties one element keeps.
const MAX_VARS: usize = 256;
/// A value's longest text once its `var()`s are filled in.
pub(crate) const MAX_VALUE: usize = 2048;
/// Fallbacks within fallbacks, and custom values naming each other.
const MAX_DEPTH: usize = 8;
/// Brackets within one maths expression.
const MAX_NESTING: usize = 16;
/// Components stay within what a page can use.
const LIMIT: f32 = 100_000.0;

/// A number plus lengths in px, ems and percent, as `calc()` adds them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Sum {
    pub(crate) n: f32,
    pub(crate) px: f32,
    pub(crate) em: f32,
    pub(crate) pct: f32,
}

impl Sum {
    /// A plain number, no length in it.
    pub(crate) fn is_number(self) -> bool {
        self.px == 0.0 && self.em == 0.0 && self.pct == 0.0
    }

    fn plus(self, o: Self, sign: f32) -> Self {
        Self {
            n: self.n + sign * o.n,
            px: self.px + sign * o.px,
            em: self.em + sign * o.em,
            pct: self.pct + sign * o.pct,
        }
    }

    fn times(self, k: f32) -> Self {
        Self {
            n: self.n * k,
            px: self.px * k,
            em: self.em * k,
            pct: self.pct * k,
        }
    }

    /// Near enough to compare: ems of the root, percent of a column.
    fn rough(self) -> f32 {
        self.n + self.px + self.em * ROOT_PX + self.pct * BASE_WIDTH / 100.0
    }

    fn sane(self) -> Option<Self> {
        let parts = [self.n, self.px, self.em, self.pct];
        parts.iter().all(|p| p.is_finite()).then(|| Self {
            n: self.n.clamp(-LIMIT, LIMIT),
            px: self.px.clamp(-LIMIT, LIMIT),
            em: self.em.clamp(-LIMIT, LIMIT),
            pct: self.pct.clamp(-LIMIT, LIMIT),
        })
    }

    /// As a length; a lone zero is one, other plain numbers aren't.
    pub(crate) fn length(self) -> Option<Len> {
        if self.is_number() {
            return (self.n == 0.0).then_some(Len::Px(0.0));
        }
        if self.n != 0.0 {
            return None;
        }
        Some(match (self.px != 0.0, self.em != 0.0, self.pct != 0.0) {
            (_, false, false) => Len::Px(self.px),
            (false, true, false) => Len::Em(self.em),
            (false, false, true) => Len::Percent(self.pct),
            _ => Len::Mix {
                px: self.px,
                em: self.em,
                pct: self.pct,
            },
        })
    }
}

/// Whether `v` is a maths function this module reads.
pub(crate) fn is_maths(v: &str) -> bool {
    let v = v.trim_start().to_ascii_lowercase();
    [
        "calc(",
        "-webkit-calc(",
        "-moz-calc(",
        "min(",
        "max(",
        "clamp(",
    ]
    .iter()
    .any(|f| v.starts_with(f))
}

/// The value of a maths function filling all of `v`.
pub(crate) fn maths(v: &str) -> Option<Sum> {
    let v = v.trim();
    if v.len() > MAX_VALUE || !is_maths(v) {
        return None;
    }
    let lower = v.to_ascii_lowercase();
    let mut p = Parser {
        s: lower.as_bytes(),
        i: 0,
        depth: 0,
    };
    let sum = p.value()?;
    p.space();
    (p.i == p.s.len()).then_some(sum)
}

/// Reads a maths expression byte by byte; ASCII is all it accepts.
struct Parser<'s> {
    s: &'s [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn space(&mut self) -> bool {
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
        self.i > start
    }

    fn eat(&mut self, c: u8) -> bool {
        let found = self.peek() == Some(c);
        self.i += usize::from(found);
        found
    }

    /// Terms joined by `+` and `-`, which need spaces around them.
    fn sum(&mut self) -> Option<Sum> {
        let mut total = self.product()?;
        loop {
            let before = self.i;
            let spaced = self.space();
            let sign = match self.peek() {
                Some(b'+') if spaced => 1.0,
                Some(b'-') if spaced => -1.0,
                _ => {
                    self.i = before;
                    return Some(total);
                }
            };
            self.i += 1;
            if !self.space() {
                return None;
            }
            total = total.plus(self.product()?, sign).sane()?;
        }
    }

    /// Values joined by `*` and `/`; one side of each is a plain number.
    fn product(&mut self) -> Option<Sum> {
        let mut total = self.value()?;
        loop {
            let before = self.i;
            self.space();
            let op = self.peek();
            if !matches!(op, Some(b'*' | b'/')) {
                self.i = before;
                return Some(total);
            }
            self.i += 1;
            self.space();
            let next = self.value()?;
            total = match op {
                Some(b'*') if next.is_number() => total.times(next.n),
                Some(b'*') if total.is_number() => next.times(total.n),
                Some(b'/') if next.is_number() && next.n != 0.0 => total.times(1.0 / next.n),
                _ => return None,
            }
            .sane()?;
        }
    }

    /// A number with its unit, a bracket, or a nested function.
    fn value(&mut self) -> Option<Sum> {
        self.space();
        if self.eat(b'(') {
            return self.inside(|p| p.sum());
        }
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'-')
        {
            self.i += 1;
        }
        let name = std::str::from_utf8(self.s.get(start..self.i)?).ok()?;
        if !name.is_empty() && name != "-" && self.eat(b'(') {
            return self.function(name);
        }
        self.i = start;
        self.number()
    }

    /// Runs `read` within a bracket just opened, through its close.
    fn inside(&mut self, read: impl FnOnce(&mut Self) -> Option<Sum>) -> Option<Sum> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return None;
        }
        let sum = read(self)?;
        self.space();
        self.depth -= 1;
        self.eat(b')').then_some(sum)
    }

    fn function(&mut self, name: &str) -> Option<Sum> {
        self.inside(|p| {
            let mut args = vec![p.sum()?];
            loop {
                p.space();
                if !p.eat(b',') {
                    break;
                }
                args.push(p.sum()?);
                if args.len() > 32 {
                    return None;
                }
            }
            let pick = |want: fn(f32, f32) -> bool, args: &[Sum]| {
                let first = *args.first()?;
                Some(
                    args.iter()
                        .fold(first, |best, a| match want(a.rough(), best.rough()) {
                            true => *a,
                            false => best,
                        }),
                )
            };
            let least = |a: f32, b: f32| a < b;
            let most = |a: f32, b: f32| a > b;
            match (name, args.as_slice()) {
                ("calc" | "-webkit-calc" | "-moz-calc", [one]) => Some(*one),
                ("min", _) => pick(least, &args),
                ("max", _) => pick(most, &args),
                ("clamp", [lo, mid, hi]) => {
                    let capped = pick(least, &[*mid, *hi])?;
                    pick(most, &[*lo, capped])
                }
                _ => None,
            }
        })
    }

    /// `12`, `-1.5e2`, `40%`, `2em`: a number then its unit.
    fn number(&mut self) -> Option<Sum> {
        let start = self.i;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.i += 1;
        }
        let digits = |p: &mut Self| {
            let from = p.i;
            while p.peek().is_some_and(|c| c.is_ascii_digit()) {
                p.i += 1;
            }
            p.i > from
        };
        let whole = digits(self);
        let fraction = self.eat(b'.') && digits(self);
        if !whole && !fraction {
            return None;
        }
        // An exponent is `e` then digits; `em` and `ex` are units.
        let at = self.i;
        if self.eat(b'e') {
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if !digits(self) {
                self.i = at;
            }
        }
        let text = std::str::from_utf8(self.s.get(start..self.i)?).ok()?;
        let n: f32 = text.parse().ok().filter(|n: &f32| n.is_finite())?;
        let unit_start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == b'%')
        {
            self.i += 1;
        }
        let unit = std::str::from_utf8(self.s.get(unit_start..self.i)?).ok()?;
        unit_sum(n, unit)?.sane()
    }
}

/// `n` of `unit`; viewport widths read as the column's, as widths do.
fn unit_sum(n: f32, unit: &str) -> Option<Sum> {
    let px = |k: f32| Sum {
        px: n * k,
        ..Sum::default()
    };
    Some(match unit {
        "" => Sum {
            n,
            ..Sum::default()
        },
        "px" => px(1.0),
        "pt" => px(4.0 / 3.0),
        "pc" => px(16.0),
        "in" => px(96.0),
        "cm" => px(96.0 / 2.54),
        "mm" => px(96.0 / 25.4),
        "q" => px(96.0 / 101.6),
        "rem" => px(ROOT_PX),
        "vh" | "vmax" => px(VIEW_HEIGHT / 100.0),
        "em" => Sum {
            em: n,
            ..Sum::default()
        },
        "ex" | "ch" => Sum {
            em: n / 2.0,
            ..Sum::default()
        },
        "%" | "vw" | "vmin" => Sum {
            pct: n,
            ..Sum::default()
        },
        _ => return None,
    })
}

/// Whether `v` names a custom property's value.
pub(crate) fn has_var(v: &str) -> bool {
    find_var(v).is_some()
}

/// Where the first `var(` starts, not part of a longer name.
fn find_var(v: &str) -> Option<usize> {
    let lower = v.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower.get(from..)?.find("var(") {
        let at = from + at;
        let before = lower.get(..at).and_then(|b| b.chars().next_back());
        if !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Some(at);
        }
        from = at + 4;
    }
    None
}

/// Where the bracket open before `text` closes, quotes skipped.
pub(crate) fn closing(text: &str) -> Option<usize> {
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => return Some(i),
            (None, ')') => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Where the first comma outside brackets and quotes is.
fn first_comma(text: &str) -> Option<usize> {
    let (mut depth, mut quote) = (0usize, None);
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth = depth.saturating_sub(1),
            (None, ',') if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// `value` with each `var()` filled in; `None` if one has no value.
pub(crate) fn substitute(value: &str, vars: &[(String, String)]) -> Option<String> {
    fill(value, vars, 0)
}

fn fill(value: &str, vars: &[(String, String)], depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    let (mut out, mut rest) = (String::new(), value);
    while let Some(at) = find_var(rest) {
        out.push_str(rest.get(..at)?);
        let body = rest.get(at + 4..)?;
        let close = closing(body)?;
        let inner = body.get(..close)?;
        let (name, fallback) = match first_comma(inner) {
            Some(c) => (inner.get(..c)?, Some(inner.get(c + 1..)?)),
            None => (inner, None),
        };
        let name = name.trim();
        let found = vars.iter().rev().find(|(n, _)| n == name);
        match (found, fallback) {
            (Some((_, v)), _) => out.push_str(v),
            (None, Some(f)) => out.push_str(fill(f.trim(), vars, depth + 1)?.as_str()),
            (None, None) => return None,
        }
        if out.len() > MAX_VALUE {
            return None;
        }
        rest = body.get(close + 1..)?;
    }
    out.push_str(rest);
    (out.len() <= MAX_VALUE).then_some(out)
}

/// Custom properties: `inherited`, then `own` in order; cycles drop out.
pub(crate) fn resolve(inherited: &Vars, own: &[&[(String, String)]]) -> Vars {
    if own.iter().all(|o| o.is_empty()) {
        return Arc::clone(inherited);
    }
    let mut map: Vec<(String, String)> = inherited.to_vec();
    for (name, value) in own.iter().flat_map(|o| o.iter()) {
        match value.trim().to_ascii_lowercase().as_str() {
            // Inheriting is what a custom property does anyway.
            "inherit" | "unset" => continue,
            "initial" => map.retain(|(n, _)| n != name),
            _ => {
                map.retain(|(n, _)| n != name);
                if map.len() < MAX_VARS {
                    map.push((name.clone(), value.clone()));
                }
            }
        }
    }
    for _ in 0..MAX_DEPTH {
        let waiting: Vec<usize> = (map.iter().enumerate())
            .filter(|(_, (_, v))| has_var(v))
            .map(|(i, _)| i)
            .collect();
        if waiting.is_empty() {
            break;
        }
        let filled: Vec<(usize, Option<String>)> = (waiting.iter())
            .map(|&i| (i, map.get(i).and_then(|(_, v)| substitute(v, &map))))
            .collect();
        for (i, value) in filled {
            if let Some(slot) = map.get_mut(i) {
                // One naming nothing set is invalid: it drops below.
                slot.1 = value.unwrap_or_else(|| "var(".to_owned());
            }
        }
    }
    map.retain(|(_, v)| !has_var(v));
    map.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len(v: &str) -> Option<Len> {
        maths(v).and_then(Sum::length)
    }

    #[test]
    fn calc_adds_scales_and_mixes_units() {
        assert_eq!(len("calc(10px + 2px * 3)"), Some(Len::Px(16.0)));
        assert_eq!(len("calc((1em + 1em) / 4)"), Some(Len::Em(0.5)));
        assert_eq!(len("CALC(1in - 48px)"), Some(Len::Px(48.0)));
        assert_eq!(
            len("calc(100% - 2em)"),
            Some(Len::Mix {
                px: 0.0,
                em: -2.0,
                pct: 100.0
            })
        );
        assert_eq!(len("-webkit-calc(50% + 0px)"), Some(Len::Percent(50.0)));
        assert_eq!(len("calc(1.5e1px)"), Some(Len::Px(15.0)));
        assert_eq!(len("calc(2ex)"), Some(Len::Em(1.0)));
    }

    #[test]
    fn calc_refuses_what_css_refuses() {
        // `+` and `-` need spaces; lengths don't multiply; no zero divisor.
        for bad in [
            "calc(1em+2px)",
            "calc(1em -2px)",
            "calc(2px * 3px)",
            "calc(1px / 0)",
            "calc(1px + 2)",
            "calc(1px",
            "calc(1px) x",
            "calc(3 furlongs)",
            "clamp(1px, 2px)",
        ] {
            assert_eq!(len(bad), None, "{bad}");
        }
        let deep = format!("{}1px{}", "calc(".repeat(40), ")".repeat(40));
        assert_eq!(len(&deep), None);
    }

    #[test]
    fn min_max_and_clamp_pick_an_argument() {
        assert_eq!(len("min(10px, 2em)"), Some(Len::Px(10.0)));
        assert_eq!(len("max(10px, 2em)"), Some(Len::Em(2.0)));
        assert_eq!(len("clamp(1rem, 10px, 2rem)"), Some(Len::Px(16.0)));
        assert_eq!(len("clamp(1rem, 100px, 2rem)"), Some(Len::Px(32.0)));
        assert_eq!(len("min(100%, 30em)"), Some(Len::Em(30.0)));
        assert_eq!(len("calc(min(4px, 8px) * 2)"), Some(Len::Px(8.0)));
        assert_eq!(maths("calc(1.2 * 1.25)").map(|s| s.n), Some(1.5));
    }

    #[test]
    fn var_fills_in_with_fallbacks() {
        let vars = vec![
            ("--gap".to_owned(), "2em".to_owned()),
            ("--ink".to_owned(), "#336".to_owned()),
        ];
        let s = |v: &str| substitute(v, &vars);
        assert_eq!(s("var(--gap) 0").as_deref(), Some("2em 0"));
        assert_eq!(s("1px solid VAR(--ink)").as_deref(), Some("1px solid #336"));
        assert_eq!(s("var(--none, red)").as_deref(), Some("red"));
        assert_eq!(s("var(--none, var(--gap))").as_deref(), Some("2em"));
        assert_eq!(s("var(--none,)").as_deref(), Some(""));
        assert_eq!(s("calc(var(--gap) * 2)").as_deref(), Some("calc(2em * 2)"));
        assert_eq!(s("var(--none)"), None);
        assert_eq!(s("var(--gap"), None);
        assert!(has_var("x var(--a)") && !has_var("somevar(--a)"));
    }

    #[test]
    fn custom_properties_inherit_settle_and_drop_cycles() {
        let parent: Vars = vec![("--a".to_owned(), "1px".to_owned())].into();
        let own = [
            ("--b".to_owned(), "var(--c) var(--a)".to_owned()),
            ("--c".to_owned(), "2px".to_owned()),
            ("--x".to_owned(), "var(--y)".to_owned()),
            ("--y".to_owned(), "var(--x)".to_owned()),
            ("--gone".to_owned(), "var(--nothing)".to_owned()),
        ];
        let vars = resolve(&parent, &[&own]);
        let get = |n: &str| vars.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
        assert_eq!(get("--a"), Some("1px"));
        assert_eq!(get("--b"), Some("2px 1px"));
        assert_eq!((get("--x"), get("--y"), get("--gone")), (None, None, None));
        // Nothing of its own: the parent's, shared.
        assert!(Arc::ptr_eq(&resolve(&parent, &[&[]]), &parent));
        let reset = [("--a".to_owned(), "initial".to_owned())];
        assert!(resolve(&parent, &[&reset]).is_empty());
    }

    #[test]
    fn doubling_values_stop_at_the_cap() {
        let mut own = vec![("--v0".to_owned(), "xxxxxxxxxxxxxxxx".to_owned())];
        for i in 1..20 {
            own.push((format!("--v{i}"), format!("var(--v{0}) var(--v{0})", i - 1)));
        }
        let vars = resolve(&Arc::from([]), &[&own]);
        assert!(vars.iter().all(|(_, v)| v.len() <= MAX_VALUE));
        assert!(vars.len() < own.len());
    }
}
