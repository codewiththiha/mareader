//! Footnotes: which links open one in place, and what each note says.

use std::collections::HashMap;
use std::ops::Range;

use mareader_reflow::{Block, Content};

use crate::xhtml::{Href, Refs};

/// A note shows this many blocks at most, and characters.
const MAX_BLOCKS: usize = 12;
const MAX_CHARS: usize = 2_000;
/// A mark-like link to something longer is a cross-reference.
const GUESS_BLOCKS: usize = 3;

/// What each note link shows, by the target its text carries.
pub(crate) fn find(blocks: &[Block], refs: &Refs) -> HashMap<u32, String> {
    let mut out = HashMap::new();
    for block in blocks {
        let Content::Text(t) = &block.content else {
            continue;
        };
        for link in &t.links {
            if out.contains_key(&link.target) {
                continue;
            }
            let index = usize::try_from(link.target).ok();
            let Some(Href::Inside(key)) = index.and_then(|i| refs.hrefs.get(i)) else {
                continue;
            };
            let marked = refs.noterefs.contains(&link.target);
            let semantic = refs.extents.get(key).is_some_and(|e| e.note);
            let looks = key.contains('#') && t.text.get(link.range.clone()).is_some_and(mark);
            if !(marked || semantic || looks) {
                continue;
            }
            if let Some(text) = text(blocks, refs, key, marked || semantic) {
                out.insert(link.target, text);
            }
        }
    }
    out
}

/// Whether a link's words look like a note's mark: "3", "[12]", "*", "iv".
fn mark(words: &str) -> bool {
    let brackets = |c: char| matches!(c, '[' | ']' | '(' | ')' | '{' | '}');
    let core = words.trim().trim_matches(brackets);
    let marky = |c: char| c.is_ascii_digit() || "*†‡\u{a7}¶#ivxlc¹²³⁰⁴⁵⁶⁷⁸⁹".contains(c);
    !core.is_empty() && core.chars().count() <= 4 && core.chars().all(marky)
}

/// The note at `key`; unless `sure`, headings and long runs aren't notes.
fn text(blocks: &[Block], refs: &Refs, key: &str, sure: bool) -> Option<String> {
    let (range, from): (Range<usize>, usize) = match refs.extents.get(key) {
        Some(e) => (e.blocks.clone(), 0),
        None => {
            let at = refs.places.get(key)?;
            (at.block..at.block + 1, at.offset)
        }
    };
    if !sure && range.len() > GUESS_BLOCKS {
        return None;
    }
    let mut parts = Vec::new();
    let held = blocks.iter().enumerate().skip(range.start);
    for (i, block) in held.take(range.len().min(MAX_BLOCKS)) {
        let Content::Text(t) = &block.content else {
            continue;
        };
        if !sure && t.role.is_heading() {
            return None;
        }
        let skip = if i == range.start { from } else { 0 };
        let words = clean(t.text.get(skip..).unwrap_or_default());
        if !words.is_empty() {
            parts.push(words);
        }
    }
    let note = parts.join("\n\n");
    (!note.is_empty()).then(|| cut(note))
}

/// Trimmed, the arrows back to the mark dropped.
fn clean(text: &str) -> String {
    let back =
        |c: char| matches!(c, '↩' | '↑' | '^' | '\u{fe0e}' | '\u{fe0f}') || c.is_whitespace();
    text.trim_matches(back).to_owned()
}

fn cut(text: String) -> String {
    match text.char_indices().nth(MAX_CHARS) {
        Some((at, _)) => format!("{}…", text.get(..at).unwrap_or_default().trim_end()),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_look_like_note_numbers_not_words() {
        for m in ["3", "[12]", "(4)", "*", "†", "iv", "²"] {
            assert!(mark(m), "{m}");
        }
        for w in ["", "see", "Chapter 3", "12345", "a"] {
            assert!(!mark(w), "{w}");
        }
    }

    #[test]
    fn a_long_note_is_cut_on_a_character() {
        let long = "မ".repeat(MAX_CHARS + 5);
        let cut = cut(long);
        assert_eq!(cut.chars().count(), MAX_CHARS + 1);
        assert!(cut.ends_with('…'));
        assert_eq!(clean("  The note. ↩\u{fe0e} "), "The note.");
    }
}
