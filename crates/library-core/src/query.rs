//! The titlebar search: which books a query keeps, which shelves, and which books
//! the bar suggests while the reader is still typing.
//!
//! Client-side and over three fields — title, author, address — because that is
//! all a library is. Nothing here is indexed: a few thousand string comparisons
//! per keystroke is well inside a frame.

use crate::book::{Book, Row, book_rows};

/// Seven rows fill the suggestion panel without scrolling.
pub const SUGGEST_LIMIT: usize = 7;

/// Characters that start a word, so a match on one is a match on a boundary the reader can see.
fn is_boundary(hay: &[char], at: usize) -> bool {
    at == 0 || matches!(hay[at - 1], ' ' | '-' | '_' | '.' | '/' | ':' | '(' | ')')
}

/// One fold per char, no allocation: the hot path walks fields a keystroke at a time.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// One match: its score, and the character spans it lit up, merged so a consecutive run is one span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub score: i32,
    pub spans: Vec<(usize, usize)>,
}

fn merge_spans(indices: &[usize]) -> Vec<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for &at in indices {
        match spans.last_mut() {
            Some(last) if last.1 == at => last.1 = at + 1,
            _ => spans.push((at, at + 1)),
        }
    }
    spans
}

/// Match `term` against `field`, as a substring first and a scored subsequence
/// when it is not one. `None` when the term is not in the field at all, or only in
/// a subsequence too scattered to be a match.
///
/// The scattered floor is two points per term character, which a dropped-vowel
/// shape (`mthmtcl`, `dne`) clears and a sprinkle across a long field does not.
pub fn term_match(field: &str, term: &str) -> Option<Match> {
    let term: Vec<char> = term.chars().map(fold).collect();
    let q = term.len();
    if q == 0 {
        return None;
    }
    let hay: Vec<char> = field.chars().map(fold).collect();
    if q > hay.len() {
        return None;
    }
    // Substring first: the first occurrence, case-folded, walking the field once.
    let mut at = 0;
    while at + q <= hay.len() {
        if hay[at..at + q] == term[..] {
            let mut score = 5 * q as i32 - 3;
            if is_boundary(&hay, at) {
                score += 4;
            }
            return Some(Match {
                score,
                spans: vec![(at, at + q)],
            });
        }
        at += 1;
    }
    // The fuzzy pass: one walk, in order, scoring runs and boundaries and charging the gaps.
    let mut score = 0i32;
    let mut hits: Vec<usize> = Vec::with_capacity(q);
    let mut prev: Option<usize> = None;
    let mut ti = 0usize;
    for (at, c) in hay.iter().copied().enumerate() {
        if c != term[ti] {
            continue;
        }
        let mut add = 2;
        match prev {
            Some(p) if p + 1 == at => add += 3,
            Some(p) => add -= (at - p - 1).min(8) as i32,
            None => {}
        }
        if is_boundary(&hay, at) {
            add += 4;
        }
        score += add;
        hits.push(at);
        prev = Some(at);
        ti += 1;
        if ti == q {
            break;
        }
    }
    if ti < q {
        return None;
    }
    if score < 2 * q as i32 {
        return None;
    }
    Some(Match {
        score,
        spans: merge_spans(&hits),
    })
}

/// Whether a book survives `query`: every whitespace-separated term has to match
/// SOME field — title, author or address — in any order across terms, so "herbert
/// dune" and "dune herbert" answer the same.
pub fn matches(book: &Book, query: &str) -> bool {
    if !is_active(query) {
        return true;
    }
    let title = book.title();
    let author = book.author();
    let path = book.path();
    query.split_whitespace().all(|term| {
        term_match(&title, term).is_some()
            || author.as_deref().is_some_and(|a| term_match(a, term).is_some())
            || term_match(path, term).is_some()
    })
}

/// The rule [`matches`] applies without the book, because a shelf on the page is
/// searched by the same bar as the books on it: the two have to agree, or a query
/// would hide a shelf whose name it matched.
pub fn matches_terms(text: &str, query: &str) -> bool {
    if !is_active(query) {
        return true;
    }
    query
        .split_whitespace()
        .all(|term| term_match(text, term).is_some())
}

/// A blank bar means "show everything", and a bar of spaces means the same thing.
pub fn is_active(query: &str) -> bool {
    !query.trim().is_empty()
}

/// One suggestion: the book, and the spans each of its three fields lit up.
#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub book: Book,
    pub title_spans: Vec<(usize, usize)>,
    pub author_spans: Vec<(usize, usize)>,
    pub path_spans: Vec<(usize, usize)>,
    pub score: i32,
}

