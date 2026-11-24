//! A word's class, and the one rule every card's class follows.
//!
//! Three sources may name it: the tagger reading it in its sentence, an AI
//! asked about the word, and a dictionary's part-of-speech cell.
//! [`Classes::class`] takes the first that says one, in that order. The
//! tagger leads because it is the same reading wherever the word is asked;
//! the AI is asked a class only where the tagger had none, and a
//! dictionary, which lists every class a word may take, comes last. Packs
//! write their cells short or long ("v", "verb", "Adj.", "n, v");
//! [`Pos::read`] reads them all alike.

use serde::{Deserialize, Serialize};

/// The word classes cards and saved words name.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pos {
    Noun,
    Verb,
    Adjective,
    Adverb,
    Pronoun,
    Preposition,
    Conjunction,
    Determiner,
    Interjection,
    Numeral,
}

/// Cell spellings packs and models use, short and long, as written.
const SPELLED: &[(&str, Pos)] = &[
    ("n", Pos::Noun),
    ("ns", Pos::Noun),
    ("nn", Pos::Noun),
    ("noun", Pos::Noun),
    ("nouns", Pos::Noun),
    ("name", Pos::Noun),
    ("propn", Pos::Noun),
    ("proper_noun", Pos::Noun),
    ("v", Pos::Verb),
    ("vs", Pos::Verb),
    ("vb", Pos::Verb),
    ("vt", Pos::Verb),
    ("vi", Pos::Verb),
    ("verb", Pos::Verb),
    ("verbs", Pos::Verb),
    ("aux", Pos::Verb),
    ("auxiliary", Pos::Verb),
    ("cop", Pos::Verb),
    ("copula", Pos::Verb),
    ("modal", Pos::Verb),
    ("a", Pos::Adjective),
    ("aj", Pos::Adjective),
    ("adj", Pos::Adjective),
    ("adjs", Pos::Adjective),
    ("adjective", Pos::Adjective),
    ("adv", Pos::Adverb),
    ("advs", Pos::Adverb),
    ("adverb", Pos::Adverb),
    ("indefadv", Pos::Adverb),
    ("advpart", Pos::Adverb),
    ("pron", Pos::Pronoun),
    ("pers", Pos::Pronoun),
    ("indefpron", Pos::Pronoun),
    ("possess", Pos::Pronoun),
    ("pronoun", Pos::Pronoun),
    ("prep", Pos::Preposition),
    ("postp", Pos::Preposition),
    ("adp", Pos::Preposition),
    ("preposition", Pos::Preposition),
    ("postposition", Pos::Preposition),
    ("conj", Pos::Conjunction),
    ("cconj", Pos::Conjunction),
    ("sconj", Pos::Conjunction),
    ("conjunction", Pos::Conjunction),
    ("det", Pos::Determiner),
    ("indefdet", Pos::Determiner),
    ("art", Pos::Determiner),
    ("article", Pos::Determiner),
    ("indefart", Pos::Determiner),
    ("determiner", Pos::Determiner),
    ("interj", Pos::Interjection),
    ("intj", Pos::Interjection),
    ("excl", Pos::Interjection),
    ("exclamation", Pos::Interjection),
    ("interjection", Pos::Interjection),
    ("num", Pos::Numeral),
    ("number", Pos::Numeral),
    ("numeral", Pos::Numeral),
];

/// Words that name a class wherever they stand, as in "phrasal verb".
const NAMING: &[&str] = &[
    "noun",
    "verb",
    "adjective",
    "adj",
    "adverb",
    "adv",
    "pronoun",
    "pron",
    "preposition",
    "prep",
    "conjunction",
    "conj",
    "determiner",
    "article",
    "interjection",
    "numeral",
];

impl Pos {
    pub const ALL: [Self; 10] = [
        Self::Noun,
        Self::Verb,
        Self::Adjective,
        Self::Adverb,
        Self::Pronoun,
        Self::Preposition,
        Self::Conjunction,
        Self::Determiner,
        Self::Interjection,
        Self::Numeral,
    ];

