//! Word list options: picked pages' hard words and their meanings.

use serde::{Deserialize, Serialize};

/// At most this many languages sit beside English.
pub const WORD_LANGUAGES: usize = 3;
/// At most this many presets are kept.
pub const WORD_PRESETS: usize = 24;

/// What a word list holds and how it reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordList {
    /// Dictionary languages, in column order; none lists the words alone.
    pub languages: Vec<String>,
    pub fields: WordFields,
    pub sort: WordSort,
    pub group: WordGroup,
    /// Each word's page, where it first shows.
    pub pages: bool,
    /// The book's title heads the list.
    pub book: bool,
    /// Dictionary words near each word, under the word that found them.
    pub similar: bool,
    pub layout: WordLayout,
    pub output: WordOutput,
}

impl Default for WordList {
    fn default() -> Self {
        Self {
            languages: vec!["my".into()],
            fields: WordFields::default(),
            sort: WordSort::default(),
            group: WordGroup::default(),
            pages: true,
            book: true,
            similar: false,
            layout: WordLayout::default(),
            output: WordOutput::default(),
        }
    }
}

impl WordList {
    pub fn sanitize(&mut self) {
        for lang in &mut self.languages {
            *lang = mareader_dict::language_code(lang);
        }
        let mut seen = Vec::new();
        self.languages.retain(|l| {
            let keep = !l.trim().is_empty() && !seen.contains(l);
            seen.push(l.clone());
            keep
        });
        self.languages.truncate(WORD_LANGUAGES);
    }
}

/// Which parts of a dictionary row are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordFields {
    /// The word's CEFR level, from the word list's dataset.
    pub level: bool,
    pub part_of_speech: bool,
    pub definition: bool,
    pub romanization: bool,
    /// Which sense of the word a row translates.
    pub sense: bool,
}

impl Default for WordFields {
    fn default() -> Self {
        Self {
            level: true,
            part_of_speech: true,
            definition: true,
            romanization: false,
            sense: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordSort {
    /// As the words first show in the book.
    #[default]
    Reading,
    AToZ,
    ZToA,
    /// Hardest first; reading order within a level.
    Level,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordGroup {
    None,
    /// Under the outline's chapters; none when the book has no outline.
    #[default]
    Chapter,
    Page,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordLayout {
    /// One row per word, a column per language.
    #[default]
    Table,
    /// A word per paragraph, a line per language.
    List,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WordOutput {
    #[default]
    Copy,
    Markdown,
    Epub,
}

/// A named set of word list options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordPreset {
    pub name: String,
    pub list: WordList,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordListSettings {
    /// The options last used; the preview opens with them.
    pub last: WordList,
    pub presets: Vec<WordPreset>,
    /// The preset an instant export uses, by name.
    pub instant: Option<String>,
}

impl WordListSettings {
    pub fn sanitize(&mut self) {
        self.last.sanitize();
        let mut seen: Vec<String> = Vec::new();
        self.presets.retain_mut(|p| {
            p.name = p.name.trim().to_owned();
            p.list.sanitize();
            let fresh = !p.name.is_empty() && !seen.contains(&p.name);
            seen.push(p.name.clone());
            fresh
        });
        self.presets.truncate(WORD_PRESETS);
        let named = |n: &String| self.presets.iter().any(|p| &p.name == n);
        if !self.instant.as_ref().is_some_and(named) {
            self.instant = None;
        }
    }

    /// The options an instant export uses.
    pub fn instant(&self) -> &WordList {
        let preset = self
            .presets
            .iter()
            .find(|p| Some(&p.name) == self.instant.as_ref());
        preset.map_or(&self.last, |p| &p.list)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_three_distinct_languages_and_named_presets() {
        let mut s = WordListSettings::default();
        s.last.languages = ["my", "my", "", "jp", "French", "de"]
            .map(String::from)
            .to_vec();
        let mine = |name: &str| WordPreset {
            name: name.into(),
            list: WordList::default(),
        };
        s.presets = vec![mine(" Study "), mine("Study"), mine("  ")];
        s.instant = Some("Gone".into());
        s.sanitize();
        assert_eq!(s.last.languages, ["my", "jp", "fr"]);
        assert_eq!(s.presets.len(), 1);
        assert_eq!(s.presets.first().unwrap().name, "Study");
        assert_eq!(s.instant, None);
        s.instant = Some("Study".into());
        s.presets.first_mut().unwrap().list.similar = true;
        s.sanitize();
        assert!(s.instant().similar);
    }

    #[test]
    fn old_settings_without_word_lists_read_as_defaults() {
        let s: WordListSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.last.languages, ["my"]);
        assert_eq!(s.last.group, WordGroup::Chapter);
    }
}
