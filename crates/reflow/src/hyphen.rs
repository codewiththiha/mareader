//! Soft hyphens put where the text's language allows a break.
use hypher::Lang;

use crate::block::{Link, Span, Text};

/// U+00AD: parley may break after it; the hyphen is drawn there.
pub(crate) const SHY: char = '\u{AD}';
const SHY_LEN: usize = SHY.len_utf8();
/// Shorter words never break.
const MIN_LETTERS: usize = 5;

/// A BCP 47 tag's patterns; an unknown language falls back to English.
pub(crate) fn lang(tag: &str) -> Lang {
    let mut code = tag.bytes().map(|b| b.to_ascii_lowercase());
    match (code.next(), code.next()) {
        (Some(a), Some(b)) => Lang::from_iso([a, b]).unwrap_or(Lang::English),
        _ => Lang::English,
    }
}

/// Text with soft hyphens in, and where each one went.
pub(crate) struct Hyphenated {
    pub(crate) text: Text,
    /// Each mark's byte offset in the new text, ascending.
    pub(crate) marks: Vec<usize>,
}

impl Hyphenated {
    /// A new-text offset as an offset into the original text.
    pub(crate) fn original(&self, offset: usize) -> usize {
        offset - SHY_LEN * self.marks.partition_point(|&m| m < offset)
    }
}

/// Marks every break hypher finds; inline code is left whole.
pub(crate) fn hyphenate(text: &Text, lang: Lang) -> Hyphenated {
    let src = text.text.as_str();
    let code: Vec<_> = text
        .spans
        .iter()
        .filter(|s| s.emphasis.code)
        .map(|s| s.range.clone())
        .collect();
    let mut out = String::with_capacity(src.len() + src.len() / 8);
    // Insertion points, as offsets into `src`.
    let mut at = Vec::new();
    let mut done = 0;
    for (start, end) in words(src) {
        let in_code = code.iter().any(|r| r.start < end && start < r.end);
        let Some(word) = src.get(start..end) else {
            continue;
        };
        if in_code || word.chars().count() < MIN_LETTERS {
            continue;
        }
        out.push_str(src.get(done..start).unwrap_or_default());
        let mut pos = start;
        for (i, syllable) in hypher::hyphenate(word, lang).enumerate() {
            if i > 0 {
                at.push(pos);
                out.push(SHY);
            }
            out.push_str(syllable);
            pos += syllable.len();
        }
        done = end;
    }
    out.push_str(src.get(done..).unwrap_or_default());
    let marks = at
        .iter()
        .enumerate()
        .map(|(i, p)| p + i * SHY_LEN)
        .collect();
    Hyphenated {
        text: shift_text(text, out, &at),
        marks,
    }
}

fn shift_text(text: &Text, out: String, at: &[usize]) -> Text {
    let shift = |x: usize, before: fn(usize, usize) -> bool| {
        x + SHY_LEN * at.partition_point(|&p| before(p, x))
    };
    let spans = text
        .spans
        .iter()
        .map(|s| Span {
            range: shift(s.range.start, |p, x| p <= x)..shift(s.range.end, |p, x| p < x),
            emphasis: s.emphasis,
        })
        .collect();
    let links = text
        .links
        .iter()
        .map(|l| Link {
            range: shift(l.range.start, |p, x| p <= x)..shift(l.range.end, |p, x| p < x),
            target: l.target,
        })
        .collect();
    Text {
        role: text.role,
        text: out,
        spans,
        links,
        initial: text.initial,
        first_line: text.first_line.clone(),
        ruby: (text.ruby.iter())
            .map(|a| crate::block::Annotation {
                range: shift(a.range.start, |p, x| p <= x)..shift(a.range.end, |p, x| p < x),
                text: a.text.clone(),
            })
            .collect(),
        faces: text.faces.clone(),
        combine: (text.combine.iter())
            .map(|r| shift(r.start, |p, x| p <= x)..shift(r.end, |p, x| p < x))
            .collect(),
        inlines: (text.inlines.iter())
            .map(|i| crate::block::Inline {
                at: shift(i.at, |p, x| p <= x),
                ..i.clone()
            })
            .collect(),
        spacers: (text.spacers.iter())
            .map(|s| crate::anim::InlineSpace {
                at: shift(s.at, |p, x| p <= x),
                ..*s
            })
            .collect(),
    }
}

/// Byte ranges of letter runs.
fn words(s: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let mut chars = s.char_indices().peekable();
    std::iter::from_fn(move || {
        let (start, _) = chars.by_ref().find(|(_, c)| c.is_alphabetic())?;
        let mut end = s.len();
        while let Some(&(i, c)) = chars.peek() {
            if !c.is_alphabetic() {
                end = i;
                break;
            }
            chars.next();
        }
        Some((start, end))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{Emphasis, Role};

    fn text(parts: &[(&str, Emphasis)]) -> Text {
        let mut t = Text::new(Role::Paragraph);
        for (s, e) in parts {
            t.push(s, *e);
        }
        t
    }

    #[test]
    fn long_words_get_soft_hyphens_short_ones_and_code_do_not() {
        let code = Emphasis {
            code: true,
            ..Emphasis::default()
        };
        let t = text(&[
            ("an extensive ", Emphasis::default()),
            ("hyphenation", code),
            (" test", Emphasis::default()),
        ]);
        let h = hyphenate(&t, Lang::English);
        assert_eq!(
            h.text.text, "an ex\u{AD}ten\u{AD}sive hyphenation test",
            "{:?}",
            h.text.text
        );
        // The code span still covers exactly its word.
        let span = h.text.spans.first().map(|s| s.range.clone());
        let covered = span.and_then(|r| h.text.text.get(r));
        assert_eq!(covered, Some("hyphenation"));
    }

    #[test]
    fn offsets_map_back_to_the_original_text() {
        let t = text(&[("extensive words", Emphasis::default())]);
        let h = hyphenate(&t, Lang::English);
        let words = h.text.text.find("words").unwrap_or(0);
        assert_eq!(h.original(words), "extensive ".len());
        assert_eq!(h.original(0), 0);
    }

    #[test]
    fn links_cover_the_same_words_once_hyphenated() {
        let mut t = Text::new(Role::Paragraph);
        t.push("See ", Emphasis::default());
        t.push_link("incomprehensible things", Emphasis::default(), 4);
        t.push(" now.", Emphasis::default());
        let h = hyphenate(&t, Lang::English);
        let linked: Vec<(&str, u32)> = h
            .text
            .links
            .iter()
            .map(|l| {
                (
                    h.text.text.get(l.range.clone()).unwrap_or_default(),
                    l.target,
                )
            })
            .collect();
        assert_eq!(linked.len(), 1, "{linked:?}");
        let Some(&(words, target)) = linked.first() else {
            return;
        };
        assert_eq!(target, 4);
        assert!(words.contains(SHY), "{words:?}");
        assert_eq!(words.replace(SHY, ""), "incomprehensible things");
    }

    #[test]
    fn language_tags_pick_patterns_and_fall_back_to_english() {
        assert_eq!(lang("de-AT"), Lang::German);
        assert_eq!(lang("FR"), Lang::French);
        assert_eq!(lang("zz"), Lang::English);
        assert_eq!(lang(""), Lang::English);
    }
}