/// Score one book against every term, keeping the best field per term and
/// weighting them the way a reader means them: name first, author second, address
/// last. `None` when any term misses every field.
fn rank(book: &Book, query: &str) -> Option<Suggestion> {
    let title = book.title();
    let author = book.author();
    let path = book.path();
    let mut score = 0i32;
    let mut spans: [Vec<(usize, usize)>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for term in query.split_whitespace() {
        let candidates = [
            term_match(&title, term),
            author.as_deref().and_then(|a| term_match(a, term)),
            term_match(path, term),
        ];
        let weights = [3i32, 2, 1];
        let mut best: Option<(i32, usize, Vec<(usize, usize)>)> = None;
        for (field, candidate) in candidates.into_iter().enumerate() {
            let Some(m) = candidate else { continue };
            let weighted = weights[field] * m.score;
            let beats = match &best {
                Some((b, _, _)) => weighted > *b,
                None => true,
            };
            if beats {
                best = Some((weighted, field, m.spans));
            }
        }
        let (weighted, field, hit) = best?;
        score += weighted;
        spans[field].extend(hit);
    }
    for list in &mut spans {
        sort_merge(list);
    }
    Some(Suggestion {
        book: book.clone(),
        title_spans: std::mem::take(&mut spans[0]),
        author_spans: std::mem::take(&mut spans[1]),
        path_spans: std::mem::take(&mut spans[2]),
        score,
    })
}

/// Fold overlaps and neighbours together, so painting a field's hits is one walk with no double-lit characters.
fn sort_merge(spans: &mut Vec<(usize, usize)>) {
    spans.sort_unstable_by_key(|s| s.0);
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for span in spans.drain(..) {
        match merged.last_mut() {
            Some(last) if span.0 <= last.1 => last.1 = last.1.max(span.1),
            _ => merged.push(span),
        }
    }
    *spans = merged;
}

/// The books the bar suggests for a query, best first: score, then recency, then
/// title, so a tie between two editions lands on the one the reader opened last.
/// Links are not suggested: the book a link points at is.
pub fn suggest(rows: &[Row], query: &str, limit: usize) -> Vec<Suggestion> {
    if !is_active(query) {
        return Vec::new();
    }
    let mut ranked: Vec<Suggestion> = book_rows(rows).filter_map(|b| rank(b, query)).collect();
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.book.last_read_ms.cmp(&a.book.last_read_ms))
            .then_with(|| a.book.title().cmp(&b.book.title()))
    });
    ranked.truncate(limit);
    ranked
}

