//! Search result snippet formatting.

use std::ops::Range;
use egui::text::{LayoutJob, TextWrapping};
use egui::{FontId, TextFormat};
use crate::theme::{self, Tokens};

/// The snippet over two lines at most, its match washed in the accent.
pub(super) fn snippet(t: &Tokens, text: &str, needle: &str, width: f32, on: bool) -> LayoutJob {
    let body = FontId::proportional(t.text_small + 0.5);
    let plain = TextFormat::simple(body, if on { t.ink } else { t.muted });
    let marked = TextFormat {
        font_id: theme::medium(t.text_small + 0.5),
        color: t.ink,
        background: t.accent.gamma_multiply(0.22),
        ..TextFormat::default()
    };
    let mut job = LayoutJob::default();
    match marked_range(text, needle) {
        Some(m) => {
            job.append(text.get(..m.start).unwrap_or_default(), 0.0, plain.clone());
            job.append(text.get(m.clone()).unwrap_or_default(), 0.0, marked);
            job.append(text.get(m.end..).unwrap_or_default(), 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job.wrap = TextWrapping {
        max_width: width.max(40.0),
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job
}

/// Where `needle` sits in `text`, ignoring case: the match nearest the middle.
fn marked_range(text: &str, needle: &str) -> Option<Range<usize>> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let want: Vec<char> = needle.chars().map(fold).collect();
    if want.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let middle = text.len() / 2;
    let mut best: Option<Range<usize>> = None;
    let last = chars.len().checked_sub(want.len())?;
    for start in 0..=last {
        let Some(window) = chars.get(start..start + want.len()) else {
            continue;
        };
        let same = window.iter().zip(&want).all(|(&(_, c), &w)| fold(c) == w);
        if !same {
            continue;
        }
        let from = window.first().map_or(0, |&(b, _)| b);
        let to = chars
            .get(start + want.len())
            .map_or(text.len(), |&(b, _)| b);
        let nearer = |r: &Range<usize>| r.start.abs_diff(middle) > from.abs_diff(middle);
        if best.as_ref().is_none_or(nearer) {
            best = Some(from..to);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_match_is_found_in_any_case_nearest_the_middle() {
        let text = "…the Quick fox and the quick dog and the quick cat…";
        let m = marked_range(text, "QUICK").unwrap_or_default();
        assert_eq!(text.get(m.clone()), Some("quick"));
        assert!(m.start > 20 && m.start < 30, "the middle one: {m:?}");
        assert_eq!(marked_range("ÉCOLE", "école"), Some(0.."ÉCOLE".len()));
        assert_eq!(marked_range("abc", ""), None);
        assert_eq!(marked_range("abc", "z"), None);
    }
}
