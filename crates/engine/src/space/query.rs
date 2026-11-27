//! The library's studied words read into Space's rows, for one filter.

use std::cmp::{Ordering, Reverse};
use std::collections::BTreeMap;

use mareader_library::{BookId, LearnedBy, Library, WordSighting, WordState, WordStatus};

use super::{
    BANDS, Counts, Filter, Group, History, MetAt, Row, Scope, Section, Shelf, Sort, Stats, View,
};
use crate::export::civil;

const DAY_MS: u64 = 86_400_000;
/// Words of a sentence its book is searched for.
const QUOTE_WORDS: usize = 7;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// A sighting beside its word and its book's title.
pub(super) struct Met<'a> {
    pub(super) seen: &'a WordSighting,
    word: &'a WordState,
    title: &'a str,
}

/// Everything Space shows for `filter`, read from `lib`.
pub fn view(lib: &Library, filter: &Filter) -> View {
    let met = met(lib);
    let needle = filter.text.trim().to_lowercase();
    let base: Vec<&Met<'_>> = met.iter().filter(|m| passes(m, filter, &needle)).collect();
    let mut rows = match filter.scope {
        Scope::Books => apart(&base),
        Scope::Merged => merged(&base),
    };
    let counts = counts(&rows);
    let stats = stats(&base, &rows);
    rows.retain(|r| filter.tab.holds(r.status));
    rows.sort_by(|a, b| order(filter.sort, a, b));
    View {
        sections: sections(rows, filter),
        counts,
        stats,
        books: shelves(lib, &met),
        total: met.len(),
        poses: poses(&met),
    }
}

/// `ms`'s day in UTC, including its year.
pub fn date(ms: u64) -> String {
    let (year, _, _) = civil(ms / DAY_MS);
    format!("{}, {year}", day(ms))
}

/// `ms`'s day in UTC, as Nov 21.
pub fn day(ms: u64) -> String {
    let (_, m, d) = civil(ms / DAY_MS);
    let name = month_name(m);
    format!("{} {d}", name.get(..3).unwrap_or(name))
}

/// `ms`'s month in UTC, as November 2026.
fn month(ms: u64) -> String {
    let (y, m, _) = civil(ms / DAY_MS);
    format!("{} {y}", month_name(m))
}

fn month_name(m: u64) -> &'static str {
    let at = usize::try_from(m.saturating_sub(1)).unwrap_or(0);
    MONTHS.get(at).copied().unwrap_or("")
}

pub(super) fn met(lib: &Library) -> Vec<Met<'_>> {
    let mut out = Vec::new();
    for seen in lib.every_sighting() {
        let (word, book) = (lib.word(seen.word), lib.book(seen.book.value));
        if let (Some(word), Some(book)) = (word, book) {
            let title = book.title.value.as_str();
            out.push(Met { seen, word, title });
        }
    }
    out
}

fn passes(m: &Met<'_>, f: &Filter, needle: &str) -> bool {
    let book = f.book.is_none_or(|b| b == m.seen.book.value);
    let level = f.levels.is_empty() || f.levels.contains(&m.word.band);
    let by = f.by.is_none_or(|by| by == m.seen.by.value);
    let wrote = f.meaning_by.is_none() || f.meaning_by == m.seen.meaning_by.value;
    let pos = f.pos.is_none() || f.pos == m.word.pos;
    let picked = book && level && by && wrote && pos;
    picked && (needle.is_empty() || finds(m, needle))
}

fn finds(m: &Met<'_>, needle: &str) -> bool {
    let fields = [&m.word.lemma, &m.seen.meaning.value, &m.seen.context.value];
    fields.iter().any(|f| f.to_lowercase().contains(needle))
}