    /// The class of a Penn tag such as `VBD`; none for one naming none.
    pub fn of_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim();
        let class = match tag {
            "MD" | "VB" | "VBD" | "VBG" | "VBN" | "VBP" | "VBZ" => Self::Verb,
            "NN" | "NNS" | "NNP" | "NNPS" => Self::Noun,
            "JJ" | "JJR" | "JJS" => Self::Adjective,
            "RB" | "RBR" | "RBS" | "WRB" | "RP" => Self::Adverb,
            "PRP" | "PRP$" | "WP" | "WP$" | "EX" => Self::Pronoun,
            "IN" => Self::Preposition,
            "CC" => Self::Conjunction,
            "DT" | "PDT" | "WDT" => Self::Determiner,
            "UH" => Self::Interjection,
            "CD" => Self::Numeral,
            _ => return None,
        };
        Some(class)
    }

    /// The class a dictionary cell or a model's answer names, if any.
    ///
    /// Parts split on commas and the like count in order: "v, n" is a verb.
    pub fn read(cell: &str) -> Option<Self> {
        Self::read_all(cell).next()
    }

    /// Every class a cell names, in order: "v, n" is a verb, then a noun.
    pub fn read_all(cell: &str) -> impl Iterator<Item = Self> + '_ {
        let parts = cell.split([',', ';', '/', '|', '&', '+']);
        parts.map(str::trim).filter_map(read_part)
    }

    /// A cell as a dictionary row shows it: "v,n" is "verb · noun"; one
    /// naming no class, such as "prefix", stays as written.
    pub fn spell(cell: &str) -> String {
        let mut names: Vec<&str> = Vec::new();
        for name in Self::read_all(cell).map(Self::name) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        match names.is_empty() {
            true => cell.trim().to_owned(),
            false => names.join(" · "),
        }
    }

    /// The first class `cells` name, as dictionary rows give them.
    pub fn of_cells<'a>(cells: impl IntoIterator<Item = &'a str>) -> Option<Self> {
        cells.into_iter().find_map(Self::read)
    }

    /// The class as stored and shown, as "verb".
    pub fn name(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adjective",
            Self::Adverb => "adverb",
            Self::Pronoun => "pronoun",
            Self::Preposition => "preposition",
            Self::Conjunction => "conjunction",
            Self::Determiner => "determiner",
            Self::Interjection => "interjection",
            Self::Numeral => "numeral",
        }
    }

    /// A stored class back, read as leniently as a cell.
    pub fn parse(name: &str) -> Option<Self> {
        Self::read(name)
    }
}

/// One part of a cell: a known spelling whole, else a naming word inside.
fn read_part(part: &str) -> Option<Pos> {
    let bare = |c: char| c.is_whitespace() || matches!(c, '.' | '(' | ')' | '[' | ']');
    let part = part.trim_matches(bare).replace('.', "").to_lowercase();
    if part.is_empty() {
        return None;
    }
    let spelled = |s: &str| SPELLED.iter().find(|(w, _)| *w == s).map(|(_, p)| *p);
    if let Some(class) = spelled(&part) {
        return Some(class);
    }
    let words = part.split(|c: char| !c.is_alphabetic());
    words.filter(|w| NAMING.contains(w)).find_map(spelled)
}

/// What each source says of one word's class; strongest first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Classes {
    /// The tagger's reading in the word's sentence.
    pub tagger: Option<Pos>,
    /// An AI's answer, on cards an AI wrote.
    pub ai: Option<Pos>,
    /// A dictionary's cell for the word.
    pub dictionary: Option<Pos>,
}

