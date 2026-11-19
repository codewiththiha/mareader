//! Space: learned words filtered, sorted, grouped and exported.

use serde::{Deserialize, Serialize};

use crate::deck::{WordClass, WordKey};
use crate::mode::StudyMode;

/// Where a word stands with the reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordStatus {
    Memorized,
    #[default]
    Learning,
    Missed,
}

impl WordStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Memorized => "Memorized",
            Self::Learning => "Learning",
            Self::Missed => "Missed",
        }
    }

    /// Parses a stored status name.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "memorized" => Some(Self::Memorized),
            "learning" => Some(Self::Learning),
            "missed" => Some(Self::Missed),
            _ => None,
        }
    }
}

/// How a word was learned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LearnedBy {
    Card,
    Recall,
}

impl LearnedBy {
    pub fn of(mode: StudyMode) -> Self {
        match mode {
            StudyMode::Reactive => Self::Recall,
            _ => Self::Card,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Card => "Cards",
            Self::Recall => "Recall",
        }
    }
}

/// One word in one book, as Space lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceRow {
    pub key: WordKey,
    /// CEFR band 1 (A1) to 6 (C2).
    pub band: u8,
    pub meaning: String,
    pub context: String,
    pub book: String,
    pub status: WordStatus,
    pub by: LearnedBy,
    pub attempts: u8,
    pub first_try: bool,
    /// When last touched, wall milliseconds.
    pub at: u64,
}

/// With books, or merged across books by word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    #[default]
    Books,
    Merged,
}

/// How Space rows are ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpaceSort {
    #[default]
    Recent,
    Az,
    Hardest,
    MostTries,
    Book,
}

/// How Space rows are grouped; book grouping needs book scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SpaceGroup {
    None,
    #[default]
    Book,
    Level,
    Month,
    Status,
}

/// Space's filters; empty fields filter nothing.
#[derive(Clone, Debug, Default)]
pub struct SpaceFilter {
    pub book: Option<String>,
    pub status: Option<WordStatus>,
    pub bands: Vec<u8>,
    pub by: Option<LearnedBy>,
    pub class: Option<WordClass>,
    pub search: String,
}

/// One merged or single-book word, as Space shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct SpaceItem {
    pub key: WordKey,
    pub band: u8,
    pub meaning: String,
    pub context: String,
    pub books: Vec<String>,
    pub status: WordStatus,
    pub by: LearnedBy,
    pub attempts: u32,
    pub first_try: bool,
    pub at: u64,
    pub rows: usize,
}

/// Collapses rows to items by scope, keeping the best status.
pub fn to_items(rows: &[SpaceRow], scope: Scope) -> Vec<SpaceItem> {
    let groups: Vec<Vec<&SpaceRow>> = match scope {
        Scope::Books => rows.iter().map(|r| vec![r]).collect(),
        Scope::Merged => {
            let mut keys: Vec<WordKey> = Vec::new();
            for r in rows {
                if !keys.contains(&r.key) {
                    keys.push(r.key.clone());
                }
            }
            keys.iter()
                .map(|k| rows.iter().filter(|r| &r.key == k).collect())
                .collect()
        }
    };
    groups
        .iter()
        .filter_map(|group| {
            let head = group.iter().max_by_key(|r| r.at)?;
            let status = group.iter().map(|r| r.status).min().unwrap_or(head.status);
            let mut books: Vec<String> = group.iter().map(|r| r.book.clone()).collect();
            books.sort();
            books.dedup();
            Some(SpaceItem {
                key: head.key.clone(),
                band: head.band,
                meaning: head.meaning.clone(),
                context: head.context.clone(),
                books,
                status,
                by: head.by,
                attempts: group.iter().map(|r| u32::from(r.attempts)).sum(),
                first_try: group.iter().any(|r| r.first_try),
                at: head.at,
                rows: group.len(),
            })
        })
        .collect()
}