/// One row from `mets`, the most recent first, known by `key`.
fn row(key: u128, mets: &[&Met<'_>]) -> Option<Row> {
    let head = mets.first()?;
    let (mut books, mut book_ids) = (Vec::new(), Vec::new());
    for m in mets {
        if !book_ids.contains(&m.seen.book.value) {
            books.push(m.title.to_owned());
            book_ids.push(m.seen.book.value);
        }
    }
    let said = mets.iter().find(|m| !m.seen.context.value.is_empty());
    let met_at = said.map(|m| MetAt {
        book: m.seen.book.value,
        quote: words_around(&m.seen.context.value, &m.seen.form.value),
    });
    Some(Row {
        key,
        word: head.word.id,
        lemma: head.word.lemma.clone(),
        pos: head.word.pos.clone(),
        band: head.word.band,
        status: head.word.status.value,
        meaning: first_said(mets.iter().map(|m| m.seen.meaning.value.as_str())),
        context: first_said(mets.iter().map(|m| m.seen.context.value.as_str())),
        form: said.map_or_else(String::new, |m| m.seen.form.value.clone()),
        books,
        book_ids,
        met_at,
        seen: head.seen.seen.value,
        tries: mets.iter().map(|m| u32::from(m.seen.tries.value)).sum(),
        by: head.seen.by.value,
        first_try: mets.iter().any(|m| m.seen.first_try.value),
        synonyms: unique(mets.iter().flat_map(|m| m.seen.synonyms.value.iter())),
        usages: unique(mets.iter().flat_map(|m| m.seen.usages.value.iter())),
    })
}

fn unique<'a>(items: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if !item.trim().is_empty() && !out.contains(item) {
            out.push(item.clone());
        }
    }
    out
}

/// Expand a clicked row with every other book where its word was encountered.
/// The row's own books stay first, without changing its sentence or meaning.
pub fn detail_row(lib: &Library, mut row: Row) -> Row {
    let mut seen = lib.sightings_of(row.word);
    seen.sort_by_key(|s| Reverse(s.seen.value));
    for sighting in seen {
        let id = sighting.book.value;
        if row.book_ids.contains(&id) {
            continue;
        }
        if let Some(book) = lib.book(id) {
            row.book_ids.push(id);
            row.books.push(book.title.value.clone());
        }
    }
    row
}

/// Recall sessions belonging to a displayed word's included books.
pub fn history(lib: &Library, row: &Row) -> Vec<History> {
    let mut out = Vec::new();
    for seen in lib.sightings_of(row.word) {
        if !row.book_ids.contains(&seen.book.value) {
            continue;
        }
        let Some(book) = lib.book(seen.book.value) else {
            continue;
        };
        for run in seen.recall_history.sessions.values() {
            out.push(History {
                book: book.title.value.clone(),
                when: run.when,
                answers: run.answers.clone(),
            });
        }
    }
    out.sort_by_key(|run| Reverse(run.when));
    out
}

/// Up to [`QUOTE_WORDS`] of `sentence`'s words around `form`, else its
/// first ones: few enough to find through a page's line breaks.
pub(super) fn words_around(sentence: &str, form: &str) -> String {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    let bare = |w: &&str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
    };
    let form = form.trim().to_lowercase();
    let at = words.iter().position(|w| bare(w) == form).unwrap_or(0);
    let from = at.saturating_sub(QUOTE_WORDS / 2);
    let from = from.min(words.len().saturating_sub(QUOTE_WORDS));
    let near = words.iter().skip(from).take(QUOTE_WORDS);
    near.copied().collect::<Vec<_>>().join(" ")
}

/// The first of `texts` that says something.
fn first_said<'a>(mut texts: impl Iterator<Item = &'a str>) -> String {
    texts.find(|t| !t.is_empty()).unwrap_or_default().to_owned()
}

