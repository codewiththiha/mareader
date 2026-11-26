//! Pre-normalized search index over the setting catalog.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use mareader_engine::settings::Tab;

const CATALOG: &str = include_str!("catalog.txt");

/// A destination in the existing settings pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub tab: Tab,
    pub section: &'static str,
    pub title: &'static str,
    pub keywords: &'static str,
    pub conditional: bool,
}

struct Indexed {
    entry: Entry,
    title: String,
    words: String,
}

struct Index {
    entries: Vec<Indexed>,
    grams: HashMap<String, Vec<usize>>,
}

static INDEX: OnceLock<Index> = OnceLock::new();

fn normalize(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    }
    out.trim_end().to_owned()
}

fn parse(line: &'static str) -> Option<Entry> {
    let mut fields = line.split('|');
    let tab = match fields.next()? {
        "Appearance" => Tab::Appearance,
        "Layout" => Tab::Layout,
        "ReadAloud" => Tab::ReadAloud,
        "Highlights" => Tab::Highlights,
        "Vocabulary" => Tab::Vocabulary,
        "Dictionary" => Tab::Dictionary,
        "Ai" => Tab::Ai,
        "Skills" => Tab::Skills,
        "Study" => Tab::Study,
        "Models" => Tab::Models,
        "Panes" => Tab::Panes,
        "Motion" => Tab::Motion,
        "Updates" => Tab::Updates,
        _ => return None,
    };
    let section = fields.next()?;
    let title = fields.next()?;
    let keywords = fields.next()?;
    let conditional = !fields.next()?.is_empty();
    Some(Entry {
        tab,
        section,
        title,
        keywords,
        conditional,
    })
}

fn grams(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.windows(3).map(|w| w.iter().collect()).collect()
}

fn index() -> &'static Index {
    INDEX.get_or_init(|| {
        let entries: Vec<Indexed> = CATALOG
            .lines()
            .filter_map(parse)
            .map(|entry| {
                let title = normalize(entry.title);
                let words = normalize(&format!(
                    "{} {} {} {}",
                    entry.title,
                    entry.section,
                    entry.tab.label(),
                    entry.keywords
                ));
                Indexed {
                    entry,
                    title,
                    words,
                }
            })
            .collect();
        let mut postings: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, item) in entries.iter().enumerate() {
            for gram in grams(&item.words).into_iter().collect::<HashSet<_>>() {
                postings.entry(gram).or_default().push(i);
            }
        }
        Index {
            entries,
            grams: postings,
        }
    })
}

