//! A page's text made searchable, and where a query turns up in it.

use std::ops::Range;

use crate::fold::{fold, invisible, spell};

/// Characters of context each side of a hit in its snippet.
pub const SNIPPET_RADIUS: usize = 32;

/// One character of a page, as its text layer reads it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub ch: char,
    pub line: u32,
    /// A gap the layer saw before it, though no space was written.
    pub space_before: bool,
}

/// A space put in for a gap: no layer character stands behind it.
const GAP: u32 = u32::MAX;

/// A page's text, gaps as single spaces, with a folded twin to match.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    /// As written, for snippets.
    shown: String,
    /// Folded char for char, so positions agree with `shown`.
    folded: String,
    /// Each char's layer character, or `GAP`.
    layer: Layer,
    /// Hyphens splitting a word across lines `(char, byte)`; matches skip them.
    soft: Vec<(u32, u32)>,
}

impl Page {
    pub fn new(glyphs: impl IntoIterator<Item = Glyph>) -> Self {
        let mut page = Self::default();
        let (mut line, mut gap) = (None, false);
        for (i, g) in glyphs.into_iter().enumerate() {
            let at = u32::try_from(i).unwrap_or(GAP);
            let broke = line.is_some_and(|l| l != g.line);
            line = Some(g.line);
            if invisible(g.ch) {
                continue;
            }
            if g.ch.is_whitespace() {
                gap = true;
                continue;
            }
            if broke && !gap && g.ch.is_lowercase() && page.ends_in_a_split_word() {
                let hyphen = u32::try_from(page.len().saturating_sub(1)).unwrap_or(GAP);
                let byte = u32::try_from(page.folded.len().saturating_sub(1)).unwrap_or(GAP);
                page.soft.push((hyphen, byte));
            } else if (gap || broke || g.space_before) && !page.is_empty() {
                page.push(' ', GAP);
            }
            gap = false;
            match spell(g.ch) {
                Some(letters) => letters.chars().for_each(|c| page.push(c, at)),
                None => page.push(g.ch, at),
            }
        }
        // Kept for every page of a searched book: no room to spare.
        page.shown.shrink_to_fit();
        page.folded.shrink_to_fit();
        page.layer.runs.shrink_to_fit();
        page.layer.gaps.shrink_to_fit();
        page.soft.shrink_to_fit();
        page
    }

    fn push(&mut self, c: char, at: u32) {
        self.shown.push(c);
        self.folded.push(fold(c));
        self.layer.push(at);
    }

    /// Characters, gaps included.
    pub fn len(&self) -> usize {
        self.layer.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.layer.len == 0
    }

    /// A letter then a hyphen: a word the line end may have split.
    fn ends_in_a_split_word(&self) -> bool {
        let mut back = self.folded.chars().rev();
        back.next() == Some('-') && back.next().is_some_and(char::is_alphabetic)
    }

    /// Where `needle` turns up, in order and never overlapping.
    pub fn find(&self, needle: &Needle) -> Vec<Range<usize>> {
        let chars = Chars::new(&self.folded);
        let n = needle.chars;
        let positions: Vec<usize> = if needle.case.is_some() {
            self.folded
                .char_indices()
                .filter_map(|(byte, _)| {
                    self.folded
                        .get(byte..)
                        .filter(|tail| tail.starts_with(&needle.folded))
                        .map(|_| byte)
                })
                .collect()
        } else {
            self.folded
                .match_indices(&needle.folded)
                .map(|(byte, _)| byte)
                .collect()
        };
        let mut hits: Vec<Range<usize>> = positions
            .into_iter()
            .map(|byte| chars.at(byte))
            .map(|c| c..c + n)
            .collect();
        if !self.soft.is_empty() {
            self.across_lines(needle, &mut hits);
        }
        if let Some(case) = &needle.case {
            hits.retain(|range| {
                let actual = self
                    .shown
                    .chars()
                    .enumerate()
                    .skip(range.start)
                    .take(range.end - range.start)
                    .filter(|(at, _)| range.end - range.start == needle.chars
                        || !self.soft.iter().any(|(soft, _)| *soft as usize == *at))
                    .map(|(_, c)| c);
                actual.zip(case.chars()).all(|(a, b)| {
                    a.is_uppercase() == b.is_uppercase() && a.is_lowercase() == b.is_lowercase()
                })
            });
        }
        hits.sort_unstable_by_key(|h| (h.start, h.end));
        let mut end = 0;
        hits.retain(|h| {
            let keep = h.start >= end;
            end = if keep { h.end } else { end };
            keep
        });
        hits
    }

