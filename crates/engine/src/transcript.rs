//! Clip transcripts laid into a pane's book, in the reader's language.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use mareader_epub::{Book, Insert, Inserts, Wanted};
use mareader_settings::AiSettings;

use crate::stable_hash::hash;

/// Transcripts open in one pane, and those already laid out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Transcripts {
    /// Each open card and the track picked for it; none picks by language.
    open: BTreeMap<usize, Option<usize>>,
    made: HashMap<(usize, Option<usize>), Arc<Insert>>,
    /// The languages `made` was picked for.
    wanted: u64,
}

impl Transcripts {
    /// A click on a card's link: its transcript shown or hidden, or a track.
    pub fn click(&mut self, card: usize, track: Option<usize>) {
        match track {
            Some(t) => {
                self.open.insert(card, Some(t));
            }
            None if self.open.remove(&card).is_some() => {}
            None => {
                self.open.insert(card, None);
            }
        }
    }

    /// Shows card `card`'s transcript, in `track` if given.
    pub fn open(&mut self, card: usize, track: Option<usize>) {
        self.open.insert(card, track);
    }

    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// The open transcripts of `book` as results, and a key for the set.
    pub fn inserts(&mut self, book: &Book, wanted: &[String]) -> (u64, Vec<Arc<Insert>>) {
        let languages = hash(wanted);
        if languages != self.wanted {
            self.made.clear();
            self.wanted = languages;
        }
        let ask = Wanted {
            languages: wanted,
            name: language_name,
        };
        let mut list = Vec::with_capacity(self.open.len());
        for (&card, &track) in &self.open {
            if let Some(made) = self.made.get(&(card, track)) {
                list.push(Arc::clone(made));
                continue;
            }
            if let Some(made) = book.transcript(card, track, ask).map(Arc::new) {
                self.made.insert((card, track), Arc::clone(&made));
                list.push(made);
            }
        }
        (hash(("transcripts", &self.open, languages)).max(1), list)
    }
}

/// The page AI's results with the transcripts; the AI's alone if none open.
pub fn combine(
    ai: Option<Arc<Inserts>>,
    own: Option<(u64, Vec<Arc<Insert>>)>,
) -> Option<Arc<Inserts>> {
    let Some((key, mut list)) = own else {
        return ai;
    };
    let key = match &ai {
        Some(ai) if ai.key != 0 => hash((ai.key, key)).max(1),
        _ => key,
    };
    if let Some(ai) = &ai {
        list.splice(0..0, ai.list.iter().cloned());
    }
    Some(Arc::new(Inserts { key, list }))
}

/// Languages to show subtitles in, most wanted first.
pub fn wanted(ai: &AiSettings) -> Vec<String> {
    let tasks = &ai.tasks;
    let chosen = [&tasks.translate.language, &tasks.explain.language];
    let mut out: Vec<String> = chosen.into_iter().flatten().cloned().collect();
    out.extend(system_languages().iter().cloned());
    let mut seen = Vec::new();
    out.retain(|t| {
        let fresh = !seen.contains(t);
        seen.push(t.clone());
        fresh
    });
    out
}

/// The system's preferred languages, read once.
fn system_languages() -> &'static [String] {
    static LANGUAGES: OnceLock<Vec<String>> = OnceLock::new();
    LANGUAGES.get_or_init(read_system_languages)
}

#[cfg(not(target_arch = "wasm32"))]
fn read_system_languages() -> Vec<String> {
    // Unix locales name a variant after a dot, as `en_US.UTF-8`.
    let tags = sys_locale::get_locales().map(|l| {
        let tag = l.split(['.', '@']).next().unwrap_or_default();
        tag.replace('_', "-")
    });
    let tags = tags.filter(|t| !t.is_empty() && t != "C" && t != "POSIX");
    tags.take(4).collect()
}

#[cfg(target_arch = "wasm32")]
fn read_system_languages() -> Vec<String> {
    Vec::new()
}

/// A tag's English name, by its full tag or its first part.
fn language_name(tag: &str) -> Option<&'static str> {
    let lower = tag.to_ascii_lowercase();
    let head = lower.split('-').next().unwrap_or_default();
    mareader_settings::language_name(&lower).or_else(|| mareader_settings::language_name(head))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_opens_and_closes_and_a_track_stays_open() {
        let mut t = Transcripts::default();
        t.click(3, None);
        assert_eq!(t.open.get(&3), Some(&None));
        t.click(3, Some(1));
        assert_eq!(t.open.get(&3), Some(&Some(1)));
        t.click(3, None);
        assert!(t.is_empty());
    }

    #[test]
    fn transcripts_join_the_page_results_under_a_new_key() {
        let ai = Arc::new(Inserts {
            key: 7,
            list: Vec::new(),
        });
        assert_eq!(combine(Some(Arc::clone(&ai)), None), Some(Arc::clone(&ai)));
        let both = combine(Some(ai), Some((9, Vec::new()))).unwrap();
        assert!(both.key != 7 && both.key != 9 && both.key != 0);
        let empty = Arc::new(Inserts::default());
        assert_eq!(combine(Some(empty), Some((9, Vec::new()))).unwrap().key, 9);
    }

    #[test]
    fn named_languages_come_first_and_once() {
        let mut ai = AiSettings::default();
        ai.tasks.translate.language = Some("my".to_owned());
        ai.tasks.explain.language = Some("my".to_owned());
        let wanted = wanted(&ai);
        assert_eq!(wanted.first().map(String::as_str), Some("my"));
        assert_eq!(wanted.iter().filter(|t| *t == "my").count(), 1);
        assert_eq!(language_name("fr-CA"), Some("French"));
    }
}
