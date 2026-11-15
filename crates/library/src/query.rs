//! Search: one rule picks what the page keeps and what it suggests.

use std::cmp::Reverse;
use std::ops::Range;
use std::path::Path;

use crate::book::{Book, Origin};
use crate::id::{BookId, ShelfId};
use crate::library::Library;
use crate::view::natural;

/// One folding for every search, the page's and the library's.
pub(crate) use mareader_search::fold;

/// Seven rows fill the suggestion panel without scrolling.
pub const SUGGEST_LIMIT: usize = 7;

/// Title over author over address, as a reader names a book.
const WEIGHTS: [i32; 3] = [3, 2, 1];

/// What the reader typed: its words, each folded once.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Query {
    terms: Vec<Vec<char>>,
}

/// Byte ranges lit in each field: title (or a shelf's name), author, address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hit {
    pub score: i32,
    pub title: Vec<Range<usize>>,
    pub author: Vec<Range<usize>>,
    pub address: Vec<Range<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Target {
    Book(BookId),
    Shelf(ShelfId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub target: Target,
    pub hit: Hit,
}

impl Query {
    pub fn new(text: &str) -> Self {
        let words = text.split_whitespace();
        let terms = words.map(|w| w.chars().map(fold).collect()).collect();
        Self { terms }
    }

    /// Blank: everything is kept and nothing suggested.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Every word turns up in the book's title, author or address.
    pub fn keeps(&self, library: &Library, book: &Book) -> bool {
        self.is_empty() || self.rank(library, book).is_some()
    }

    /// A shelf by its name, under the same rule as a book.
    pub fn keeps_name(&self, name: &str) -> bool {
        self.is_empty() || self.rank_name(name).is_some()
    }

    /// Each word's best field, weighted; none if a word is nowhere.
    pub fn rank(&self, library: &Library, book: &Book) -> Option<Hit> {
        if self.is_empty() {
            return None;
        }
        let author = book.author.value.as_deref().unwrap_or("");
        let address = library.address(book);
        let fields = [
            Folded::new(&book.title.value),
            Folded::new(author),
            Folded::new(&address),
        ];
        let mut hit = Hit::default();
        for term in &self.terms {
            let found = fields.iter().zip(WEIGHTS).enumerate();
            let best = found
                .filter_map(|(at, (field, weight))| {
                    let (score, spans) = field.find(term)?;
                    Some((weight * score, at, spans))
                })
                // The earlier field wins a tie.
                .max_by_key(|(score, at, _)| (*score, Reverse(*at)))?;
            let (score, at, spans) = best;
            hit.score += score;
            let lit = match at {
                0 => &mut hit.title,
                1 => &mut hit.author,
                _ => &mut hit.address,
            };
            lit.extend(spans);
        }
        for lit in [&mut hit.title, &mut hit.author, &mut hit.address] {
            merge(lit);
        }
        Some(hit)
    }

    /// A shelf's name, weighted as a title.
    pub fn rank_name(&self, name: &str) -> Option<Hit> {
        if self.is_empty() {
            return None;
        }
        let field = Folded::new(name);
        let mut hit = Hit::default();
        for term in &self.terms {
            let (score, spans) = field.find(term)?;
            hit.score += WEIGHTS[0] * score;
            hit.title.extend(spans);
        }
        merge(&mut hit.title);
        Some(hit)
    }
}

impl Library {
    /// The books and shelves `query` is likely about, best first.
    pub fn suggest(&self, query: &Query, limit: usize) -> Vec<Suggestion> {
        if query.is_empty() {
            return Vec::new();
        }
        let books = self.books().filter_map(|b| {
            let hit = query.rank(self, b)?;
            Some((Target::Book(b.id), hit))
        });
        // A shelf cut off by a loop can't be reached to show.
        let reachable = self.shelves().filter(|s| !self.path_to(s.id).is_empty());
        let shelves = reachable.filter_map(|s| {
            let hit = query.rank_name(&s.name.value)?;
            Some((Target::Shelf(s.id), hit))
        });
        let mut found: Vec<_> = books.chain(shelves).collect();
        // Ties go to the book read last, then by name.
        found.sort_by(|(a, x), (b, y)| {
            y.score
                .cmp(&x.score)
                .then_with(|| self.last_opened(*b).cmp(&self.last_opened(*a)))
                .then_with(|| natural(&self.name_of(*a), &self.name_of(*b)))
        });
        found.truncate(limit);
        let found = found.into_iter();
        found
            .map(|(target, hit)| Suggestion { target, hit })
            .collect()
    }

    /// Where its file sits: the path in its folder, else a short tail.
    pub fn address(&self, book: &Book) -> String {
        let here = book.locations.iter().find(|l| l.present);
        let Some(location) = here.or(book.locations.first()) else {
            return String::new();
        };
        let path = &location.path;
        let inside = match location.origin {
            Origin::Folder(id) => self
                .folder(id)
                .and_then(|f| path.strip_prefix(&f.root).ok()),
            // Kept under its content id, which says nothing.
            Origin::Store => None,
            Origin::Opened => {
                let parent = path.parent().and_then(Path::parent);
                parent.and_then(|p| path.strip_prefix(p).ok())
            }
        };
        let name = || path.file_name().map(Path::new).unwrap_or(path);
        inside.unwrap_or_else(name).to_string_lossy().into_owned()
    }

    fn last_opened(&self, target: Target) -> u64 {
        match target {
            Target::Book(id) => self.book(id).map_or(0, |b| b.opened.value),
            Target::Shelf(_) => 0,
        }
    }

    fn name_of(&self, target: Target) -> String {
        let name = match target {
            Target::Book(id) => self.book(id).map(|b| &b.title.value),
            Target::Shelf(id) => self.shelf(id).map(|s| &s.name.value),
        };
        name.cloned().unwrap_or_default()
    }
}

/// A field folded char by char, remembering where each char began.
struct Folded {
    chars: Vec<char>,
    /// Byte offsets: one per char, then the end.
    bytes: Vec<usize>,
    /// Whether each char starts a word.
    starts: Vec<bool>,
}

impl Folded {
    fn new(text: &str) -> Self {
        let mut folded = Self {
            chars: Vec::with_capacity(text.len()),
            bytes: Vec::with_capacity(text.len() + 1),
            starts: Vec::with_capacity(text.len()),
        };
        let mut prev: Option<char> = None;
        for (at, c) in text.char_indices() {
            folded.chars.push(fold(c));
            folded.bytes.push(at);
            folded.starts.push(prev.is_none_or(|p| starts_word(p, c)));
            prev = Some(c);
        }
        folded.bytes.push(text.len());
        folded
    }

    /// A substring first; else an ordered subsequence whose runs beat its gaps.
    fn find(&self, term: &[char]) -> Option<(i32, Vec<Range<usize>>)> {
        let q = term.len();
        if q == 0 || q > self.chars.len() {
            return None;
        }
        let len = i32::try_from(q).unwrap_or(i32::MAX);
        if let Some(at) = self.chars.windows(q).position(|w| w == term) {
            let start = if self.word_start(at) { 4 } else { 0 };
            return Some((5 * len - 3 + start, vec![self.span(at, at + q)]));
        }
        let mut wanted = term.iter().peekable();
        let (mut score, mut prev) = (0i32, None::<usize>);
        let mut spans: Vec<Range<usize>> = Vec::new();
        for (at, c) in self.chars.iter().enumerate() {
            if wanted.next_if(|w| *w == c).is_none() {
                continue;
            }
            score += 2 + match prev {
                Some(p) if p + 1 == at => 3,
                Some(p) => -i32::try_from((at - p - 1).min(8)).unwrap_or(8),
                None => 0,
            };
            if self.word_start(at) {
                score += 4;
            }
            spans.push(self.span(at, at + 1));
            prev = Some(at);
            if wanted.peek().is_none() {
                break;
            }
        }
        // Scattered letters are a shuffle, not a match: two points a char.
        if wanted.peek().is_some() || score < 2 * len {
            return None;
        }
        merge(&mut spans);
        Some((score, spans))
    }

    fn word_start(&self, at: usize) -> bool {
        self.starts.get(at).copied().unwrap_or(false)
    }

    fn span(&self, from: usize, to: usize) -> Range<usize> {
        let byte = |i: usize| self.bytes.get(i).copied().unwrap_or(0);
        byte(from)..byte(to)
    }
}

/// After a mark, at a case step, or where letters meet digits.
fn starts_word(prev: char, c: char) -> bool {
    if !prev.is_alphanumeric() {
        return c.is_alphanumeric();
    }
    let camel = prev.is_lowercase() && c.is_uppercase();
    camel || prev.is_numeric() != c.is_numeric()
}

/// Sorts and joins touching or overlapping ranges.
fn merge(spans: &mut Vec<Range<usize>>) {
    spans.sort_unstable_by_key(|s| s.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(spans.len());
    for span in spans.drain(..) {
        match merged.last_mut() {
            Some(last) if span.start <= last.end => last.end = last.end.max(span.end),
            _ => merged.push(span),
        }
    }
    *spans = merged;
}