    /// Matches that run through a split word's hyphen as if it weren't there.
    fn across_lines(&self, needle: &Needle, out: &mut Vec<Range<usize>>) {
        let n = needle.chars;
        if n < 2 {
            return;
        }
        for (k, &(h_char, h_byte)) in self.soft.iter().enumerate() {
            let (h_char, h_byte) = (h_char as usize, h_byte as usize);
            let floor = k
                .checked_sub(1)
                .and_then(|p| self.soft.get(p))
                .map_or(0, |&(_, b)| (b as usize).saturating_add(1));
            let Some(prefix) = self.folded.get(floor..h_byte) else {
                continue;
            };
            // Each match starts 1..n non-soft chars before its first hyphen.
            for (back, (rel, _)) in prefix.char_indices().rev().take(n - 1).enumerate() {
                let start_byte = floor + rel;
                let start_char = h_char.saturating_sub(back + 1);
                if let Some(last) = self.match_skipping_soft(start_byte, start_char, k, needle)
                    && last - start_char >= n
                {
                    out.push(start_char..last + 1);
                }
            }
        }
    }

    fn match_skipping_soft(
        &self,
        start_byte: usize,
        start_char: usize,
        mut soft_idx: usize,
        needle: &Needle,
    ) -> Option<usize> {
        let rest = self.folded.get(start_byte..)?;
        let mut want = needle.folded.chars();
        let mut expected = want.next()?;
        for (offset, (rel, ch)) in rest.char_indices().enumerate() {
            let byte = start_byte + rel;
            while self
                .soft
                .get(soft_idx)
                .is_some_and(|&(_, sb)| (sb as usize) < byte)
            {
                soft_idx += 1;
            }
            if self
                .soft
                .get(soft_idx)
                .is_some_and(|&(_, sb)| sb as usize == byte)
            {
                soft_idx += 1;
                continue;
            }
            if ch != expected {
                return None;
            }
            match want.next() {
                Some(next) => expected = next,
                None => return Some(start_char + offset),
            }
        }
        None
    }

    /// The layer characters behind `chars`, gaps at either end dropped.
    pub fn layer_range(&self, chars: Range<usize>) -> Option<Range<usize>> {
        if chars.end > self.len() {
            return None;
        }
        let drawn = |i: usize| self.layer.get(i).filter(|&l| l != GAP);
        let first = chars.clone().find_map(drawn)?;
        let last = chars.rev().find_map(drawn)?;
        Some(first as usize..last as usize + 1)
    }

    /// The hit with up to `SNIPPET_RADIUS` characters each side.
    pub fn snippet(&self, chars: Range<usize>) -> String {
        let from = chars.start.saturating_sub(SNIPPET_RADIUS);
        let to = (chars.end + SNIPPET_RADIUS).min(self.len());
        let mut out = String::new();
        if from > 0 {
            out.push('…');
        }
        out.extend(self.shown.chars().skip(from).take(to.saturating_sub(from)));
        if to < self.len() {
            out.push('…');
        }
        out
    }
}

/// Each char's layer character, as runs counting up together.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Layer {
    /// Each run's first char and that char's layer character.
    runs: Vec<(u32, u32)>,
    /// Chars put in for gaps, in order; no layer character stands behind.
    gaps: Vec<u32>,
    len: u32,
}

impl Layer {
    fn push(&mut self, at: u32) {
        let i = self.len;
        self.len = self.len.saturating_add(1);
        if at == GAP {
            self.gaps.push(i);
            return;
        }
        let counted = |&(start, first): &(u32, u32)| first.checked_add(i - start) == Some(at);
        if !self.runs.last().is_some_and(counted) {
            self.runs.push((i, at));
        }
    }

