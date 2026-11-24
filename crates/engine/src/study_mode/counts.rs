//! Words mode's count: each page's hard words, from the run's first page on.
//!
//! A page's words are read with the sizes, off the frame. The levels are
//! asked in rounds of whole pages, as a deck asks them, and a page counts
//! the lemmas the book's shading marks above the level, less memorized
//! ones, each once. The planner breaks the pages by these counts.

use std::collections::{HashMap, HashSet};

use mareader_study::mode::Key;

use super::known::Known;
use super::sightings::{self, Read};
use crate::word_list::{LEVEL_ROUND, Levels};

/// Pages counted before the first plan; the rest are counted while reading.
pub const WINDOW: usize = 60;

/// The pages counted so far at one level.
pub struct Counts {
    pub from: usize,
    pub level: u8,
    /// Each page's hard words as ids, from page 0; those before `from`
    /// hold none.
    pages: Vec<Vec<u32>>,
    ids: HashMap<String, u32>,
    bands: HashMap<String, u8>,
}

impl Counts {
    pub fn new(from: usize, level: u8) -> Self {
        Self {
            from,
            level,
            pages: vec![Vec::new(); from],
            ids: HashMap::new(),
            bands: HashMap::new(),
        }
    }

    /// Every page before this one is counted.
    pub fn done(&self) -> usize {
        self.pages.len()
    }

    pub fn pages(&self) -> &[Vec<u32>] {
        &self.pages
    }

    /// Enough is counted to plan the first batches: a window past the
    /// first page, or the book's end.
    pub fn enough(&self, total: usize) -> bool {
        self.done() >= self.from.saturating_add(WINDOW).min(total)
    }

    /// Counts the next round of `reads`' pages once their levels are in.
    ///
    /// True when pages were counted; false while the levels are asked.
    pub fn step(&mut self, reads: &[Vec<Read>], levels: &mut dyn Levels, known: &Known) -> bool {
        let first = self.done();
        let mut keys: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        let mut end = first;
        while let Some(page) = reads.get(end) {
            let fresh: HashSet<String> = page.iter().flat_map(Read::keys).collect();
            let fresh: Vec<String> = fresh.into_iter().filter(|k| !seen.contains(k)).collect();
            if end > first && keys.len() + fresh.len() > LEVEL_ROUND {
                break;
            }
            seen.extend(fresh.iter().cloned());
            keys.extend(fresh);
            end += 1;
        }
        if end == first {
            return false;
        }
        let mut all = true;
        for (key, band) in keys.iter().zip(levels.bands(&keys)) {
            match band {
                Some(b) => {
                    self.bands.insert(key.clone(), b);
                }
                None => all = false,
            }
        }
        if !all {
            return false;
        }
        for page in reads.get(first..end).unwrap_or_default() {
            let hard = self.hard(page, known);
            self.pages.push(hard);
        }
        self.bands.clear();
        true
    }

    /// `page`'s distinct hard lemmas, less the known ones.
    fn hard(&mut self, page: &[Read], known: &Known) -> Vec<u32> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for read in page {
            let band = read.band(&self.bands);
            if band <= self.level {
                continue;
            }
            let lemma = read.lemma(&self.bands, band);
            if known.has(&Key::new(lemma.as_str(), None)) {
                continue;
            }
            let next = u32::try_from(self.ids.len()).unwrap_or(u32::MAX);
            let id = *self.ids.entry(lemma).or_insert(next);
            if seen.insert(id) {
                out.push(id);
            }
        }
        out
    }
}

/// A page's words as read, each form once and without its sentence.
pub fn page_words(text: &str) -> Vec<Read> {
    let mut seen = HashSet::new();
    let mut out = sightings::words(text);
    out.retain(|r| seen.insert(r.form.clone()));
    for read in &mut out {
        read.sentence = String::new();
    }
    out.shrink_to_fit();
    out
}

/// Test levels: a key of 8 letters or more is C2, all answered, once
/// the word data is there.
#[cfg(test)]
pub(crate) struct Lengths(pub bool);

#[cfg(test)]
impl Levels for Lengths {
    fn coming(&self) -> bool {
        false
    }

    fn ready(&self) -> bool {
        self.0
    }

    fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        let band = |k: &String| Some(if k.len() >= 8 { 6 } else { 1 });
        keys.iter().map(band).collect()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn pages_count_their_hard_words_once_less_the_known() {
        let reads: Vec<Vec<Read>> = [
            "The cat sat. Remarkable, remarkable atmosphere.",
            "Atmosphere changed thoroughly. A wonderful, remarkable day.",
            "Nothing new.",
        ]
        .into_iter()
        .map(page_words)
        .collect();
        let known = Known::of([Key::new("wonderful", None)]);
        let mut counts = Counts::new(0, 3);
        assert!(counts.step(&reads, &mut Lengths(true), &known));
        assert_eq!(counts.done(), 3);
        let pages = counts.pages();
        assert_eq!(pages[0].len(), 2, "remarkable once, atmosphere");
        assert_eq!(pages[1].len(), 3, "not the known wonderful");
        assert!(pages[2].is_empty());
        assert!(pages[1].contains(&pages[0][0]), "one word, one id");
        let again = counts.step(&reads, &mut Lengths(true), &known);
        assert!(!again, "all counted");
    }

    #[test]
    fn counting_starts_at_the_first_page_and_knows_when_to_plan() {
        let reads = vec![page_words("Remarkable."); 70];
        let mut counts = Counts::new(5, 3);
        assert!(!counts.enough(70));
        while counts.step(&reads, &mut Lengths(true), &Known::default()) {}
        assert!(counts.enough(70));
        assert!(counts.pages()[4].is_empty());
        assert_eq!(counts.pages()[5].len(), 1);
        assert!(Counts::new(3, 3).enough(3), "nothing left to count");
    }
}