/// A row per sighting.
fn apart(base: &[&Met<'_>]) -> Vec<Row> {
    let mut rows = Vec::new();
    for m in base {
        rows.extend(row(m.seen.id.0, &[*m]));
    }
    rows
}

/// A row per word, its sightings folded in.
fn merged(base: &[&Met<'_>]) -> Vec<Row> {
    let mut by_word: BTreeMap<u128, Vec<&Met<'_>>> = BTreeMap::new();
    for m in base {
        by_word.entry(m.word.id.0).or_default().push(*m);
    }
    let mut rows = Vec::new();
    for (key, mut mets) in by_word {
        mets.sort_by_key(|m| Reverse(m.seen.seen.value));
        rows.extend(row(key, &mets));
    }
    rows
}

fn counts(rows: &[Row]) -> Counts {
    let of = |status| rows.iter().filter(|r| r.status == status).count();
    Counts {
        all: rows.len(),
        memorized: of(WordStatus::Memorized),
        learning: of(WordStatus::Learning),
        missed: of(WordStatus::Missed),
    }
}

/// Answers count per sighting; memorized words per row.
fn stats(base: &[&Met<'_>], rows: &[Row]) -> Stats {
    let asked = base.iter().filter(|m| answered(m));
    let asked: Vec<bool> = asked.map(|m| m.seen.first_try.value).collect();
    let memorized = |band: u8| {
        let held = |r: &&Row| r.band == band && r.status == WordStatus::Memorized;
        rows.iter().filter(held).count()
    };
    Stats {
        first_try: asked.iter().filter(|first| **first).count(),
        answered: asked.len(),
        by_level: BANDS.map(|band| (band, memorized(band))),
    }
}

fn answered(m: &Met<'_>) -> bool {
    m.seen.by.value == LearnedBy::Reactive && m.seen.tries.value > 0
}

fn order(sort: Sort, a: &Row, b: &Row) -> Ordering {
    let az = || a.lemma.cmp(&b.lemma);
    let first = match sort {
        Sort::Recent => b.seen.cmp(&a.seen),
        Sort::Az => az(),
        Sort::Hardest => b.band.cmp(&a.band).then_with(az),
        Sort::Tries => b.tries.cmp(&a.tries).then_with(az),
        Sort::Book => a.books.first().cmp(&b.books.first()).then_with(az),
    };
    first.then(a.key.cmp(&b.key))
}

/// Sections in the order their first row comes; levels hardest first.
fn sections(rows: Vec<Row>, filter: &Filter) -> Vec<Section> {
    let group = match (filter.scope, filter.group) {
        (Scope::Merged, Group::Book) => Group::None,
        (_, group) => group,
    };
    if group == Group::None {
        let label = String::new();
        return vec![Section { label, rows }];
    }
    let mut out: Vec<Section> = Vec::new();
    for row in rows {
        let label = heading(group, &row);
        match out.iter_mut().find(|s| s.label == label) {
            Some(section) => section.rows.push(row),
            None => out.push(Section {
                label,
                rows: vec![row],
            }),
        }
    }
    if group == Group::Level {
        out.sort_by_key(|s| Reverse(s.rows.first().map(|r| r.band)));
    }
    out
}

fn heading(group: Group, row: &Row) -> String {
    match group {
        Group::None => String::new(),
        Group::Book => row.books.first().cloned().unwrap_or_default(),
        Group::Level => mareader_cefr::label(row.band).to_owned(),
        Group::Day => date(row.seen),
        Group::Month => month(row.seen),
        Group::Status => super::status_label(row.status).to_owned(),
    }
}

/// Each book with a sighting, by title, its sightings counted.
fn shelves(lib: &Library, met: &[Met<'_>]) -> Vec<Shelf> {
    let mut counts: BTreeMap<BookId, usize> = BTreeMap::new();
    for m in met {
        *counts.entry(m.seen.book.value).or_default() += 1;
    }
    let one = |(id, count): (BookId, usize)| {
        let book = lib.book(id)?;
        let title = book.title.value.clone();
        let format = book.format;
        Some(Shelf {
            id,
            title,
            format,
            count,
        })
    };
    let mut out: Vec<Shelf> = counts.into_iter().filter_map(one).collect();
    out.sort_by(|a, b| a.title.cmp(&b.title).then(a.id.cmp(&b.id)));
    out
}

fn poses(met: &[Met<'_>]) -> Vec<String> {
    let mut out: Vec<String> = met.iter().filter_map(|m| m.word.pos.clone()).collect();
    out.sort();
    out.dedup();
    out
}
