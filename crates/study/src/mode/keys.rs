//! A study word's key: its lemma and coarse word class (decision D4).
//!
//! One card stands for one key, however often or in whatever form the word
//! shows. The lemma comes from folding common endings against the words the
//! level data knows, so `acknowledged` joins `acknowledge` when it is known.

use serde::{Deserialize, Serialize};

/// The four word classes study mode keeps apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pos {
    Noun,
    Verb,
    Adjective,
    Adverb,
}

impl Pos {
    pub const ALL: [Self; 4] = [Self::Noun, Self::Verb, Self::Adjective, Self::Adverb];

    /// The class of a Penn tag such as `VBD`; none for a proper noun or other.
    pub fn of_tag(tag: &str) -> Option<Self> {
        let tag = tag.trim();
        if tag.starts_with("NNP") {
            return None;
        }
        match tag.get(..2)? {
            "NN" => Some(Self::Noun),
            "VB" => Some(Self::Verb),
            "JJ" => Some(Self::Adjective),
            "RB" => Some(Self::Adverb),
            _ => None,
        }
    }

    /// The class as stored and shown, as "verb".
    pub fn name(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adjective",
            Self::Adverb => "adverb",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name.trim())
    }
}

/// One card's identity: a lemma and, when known, its word class.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Key {
    pub lemma: String,
    pub pos: Option<Pos>,
}

impl Key {
    pub fn new(lemma: impl Into<String>, pos: Option<Pos>) -> Self {
        Self {
            lemma: lemma.into(),
            pos,
        }
    }

    /// As stored: `lemma|pos`, or the bare lemma when the class is unknown.
    pub fn id(&self) -> String {
        match self.pos {
            Some(pos) => format!("{}|{}", self.lemma, pos.name()),
            None => self.lemma.clone(),
        }
    }

    /// Reads [`Self::id`] back; none for an empty lemma or an unknown class.
    pub fn parse(id: &str) -> Option<Self> {
        let (lemma, pos) = match id.split_once('|') {
            Some((lemma, pos)) => (lemma, Some(Pos::parse(pos)?)),
            None => (id, None),
        };
        let lemma = lemma.trim();
        (!lemma.is_empty()).then(|| Self::new(lemma, pos))
    }
}

/// Endings tried in order: what is cut, and what is put back.
const ENDINGS: &[(&str, &str)] = &[
    ("iest", "y"),
    ("ies", "y"),
    ("ied", "y"),
    ("ier", "y"),
    ("ing", ""),
    ("ing", "e"),
    ("est", ""),
    ("est", "e"),
    ("ed", ""),
    ("ed", "e"),
    ("er", ""),
    ("er", "e"),
    ("es", ""),
    ("s", ""),
];

/// Words that only look inflected.
const WHOLE: &[&str] = &[
    "always", "news", "perhaps", "series", "species", "thus", "lens", "during",
];

/// The shortest stem an ending may leave.
const MIN_STEM: usize = 3;

/// The lemma of a normalized word: the first base `known` has, else itself.
pub fn lemma(word: &str, known: impl Fn(&str) -> bool) -> String {
    if WHOLE.contains(&word) {
        return word.to_owned();
    }
    bases(word)
        .into_iter()
        .find(|base| known(base.as_str()))
        .unwrap_or_else(|| word.to_owned())
}

/// Every base `word` may come from, most likely first.
pub fn bases(word: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (cut, put) in ENDINGS {
        let Some(stem) = word.strip_suffix(cut) else {
            continue;
        };
        if stem.chars().count() < MIN_STEM {
            continue;
        }
        let base = format!("{stem}{put}");
        if !out.contains(&base) {
            out.push(base);
        }
        // stopped, bigger: the doubled consonant goes too
        if put.is_empty() && doubled(stem) {
            let mut single = stem.to_owned();
            single.pop();
            if !out.contains(&single) {
                out.push(single);
            }
        }
    }
    out
}

fn doubled(stem: &str) -> bool {
    let mut end = stem.chars().rev();
    match (end.next(), end.next()) {
        (Some(a), Some(b)) => a == b && a.is_alphabetic() && !"aeiou".contains(a),
        _ => false,
    }
}

/// A capitalised word inside a sentence reads as a name and gets no card.
pub fn proper(word: &str, opens_sentence: bool) -> bool {
    !opens_sentence && word.chars().next().is_some_and(char::is_uppercase)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn known(words: &[&str]) -> impl Fn(&str) -> bool {
        let words: Vec<String> = words.iter().map(|w| (*w).to_owned()).collect();
        move |w: &str| words.iter().any(|k| k.as_str() == w)
    }

    #[test]
    fn penn_tags_fold_to_four_classes_and_names_to_none() {
        assert_eq!(Pos::of_tag("VBD"), Some(Pos::Verb));
        assert_eq!(Pos::of_tag("NNS"), Some(Pos::Noun));
        assert_eq!(Pos::of_tag("JJR"), Some(Pos::Adjective));
        assert_eq!(Pos::of_tag("RB"), Some(Pos::Adverb));
        assert_eq!(Pos::of_tag("NNP"), None);
        assert_eq!(Pos::of_tag("DT"), None);
        assert_eq!(Pos::of_tag("é"), None);
    }

    #[test]
    fn a_key_reads_back_from_its_id() {
        let key = Key::new("visit", Some(Pos::Noun));
        assert_eq!(key.id(), "visit|noun");
        assert_eq!(Key::parse(&key.id()), Some(key));
        assert_eq!(Key::parse("vex"), Some(Key::new("vex", None)));
        assert_eq!(Key::parse("vex|pronoun"), None);
        assert_eq!(Key::parse("|verb"), None);
    }

    #[test]
    fn endings_fold_to_a_known_base() {
        let k = known(&["acknowledge", "study", "stop", "big", "make", "box", "vex"]);
        assert_eq!(lemma("acknowledged", &k), "acknowledge");
        assert_eq!(lemma("studies", &k), "study");
        assert_eq!(lemma("stopped", &k), "stop");
        assert_eq!(lemma("bigger", &k), "big");
        assert_eq!(lemma("making", &k), "make");
        assert_eq!(lemma("boxes", &k), "box");
        assert_eq!(lemma("vexing", &k), "vex");
    }

    #[test]
    fn an_unknown_base_or_a_lookalike_keeps_the_word() {
        let k = known(&["new", "seri", "ru"]);
        assert_eq!(lemma("news", &k), "news");
        assert_eq!(lemma("series", &k), "series");
        assert_eq!(lemma("runs", &k), "runs");
        assert_eq!(lemma("solace", &k), "solace");
        assert!(bases("is").is_empty());
    }

    #[test]
    fn a_capital_inside_a_sentence_is_a_name() {
        assert!(proper("Bingley", false));
        assert!(!proper("Bingley", true));
        assert!(!proper("fortune", false));
        assert!(!proper("", false));
    }
}
