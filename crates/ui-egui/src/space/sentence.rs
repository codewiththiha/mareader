//! The actual form of a word marked within its book's sentence.

use egui::text::{LayoutJob, TextFormat};

use crate::theme::{self, Tokens};

pub(super) fn highlight(text: &str, form: &str, t: &Tokens) -> LayoutJob {
    let normal = TextFormat {
        font_id: theme::serif(t.text + 1.0),
        color: t.ink,
        italics: true,
        ..TextFormat::default()
    };
    let mut marked = normal.clone();
    marked.color = t.accent;
    marked.background = t.accent.gamma_multiply(0.13);
    let mut job = LayoutJob::default();
    let mut from = 0;
    for (start, end) in matches(text, form) {
        if let Some(before) = text.get(from..start) {
            job.append(before, 0.0, normal.clone());
        }
        if let Some(word) = text.get(start..end) {
            job.append(word, 0.0, marked.clone());
        }
        from = end;
    }
    if let Some(rest) = text.get(from..) {
        job.append(rest, 0.0, normal);
    }
    job
}

fn matches(text: &str, form: &str) -> Vec<(usize, usize)> {
    let needle = form.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut folded = String::new();
    let mut boundaries = Vec::new();
    for (at, ch) in text.char_indices() {
        boundaries.push((folded.len(), at));
        folded.extend(ch.to_lowercase());
    }
    boundaries.push((folded.len(), text.len()));
    let original = |at| {
        boundaries
            .binary_search_by_key(&at, |&(folded, _)| folded)
            .ok()
            .and_then(|i| boundaries.get(i))
            .map(|&(_, original)| original)
    };
    folded
        .match_indices(&needle)
        .filter_map(|(start, found)| Some((original(start)?, original(start + found.len())?)))
        .filter(|&(start, end)| {
            let left = text.get(..start).and_then(|s| s.chars().next_back());
            let right = text.get(end..).and_then(|s| s.chars().next());
            !left.is_some_and(char::is_alphanumeric) && !right.is_some_and(char::is_alphanumeric)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn finds_actual_form_without_matching_inside_other_words() {
        let text = "Runs, RUNS; overruns and runs!";
        let found: Vec<_> = matches(text, "runs")
            .into_iter()
            .filter_map(|(a, b)| text.get(a..b))
            .collect();
        assert_eq!(found, ["Runs", "RUNS", "runs"]);
    }

    #[test]
    fn unicode_boundaries_and_lowercase_expansion_are_safe() {
        let text = "İSTANBUL, İstanbul and préİstanbul";
        let found: Vec<_> = matches(text, "İstanbul")
            .into_iter()
            .filter_map(|(a, b)| text.get(a..b))
            .collect();
        assert_eq!(found, ["İSTANBUL", "İstanbul"]);
        assert!(matches(text, "").is_empty());
    }
}