/// Exact labels, then prefixes, then all query terms in the indexed text.
pub(super) fn find(query: &str, allowed: impl Fn(Tab) -> bool) -> Vec<Entry> {
    let query = normalize(query);
    if query.is_empty() {
        return Vec::new();
    }
    let index = index();
    let mut shortest: Option<&[usize]> = None;
    for gram in query.split_whitespace().flat_map(grams) {
        let Some(posting) = index.grams.get(&gram) else {
            return Vec::new();
        };
        if shortest.is_none_or(|old| posting.len() < old.len()) {
            shortest = Some(posting);
        }
    }
    let candidates: Box<dyn Iterator<Item = usize>> = match shortest {
        Some(ids) => Box::new(ids.iter().copied()),
        None => Box::new(0..index.entries.len()),
    };
    let mut hits: Vec<(u8, usize, Entry)> = candidates
        .filter_map(|i| {
            let item = index.entries.get(i)?;
            if !allowed(item.entry.tab)
                || !query
                    .split_whitespace()
                    .all(|term| item.words.contains(term))
            {
                return None;
            }
            let rank = if item.title == query {
                0
            } else if item.title.starts_with(&query) {
                1
            } else if item.title.contains(&query) {
                2
            } else {
                3
            };
            Some((rank, i, item.entry))
        })
        .collect();
    hits.sort_by_key(|(rank, i, _)| (*rank, *i));
    hits.into_iter().map(|(_, _, entry)| entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_queries_rank_labels_and_match_aliases() {
        let results = find("  PAGE--INDICATOR ", |_| true);
        assert_eq!(results.first().map(|e| e.title), Some("Page indicator"));
        assert!(
            find("hover dictionary", |_| true)
                .iter()
                .any(|e| e.title.starts_with("Rest on a shaded"))
        );
        assert!(
            find("font size", |_| true)
                .iter()
                .any(|e| e.title == "Text size")
        );
        assert!(find("not a real setting xyz", |_| true).is_empty());
        assert!(find("font", |t| t == Tab::Motion).is_empty());
    }
    #[test]
    fn catalog_covers_every_available_static_tab() {
        for tab in Tab::ALL {
            assert!(
                index().entries.iter().any(|e| e.entry.tab == tab),
                "{tab:?}"
            );
        }
        assert!(index().entries.len() >= 150);
        assert_eq!(
            index().entries.len(),
            CATALOG.lines().count(),
            "every catalog entry parses"
        );
        assert!(std::ptr::eq(index(), index()), "index should be reused");
    }
    #[test]
    fn indexed_search_matches_a_full_scan() {
        for q in [
            "a",
            "ai",
            "font",
            "font size",
            "dictionary hover",
            "light",
            "spacing",
            "no-such-setting",
        ] {
            let norm = normalize(q);
            let expected: HashSet<_> = index()
                .entries
                .iter()
                .filter(|i| norm.split_whitespace().all(|term| i.words.contains(term)))
                .map(|i| (i.entry.tab.label(), i.entry.title))
                .collect();
            let got: HashSet<_> = find(q, |_| true)
                .into_iter()
                .map(|e| (e.tab.label(), e.title))
                .collect();
            assert_eq!(got, expected, "{q}");
        }
    }

    #[test]
    fn repeated_queries_reuse_the_index() {
        let _ = index();
        let start = std::time::Instant::now();
        for _ in 0..5000 {
            std::hint::black_box(find("dictionary hover", |_| true));
        }
        eprintln!("5000 indexed queries: {:?}", start.elapsed());
    }

    // Guards literal labels passed through the shared row helpers. Computed
    // labels and custom controls still require a manual catalog review.
    #[test]
    fn literal_rows_in_settings_modules_are_indexed() {
        let sources = [
            (Tab::Appearance, include_str!("../fonts.rs")),
            (Tab::Highlights, include_str!("../highlights.rs")),
            (Tab::Vocabulary, include_str!("../vocabulary.rs")),
            (Tab::Dictionary, include_str!("../dictionary.rs")),
            (Tab::ReadAloud, include_str!("../read_aloud.rs")),
            (Tab::Ai, include_str!("../ai.rs")),
            (Tab::Skills, include_str!("../skills.rs")),
            (Tab::Study, include_str!("../study.rs")),
            (Tab::Models, include_str!("../models.rs")),
            (Tab::Updates, include_str!("../updates.rs")),
        ];
        for (tab, source) in sources {
            for name in [
                "row(",
                "switch_row(",
                "stepped(",
                "picker(",
                "count_row(",
                "detail_row(",
                "disclosure(",
            ] {
                for (at, _) in source.match_indices(name) {
                    if source[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_alphanumeric() || c == '_')
                    {
                        continue;
                    }
                    let call = &source[at + name.len()..];
                    let Some(args) = call.trim_start().strip_prefix("ui,") else {
                        continue;
                    };
                    let args = args.trim_start().trim_start_matches('(').trim_start();
                    let Some(label) = args.strip_prefix('"').and_then(|s| s.split('"').next())
                    else {
                        continue;
                    };
                    assert!(
                        index()
                            .entries
                            .iter()
                            .any(|i| i.entry.tab == tab && i.entry.title == label),
                        "{tab:?}: {label}"
                    );
                }
            }
        }
    }
}