/// Rows passing `filter`; the status tab is applied by the caller.
pub fn filtered(rows: &[SpaceRow], filter: &SpaceFilter) -> Vec<SpaceRow> {
    let needle = filter.search.trim().to_lowercase();
    rows.iter()
        .filter(|r| filter.book.as_deref().is_none_or(|b| r.book == b))
        .filter(|r| filter.bands.is_empty() || filter.bands.contains(&r.band))
        .filter(|r| filter.by.is_none_or(|b| r.by == b))
        .filter(|r| filter.class.is_none_or(|c| r.key.class == c))
        .filter(|r| {
            needle.is_empty()
                || r.key.lemma.contains(&needle)
                || r.meaning.to_lowercase().contains(&needle)
                || r.context.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect()
}

/// Items ordered by `sort`.
pub fn sorted(mut items: Vec<SpaceItem>, sort: SpaceSort) -> Vec<SpaceItem> {
    items.sort_by(|a, b| match sort {
        SpaceSort::Recent => b.at.cmp(&a.at),
        SpaceSort::Az => a.key.lemma.cmp(&b.key.lemma),
        SpaceSort::Hardest => b
            .band
            .cmp(&a.band)
            .then_with(|| a.key.lemma.cmp(&b.key.lemma)),
        SpaceSort::MostTries => b
            .attempts
            .cmp(&a.attempts)
            .then_with(|| a.key.lemma.cmp(&b.key.lemma)),
        SpaceSort::Book => a
            .books
            .first()
            .cmp(&b.books.first())
            .then_with(|| a.key.lemma.cmp(&b.key.lemma)),
    });
    items
}

/// A CEFR label for a band, for group headings and export.
pub fn band_label(band: u8) -> &'static str {
    match band {
        1 => "A1",
        2 => "A2",
        3 => "B1",
        4 => "B2",
        5 => "C1",
        6 => "C2",
        _ => "?",
    }
}

/// Space rows as CSV, for Anki or a spreadsheet.
pub fn export_csv(items: &[SpaceItem]) -> String {
    let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out = String::from("lemma,pos,level,meaning,context,book,status\n");
    for item in items {
        let books = item.books.join("; ");
        let row = [
            esc(&item.key.lemma),
            esc(item.key.class.label()),
            esc(band_label(item.band)),
            esc(&item.meaning),
            esc(&item.context),
            esc(&books),
            esc(item.status.label()),
        ];
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

/// Memorized count and first-try recall rate, for Space's summary.
pub fn summary(rows: &[SpaceRow]) -> (usize, usize, usize) {
    let memorized = rows
        .iter()
        .filter(|r| r.status == WordStatus::Memorized)
        .count();
    let recalled = rows
        .iter()
        .filter(|r| r.by == LearnedBy::Recall && r.attempts > 0)
        .count();
    let first = rows
        .iter()
        .filter(|r| r.by == LearnedBy::Recall && r.first_try)
        .count();
    (memorized, first, recalled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(lemma: &str, book: &str, status: WordStatus) -> SpaceRow {
        SpaceRow {
            key: WordKey::new(lemma, WordClass::Noun),
            band: 5,
            meaning: "m".into(),
            context: "c".into(),
            book: book.into(),
            status,
            by: LearnedBy::Card,
            attempts: 0,
            first_try: false,
            at: 10,
        }
    }

    #[test]
    fn merged_collapses_a_word_across_books() {
        let rows = vec![
            row("dignity", "Alice", WordStatus::Learning),
            row("dignity", "Pride", WordStatus::Memorized),
        ];
        let items = to_items(&rows, Scope::Merged);
        assert_eq!(items.len(), 1);
        assert_eq!(items.first().map(|i| i.books.len()), Some(2));
        assert_eq!(items.first().map(|i| i.status), Some(WordStatus::Memorized));
        assert_eq!(to_items(&rows, Scope::Books).len(), 2);
    }

    #[test]
    fn filters_search_and_summarizes() {
        let rows = vec![row("dignity", "Alice", WordStatus::Memorized)];
        let mut f = SpaceFilter {
            search: "DIGN".into(),
            ..SpaceFilter::default()
        };
        assert_eq!(filtered(&rows, &f).len(), 1);
        f.search = "zzz".into();
        assert!(filtered(&rows, &f).is_empty());
        assert_eq!(summary(&rows), (1, 0, 0));
        assert!(export_csv(&to_items(&rows, Scope::Books)).contains("dignity"));
    }
}
