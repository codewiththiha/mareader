//! Searching the notes: every word typed must appear, in any case.

use std::ops::Range;

/// The words of `query`; each must be found.
pub fn terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_owned).collect()
}

/// Whether every term is somewhere in `texts`.
pub fn keeps(terms: &[String], texts: &[&str]) -> bool {
    let found = |term: &String| texts.iter().any(|t| first(t, term).is_some());
    terms.iter().all(found)
}

/// Where the terms appear in `text`, merged, in order, as byte ranges.
pub fn marks(text: &str, terms: &[String]) -> Vec<Range<usize>> {
    let mut all: Vec<Range<usize>> = terms.iter().flat_map(|t| spans(text, t)).collect();
    all.sort_by_key(|r| r.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(all.len());
    for r in all {
        match out.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => out.push(r),
        }
    }
    out
}

fn first(text: &str, term: &str) -> Option<Range<usize>> {
    let mut starts = text.char_indices().map(|(i, _)| i);
    starts.find_map(|i| at(text, i, term).map(|end| i..end))
}

/// Every place `term` appears, not overlapping.
fn spans(text: &str, term: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    for (i, _) in text.char_indices() {
        if i < from {
            continue;
        }
        if let Some(end) = at(text, i, term) {
            out.push(i..end);
            from = end;
        }
    }
    out
}

/// Where a match of `term` starting at `start` ends, if one does.
fn at(text: &str, start: usize, term: &str) -> Option<usize> {
    let mut hay = text.get(start..)?.char_indices();
    let mut end = None;
    for n in term.chars() {
        let (i, h) = hay.next()?;
        if !h.to_lowercase().eq(n.to_lowercase()) {
            return None;
        }
        end = Some(start + i + h.len_utf8());
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_must_appear_in_any_case_and_any_text() {
        let t = terms("  Fox   lazy ");
        assert_eq!(t, ["Fox", "lazy"]);
        assert!(keeps(&t, &["The quick brown fox", "a LAZY dog"]));
        assert!(!keeps(&t, &["The quick brown fox"]));
        assert!(keeps(&[], &["anything"]), "no words keep all");
    }

    #[test]
    fn marks_merge_and_hold_to_character_edges() {
        let t = terms("ab bc");
        assert_eq!(marks("xabcx abx", &t), [1..4, 6..8]);
        // Byte ranges stay on char edges in Burmese and accented text.
        let word = "မြန်မာ";
        let r = marks(&format!("— {word} —"), &terms(word));
        assert_eq!(r.len(), 1);
        assert_eq!(marks("ÉCOLE école", &terms("école")), [0..6, 7..13]);
    }
}