impl Classes {
    /// The class a card shows: the first source that names one.
    pub fn class(self) -> Option<Pos> {
        self.tagger.or(self.ai).or(self.dictionary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_long_cells_read_alike() {
        for (cell, class) in [
            ("v", Pos::Verb),
            ("verb", Pos::Verb),
            ("Verb", Pos::Verb),
            ("v.t.", Pos::Verb),
            ("n", Pos::Noun),
            ("N.", Pos::Noun),
            ("adj", Pos::Adjective),
            ("Adj.", Pos::Adjective),
            ("a", Pos::Adjective),
            (" Adv. ", Pos::Adverb),
            ("prep", Pos::Preposition),
            ("ADP", Pos::Preposition),
            ("article", Pos::Determiner),
            ("num", Pos::Numeral),
        ] {
            assert_eq!(Pos::read(cell), Some(class), "{cell}");
        }
    }

    #[test]
    fn a_list_reads_its_first_class_and_phrases_their_naming_word() {
        assert_eq!(Pos::read("v,n"), Some(Pos::Verb));
        assert_eq!(Pos::read("prefix; n"), Some(Pos::Noun));
        assert_eq!(Pos::read("phrasal verb"), Some(Pos::Verb));
        assert_eq!(Pos::read("proper noun"), Some(Pos::Noun));
        assert_eq!(Pos::read("adj (comparative)"), Some(Pos::Adjective));
    }

    #[test]
    fn no_class_reads_from_what_names_none() {
        for cell in [
            "", "  ", "prefix", "phrase", "symbol", "abbr", "Gerund", "x",
        ] {
            assert_eq!(Pos::read(cell), None, "{cell}");
        }
        assert_eq!(Pos::read("an adverb-like word"), Some(Pos::Adverb));
        assert_eq!(
            Pos::read("a"),
            Some(Pos::Adjective),
            "whole, a is a pack's adj"
        );
        assert_eq!(Pos::read("a b"), None, "inside, a names nothing");
    }

    #[test]
    fn penn_tags_fold_to_classes() {
        assert_eq!(Pos::of_tag("VBD"), Some(Pos::Verb));
        assert_eq!(Pos::of_tag("MD"), Some(Pos::Verb));
        assert_eq!(Pos::of_tag("NNS"), Some(Pos::Noun));
        assert_eq!(Pos::of_tag("NNP"), Some(Pos::Noun));
        assert_eq!(Pos::of_tag("JJR"), Some(Pos::Adjective));
        assert_eq!(Pos::of_tag("RP"), Some(Pos::Adverb));
        assert_eq!(Pos::of_tag("IN"), Some(Pos::Preposition));
        assert_eq!(Pos::of_tag("DT"), Some(Pos::Determiner));
        assert_eq!(Pos::of_tag("PRP$"), Some(Pos::Pronoun));
        assert_eq!(Pos::of_tag("FW"), None);
        assert_eq!(Pos::of_tag("é"), None);
    }

    #[test]
    fn a_row_spells_its_classes_and_keeps_what_names_none() {
        assert_eq!(Pos::spell("v,n"), "verb · noun");
        assert_eq!(Pos::spell("vt; vi"), "verb", "said once");
        assert_eq!(Pos::spell("Adjective."), "adjective");
        assert_eq!(Pos::spell(" prefix "), "prefix");
        assert_eq!(Pos::spell(""), "");
    }

    #[test]
    fn every_class_reads_back_from_its_name() {
        for class in Pos::ALL {
            assert_eq!(Pos::parse(class.name()), Some(class));
        }
    }

    #[test]
    fn the_tagger_leads_then_the_ai_then_the_dictionary() {
        let all = Classes {
            tagger: Some(Pos::Verb),
            ai: Some(Pos::Adjective),
            dictionary: Some(Pos::Noun),
        };
        assert_eq!(all.class(), Some(Pos::Verb));
        let no_tagger = Classes {
            tagger: None,
            ..all
        };
        assert_eq!(no_tagger.class(), Some(Pos::Adjective));
        let only_dictionary = Classes {
            dictionary: Some(Pos::Noun),
            ..Classes::default()
        };
        assert_eq!(only_dictionary.class(), Some(Pos::Noun));
        assert_eq!(Classes::default().class(), None, "none of the three");
    }
}