    /// Char `i`'s layer character, `GAP` for a gap; none past the end.
    fn get(&self, i: usize) -> Option<u32> {
        let i = u32::try_from(i).ok().filter(|i| *i < self.len)?;
        if self.gaps.binary_search(&i).is_ok() {
            return Some(GAP);
        }
        let run = self.runs.partition_point(|&(start, _)| start <= i);
        let &(start, first) = self.runs.get(run.checked_sub(1)?)?;
        first.checked_add(i - start)
    }
}

/// Byte offsets to char positions, quick when the text is ASCII.
struct Chars<'a> {
    text: &'a str,
    ascii: bool,
}

impl<'a> Chars<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            ascii: text.is_ascii(),
        }
    }

    fn at(&self, byte: usize) -> usize {
        if self.ascii {
            return byte;
        }
        self.text.get(..byte).map_or(0, |s| s.chars().count())
    }
}

/// What the reader typed, made to match a page's folded text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Needle {
    folded: String,
    chars: usize,
    case: Option<String>,
}

impl Needle {
    /// None for a query of nothing but spaces.
    pub fn new(query: &str) -> Option<Self> {
        Self::with_case(query, false)
    }

    /// Preserve the query's letter case while keeping the usual accent folding.
    pub fn with_case(query: &str, sensitive: bool) -> Option<Self> {
        let mut folded = String::new();
        let mut original = String::new();
        for word in query.split_whitespace() {
            if !folded.is_empty() {
                folded.push(' ');
                original.push(' ');
            }
            for c in word.chars().filter(|c| !invisible(*c)) {
                match spell(c) {
                    Some(letters) => {
                        folded.extend(letters.chars().map(fold));
                        original.push_str(letters);
                    }
                    None => {
                        folded.push(fold(c));
                        original.push(c);
                    }
                }
            }
        }
        let chars = folded.chars().count();
        (chars > 0).then_some(Self {
            folded,
            chars,
            case: sensitive.then_some(original),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// Lines of words; a word's leading `^` marks a gap the layer saw.
    fn page(lines: &[&str]) -> Page {
        let glyphs = lines.iter().enumerate().flat_map(|(n, l)| {
            let mut gap = false;
            l.chars()
                .filter_map(move |ch| {
                    if ch == '^' {
                        gap = true;
                        return None;
                    }
                    let g = Glyph {
                        ch,
                        line: n as u32,
                        space_before: std::mem::take(&mut gap),
                    };
                    Some(g)
                })
                .collect::<Vec<_>>()
        });
        Page::new(glyphs)
    }

    #[test]
    fn case_sensitive_search_preserves_other_folding_and_split_words() {
        let p = page(&["Fox fox Fóx", "Inter-", "national international"]);
        let needle = Needle::with_case("Fox", true).unwrap();
        assert_eq!(p.find(&needle).len(), 2);
        let needle = Needle::with_case("fox", true).unwrap();
        assert_eq!(p.find(&needle).len(), 1);
        let needle = Needle::with_case("International", true).unwrap();
        assert_eq!(p.find(&needle).len(), 1);
        assert_eq!(p.find(&Needle::with_case("Inter-national", true).unwrap()).len(), 1);
        assert_eq!(p.find(&Needle::new("fox").unwrap()).len(), 3);
        let overlap = page(&["aAa"]);
        assert_eq!(
            overlap.find(&Needle::with_case("Aa", true).unwrap()).len(),
            1
        );
    }

    fn found(p: &Page, q: &str) -> Vec<String> {
        let needle = Needle::new(q).unwrap();
        let shown: Vec<char> = p.shown.chars().collect();
        let hits = p.find(&needle);
        hits.into_iter()
            .map(|h| shown[h].iter().collect())
            .collect()
    }

    #[test]
    fn gaps_and_line_ends_read_as_one_space() {
        let p = page(&["The  quick", "brown^fox"]);
        assert_eq!(p.shown, "The quick brown fox");
        assert_eq!(found(&p, "QUICK   brown"), ["quick brown"]);
        assert_eq!(found(&p, "brown fox"), ["brown fox"]);
        assert_eq!(Needle::new("  \t "), None);
    }

    #[test]
    fn matches_fold_and_never_overlap() {
        let p = page(&["Café CAFE cafe aaa"]);
        assert_eq!(found(&p, "cafe"), ["Café", "CAFE", "cafe"]);
        assert_eq!(found(&p, "aa"), ["aa"]);
        // Curly and straight quotes are one.
        assert_eq!(found(&page(&["it\u{2019}s"]), "it's"), ["it\u{2019}s"]);
    }

    #[test]
    fn a_ligature_is_spelled_out_and_maps_to_its_glyph() {
        let p = page(&["e\u{fb03}cient"]);
        assert_eq!(found(&p, "efficient").len(), 1);
        let hit = p.find(&Needle::new("ffi").unwrap())[0].clone();
        assert_eq!(p.layer_range(hit), Some(1..2), "the one ligature glyph");
    }

    #[test]
    fn a_word_split_at_a_line_end_is_found_both_ways() {
        let p = page(&["a docu-", "ment and well-", "Known"]);
        assert_eq!(found(&p, "document"), ["docu-ment"]);
        assert_eq!(found(&p, "docu-ment"), ["docu-ment"]);
        let hit = p.find(&Needle::new("document").unwrap())[0].clone();
        assert_eq!(p.layer_range(hit), Some(2..11), "the hyphen highlights too");
        // A capital after the break is a new word, not a split one.
        assert_eq!(found(&p, "well- known"), ["well- Known"]);
        assert!(found_none(&p, "wellknown"));
    }

    fn found_none(p: &Page, q: &str) -> bool {
        p.find(&Needle::new(q).unwrap()).is_empty()
    }

    #[test]
    fn layer_ranges_skip_gaps_and_snippets_cut_both_ends() {
        let p = page(&["one two"]);
        assert_eq!(p.layer_range(3..7), Some(4..7), "the gap starts nothing");
        let long = format!("{}target{}", "x".repeat(50), "y".repeat(50));
        let p = page(&[long.as_str()]);
        let hit = p.find(&Needle::new("target").unwrap())[0].clone();
        let s = p.snippet(hit);
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert_eq!(s.chars().count(), SNIPPET_RADIUS * 2 + 6 + 2);
        let p = page(&["target"]);
        assert_eq!(p.snippet(0..6), "target");
    }

    #[test]
    fn the_layer_map_reads_as_a_list_in_a_few_runs() {
        let p = page(&["say e\u{fb03}cient  words", "and^more"]);
        // The glyph behind each char; gaps before glyphs 18 and 21 too.
        let glyphs = "say e\u{fb03}cient  wordsandmore";
        let (mut want, mut spaced) = (Vec::new(), false);
        for (i, ch) in glyphs.chars().enumerate() {
            if ch == ' ' {
                spaced = true;
                continue;
            }
            if spaced || i == 18 || i == 21 {
                want.push(GAP);
            }
            spaced = false;
            let n = if ch == '\u{fb03}' { 3 } else { 1 };
            want.extend(std::iter::repeat_n(i as u32, n));
        }
        let got: Vec<u32> = (0..p.len()).map(|i| p.layer.get(i).unwrap()).collect();
        assert_eq!(got, want);
        assert_eq!(p.layer.get(p.len()), None);
        assert!(p.layer.runs.len() <= 6, "runs: {:?}", p.layer.runs);
    }

    #[test]
    fn soft_hyphens_and_zero_widths_vanish() {
        let p = page(&["hy\u{00ad}phen\u{200b}ated"]);
        assert_eq!(found(&p, "hyphenated").len(), 1);
    }

    #[test]
    fn multi_hyphen_and_unicode_line_splits_match() {
        let p = page(&["αβ su-", "per-", "natural", "aa-", "aa-", "aa"]);
        assert_eq!(found(&p, "supernatural"), vec!["su-per-natural"]);
        assert_eq!(found(&p, "αβ super"), vec!["αβ su-per"]);
        assert_eq!(found(&p, "aaa"), vec!["aa-a", "a-aa"]);
    }
}