/// The rows a query keeps, in the order they were given — the shelf's own sort has
/// already run. A book is kept by [`matches`]; a link is kept by its NAME, through
/// the rule a shelf's name is filtered by ([`matches_terms`]), because a search
/// that hid a visible row would quietly drop results.
pub fn filter(rows: &[Row], query: &str) -> Vec<Row> {
    if !is_active(query) {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| match row {
            Row::Book(b) => matches(b, query),
            Row::Link { name, .. } => matches_terms(name, query),
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::Origin;

    fn row(title: &str, author: Option<&str>, path: &str) -> Row {
        Row::Book(book(title, author, path))
    }

    fn at(rows: &[Row], i: usize) -> &Book {
        rows[i].book().expect("a book row")
    }

    fn at_mut(rows: &mut [Row], i: usize) -> &mut Book {
        rows[i].as_book_mut().expect("a book row")
    }

    fn book(title: &str, author: Option<&str>, path: &str) -> Book {
        Book {
            title: Some(title.to_string()),
            author: author.map(str::to_string),
            origin: Origin::Linked {
                src: path.to_string(),
            },
            ..crate::testkit::book(title)
        }
    }

    #[test]
    fn a_blank_bar_filters_nothing() {
        assert!(!is_active(""));
        assert!(!is_active("   "));
        assert!(is_active("d"));
        let books = vec![row("Dune", None, "/books/dune.pdf")];
        assert_eq!(filter(&books, "").len(), 1);
        assert_eq!(filter(&books, "  ").len(), 1);
    }

    #[test]
    fn a_title_is_found_whatever_its_case() {
        let b = book("Dune Messiah", Some("Frank Herbert"), "/books/dune.pdf");
        assert!(matches(&b, "dune"));
        assert!(matches(&b, "DUNE"));
        assert!(matches(&b, "messiah"));
        assert!(!matches(&b, "foundation"));
    }

    #[test]
    fn every_term_has_to_be_found_but_the_order_does_not_matter() {
        let b = book("Dune", Some("Frank Herbert"), "/books/dune.pdf");
        assert!(matches(&b, "herbert dune"));
        assert!(matches(&b, "dune herbert"));
        assert!(matches(&b, "frank  dune"));
        assert!(!matches(&b, "dune asimov"));
    }

    #[test]
    fn the_address_is_searchable_too() {
        let b = book("Untitled scan", None, "/Books/Scifi/1984-report.pdf");
        assert!(matches(&b, "scifi"));
        assert!(matches(&b, "1984"));
        assert!(matches(&b, "report"));
    }

    #[test]
    fn a_titleless_book_is_searchable_by_its_stem() {
        let mut b = book("ignored", None, "/books/Foundation.pdf");
        b.title = None;
        assert!(matches(&b, "foundation"), "the stem is the title when there is none");
    }

    #[test]
    fn a_name_is_searched_by_the_same_rule_as_a_book() {
        assert!(matches_terms("Science Fiction", "sci"), "a prefix is enough");
        assert!(matches_terms("Science Fiction", "fiction science"));
        assert!(matches_terms("Science Fiction", "SCIENCE"));
        assert!(!matches_terms("Science Fiction", "science crime"));
        assert!(matches_terms("Science Fiction", "   "));
    }

    #[test]
    fn filtering_keeps_the_order_it_was_given() {
        let books = vec![
            row("Zebra", None, "/books/z.pdf"),
            row("Apple", None, "/books/ap.pdf"),
            row("Apricot", None, "/books/apc.pdf"),
        ];
        let kept: Vec<String> = filter(&books, "ap").iter().map(Row::display_name).collect();
        assert_eq!(kept, vec!["Apple", "Apricot"], "the shelf's own sort ran first");
        assert!(filter(&books, "q").is_empty());
    }


    #[test]
    fn a_link_is_found_by_its_name_and_never_suggested() {
        // A link is a visible row, so a search that hid it would drop results — and it is not a book, so the bar does not suggest it.
        let rows = vec![
            row("Dune", Some("Frank Herbert"), "/books/dune.pdf"),
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
            Row::link("l2".into(), "Recipes".into(), "b2".into(), 5),
        ];
        let kept = filter(&rows, "dune");
        assert_eq!(kept.len(), 2, "the book and the pointer that wears its name");
        assert!(kept.iter().any(Row::is_link));
        assert_eq!(filter(&rows, "recipes").len(), 1);
        assert_eq!(suggest(&rows, "dune", SUGGEST_LIMIT).len(), 1);
        assert_eq!(filter(&rows, "").len(), 3);
        assert_eq!(at(&rows, 0).title(), "Dune");
    }

    #[test]
    fn a_subsequence_that_holds_its_shape_is_a_match() {
        // The shapes a reader half-remembers: missing vowels, initials, a run broken once.
        let m = term_match("Mathematical Proofs", "mthmtcl").expect("fuzzy match");
        assert!(m.score >= 2 * 7);
        assert!(m.spans.len() >= 2, "runs merge, gaps split: {:?}", m.spans);
        assert!(term_match("Dune", "dne").is_some());
        assert!(term_match("The Left Hand of Darkness", "tlhod").is_some());
        // A word boundary is worth four, so the same term scores higher where it starts a word.
        let boundary = term_match("mathematical proofs", "math").unwrap();
        let midword = term_match("xxmathxx", "math").unwrap();
        assert!(boundary.score > midword.score);
    }

    #[test]
    fn a_subsequence_too_scattered_is_not_a_match() {
        // The floor is two points a character, and a sprinkle across a long field earns
        // that minus its gaps. One point lower and fuzzy would match everything.
        assert!(term_match("A Brief Collection of Zebra Essays", "xyz").is_none());
        assert!(term_match("Dune", "nud").is_none(), "order still matters");
        assert!(term_match("Dune", "dunee").is_none(), "so does count");
    }

    #[test]
    fn a_substring_always_matches_wherever_it_sits() {
        let mid = term_match("xxdunexx", "dune").expect("substring");
        assert_eq!(mid.spans, vec![(2, 6)]);
        let start = term_match("dunexx", "dune").expect("substring at start");
        assert!(start.score > mid.score, "a start is worth more than a middle");
    }

    #[test]
    fn suggestions_rank_the_name_over_the_author_over_the_address() {
        let books = vec![
            row("Notes on Dune", None, "/x/a.pdf"),
            row("Collected Papers", Some("Dune White"), "/x/b.pdf"),
            row("Collected Papers", None, "/dune/raw.pdf"),
        ];
        let ranked = suggest(&books, "dune", 3);
        let titles: Vec<String> = ranked.iter().map(|s| s.book.title()).collect();
        assert_eq!(
            titles,
            vec![
                "Notes on Dune".to_string(),
                "Collected Papers".to_string(),
                "Collected Papers".to_string()
            ],
            "title beats author beats address"
        );
        assert!(ranked[1].title_spans.is_empty());
        assert!(!ranked[1].author_spans.is_empty());
        assert!(!ranked[2].path_spans.is_empty());
    }

    #[test]
    fn suggestions_stop_at_the_limit_and_keep_the_reader_s_last() {
        let mut books: Vec<Row> = (0..12)
            .map(|i| row(&format!("Dune {i}"), None, &format!("/x/{i}.pdf")))
            .collect();
        at_mut(&mut books, 7).last_read_ms = 99;
        let ranked = suggest(&books, "dune", SUGGEST_LIMIT);
        assert_eq!(ranked.len(), SUGGEST_LIMIT);
        assert_eq!(ranked[0].book.title(), "Dune 7", "recency breaks a score tie");
    }

    #[test]
    fn a_fuzzy_query_suggests_and_the_spans_light_the_hits() {
        let books = vec![row("Mathematical Proofs", None, "/x/mp.pdf")];
        let ranked = suggest(&books, "mthprf", SUGGEST_LIMIT);
        assert_eq!(ranked.len(), 1);
        assert!(!ranked[0].title_spans.is_empty());
    }
}
