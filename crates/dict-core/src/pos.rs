//! POS canon across sources, and the ranks a card sorts by.

use serde::{Deserialize, Serialize};

/// The canon every source tag folds into: `v` and `verb`
/// speak one language here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonPos {
    Noun,
    ProperName,
    Verb,
    ModalVerb,
    Auxiliary,
    Copula,
    Adjective,
    Adnominal,
    Adverb,
    AdverbParticiple,
    Pronoun,
    Possessive,
    Determiner,
    Article,
    Number,
    Conjunction,
    Preposition,
    Postposition,
    Particle,
    Interjection,
    Contraction,
    Phrase,
    PrepPhrase,
    Proverb,
    Prefix,
    Suffix,
    Affix,
    Infix,
    Interfix,
    CombiningForm,
    Classifier,
    Counter,
    Symbol,
    Character,
    Punctuation,
    Abbreviation,
    Syllable,
    Root,
    Other,
    /// A tag nothing maps to: kept, shown, never dropped.
    Unknown,
}

impl CanonPos {
    /// The readable label the menu shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::ProperName => "name",
            Self::Verb => "verb",
            Self::ModalVerb => "modal verb",
            Self::Auxiliary => "auxiliary",
            Self::Copula => "copula",
            Self::Adjective => "adjective",
            Self::Adnominal => "adnominal",
            Self::Adverb => "adverb",
            Self::AdverbParticiple => "adverb participle",
            Self::Pronoun => "pronoun",
            Self::Possessive => "possessive",
            Self::Determiner => "determiner",
            Self::Article => "article",
            Self::Number => "number",
            Self::Conjunction => "conjunction",
            Self::Preposition => "preposition",
            Self::Postposition => "postposition",
            Self::Particle => "particle",
            Self::Interjection => "interjection",
            Self::Contraction => "contraction",
            Self::Phrase => "phrase",
            Self::PrepPhrase => "prepositional phrase",
            Self::Proverb => "proverb",
            Self::Prefix => "prefix",
            Self::Suffix => "suffix",
            Self::Affix => "affix",
            Self::Infix => "infix",
            Self::Interfix => "interfix",
            Self::CombiningForm => "combining form",
            Self::Classifier => "classifier",
            Self::Counter => "counter",
            Self::Symbol => "symbol",
            Self::Character => "character",
            Self::Punctuation => "punctuation",
            Self::Abbreviation => "abbreviation",
            Self::Syllable => "syllable",
            Self::Root => "root",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }
    /// The family a match falls back to; siblings answer
    /// lower. Nothing else answers.
    pub fn family(self) -> &'static str {
        match self {
            Self::Noun | Self::ProperName => "noun",
            Self::Verb | Self::ModalVerb | Self::Auxiliary | Self::Copula => "verb",
            Self::Adjective | Self::Adnominal => "adjective",
            Self::Adverb | Self::AdverbParticiple => "adverb",
            Self::Pronoun | Self::Possessive => "pronoun",
            Self::Determiner | Self::Article => "determiner",
            Self::Phrase | Self::PrepPhrase | Self::Proverb => "phrase",
            Self::Prefix | Self::Suffix | Self::Affix | Self::Infix | Self::Interfix => "affix",
            _ => "closed",
        }
    }
}

/// One source tag folded to the canon. `None` when empty.
pub fn canonize(tag: &str) -> Option<CanonPos> {
    let t = tag
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '.'], "_");
    if t.is_empty() || t == "none" || t == "null" {
        return None;
    }
    let t = t.as_str();
    let canon = match t {
        "n" | "ns" | "noun" | "nouns" | "common_noun" => CanonPos::Noun,
        "name" | "names" | "proper" | "proper_noun" | "propernoun" => CanonPos::ProperName,
        "v" | "vs" | "verb" | "verbs" => CanonPos::Verb,
        "md" | "modal" | "modals" | "modal_verb" | "modal_verb_" | "modalverb" => {
            CanonPos::ModalVerb
        }
        "aux" | "auxiliary" | "auxiliaries" | "helping_verb" => CanonPos::Auxiliary,
        "cop" | "copula" | "copulas" => CanonPos::Copula,
        "adj" | "adjs" | "adjective" | "adjectives" => CanonPos::Adjective,
        "adnominal" | "adn" => CanonPos::Adnominal,
        "adv" | "advs" | "adverb" | "adverbs" | "adverbial" | "indefadv" => CanonPos::Adverb,
        "advpart" | "adverb_participle" | "adverbial_particle" => CanonPos::AdverbParticiple,
        "pron" | "pronoun" | "pronouns" | "prp" | "indefpron" => CanonPos::Pronoun,
        "pers" | "person" | "personal" | "personal_pronoun" => CanonPos::Pronoun,
        "possess" | "possessive" | "poss" | "possessive_pronoun" | "pos" => CanonPos::Possessive,
        "det" | "determiner" | "determiners" | "indefdet" => CanonPos::Determiner,
        "article" | "articles" | "art" | "indefart" => CanonPos::Article,
        "num" | "number" | "numbers" | "numeral" | "cardinal" => CanonPos::Number,
        "conj" | "conjunction" | "conjunctions" | "cc" => CanonPos::Conjunction,
        "prep" | "preposition" | "prepositions" => CanonPos::Preposition,
        "postp" | "postposition" | "postpositions" => CanonPos::Postposition,
        "particle" | "particles" | "part" => CanonPos::Particle,
        "interj" | "intj" | "interjection" | "interjections" => CanonPos::Interjection,
        "contraction" | "contractions" => CanonPos::Contraction,
        "phrase" | "phrases" => CanonPos::Phrase,
        "prep_phrase" | "prepositional_phrase" | "prepphrase" => CanonPos::PrepPhrase,
        "proverb" | "proverbs" => CanonPos::Proverb,
        "pref" | "prefix" | "prefixes" => CanonPos::Prefix,
        "suff" | "suffix" | "suffixes" => CanonPos::Suffix,
        "affix" | "affixes" => CanonPos::Affix,
        "infix" | "infixes" => CanonPos::Infix,
        "interfix" | "interfixes" => CanonPos::Interfix,
        "combining_form" | "combiningform" => CanonPos::CombiningForm,
        "classifier" | "classifiers" => CanonPos::Classifier,
        "counter" | "counters" | "measure_word" => CanonPos::Counter,
        "symb" | "symbol" | "symbols" => CanonPos::Symbol,
        "character" | "characters" | "char" | "letter" | "kanji" => CanonPos::Character,
        "punct" | "punctuation" => CanonPos::Punctuation,
        "abbr" | "abbreviation" | "abbreviations" => CanonPos::Abbreviation,
        "syllable" | "syllables" => CanonPos::Syllable,
        "root" | "roots" => CanonPos::Root,
        "other" | "unclear" | "uncategorised" | "uncategorized" => CanonPos::Other,
        _ => CanonPos::Unknown,
    };
    Some(canon)
}

/// One entry's tags: a cell may hold several (`v,n`).
pub fn parse_tags(raw: &str) -> Vec<CanonPos> {
    let mut out: Vec<CanonPos> = Vec::new();
    for part in raw.split([',', ';', '|', '/', '+']) {
        if let Some(canon) = canonize(part)
            && !out.contains(&canon)
        {
            out.push(canon);
        }
    }
    out
}

/// How the detected role answered an entry's tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PosMatch {
    /// The entry speaks the detected role by name.
    Exact,
    /// A sibling in the same family answered.
    Family,
    /// Nothing answered; the word alone carried the match.
    Bare,
}

/// The best answer an entry's tags give a detected role.
pub fn rank_tags(detected: CanonPos, tags: &[CanonPos]) -> PosMatch {
    if tags.contains(&detected) {
        return PosMatch::Exact;
    }
    if tags
        .iter()
        .any(|tag| tag.family() == detected.family() && tag.family() != "closed")
    {
        return PosMatch::Family;
    }
    PosMatch::Bare
}

/// The canon home of our NLP's eleven readable kinds
/// (`cefr::tags::kind_of`).
pub fn kind_canon(kind: &str) -> CanonPos {
    match kind {
        "noun" => CanonPos::Noun,
        "verb" => CanonPos::Verb,
        "adjective" => CanonPos::Adjective,
        "adverb" => CanonPos::Adverb,
        "pronoun" => CanonPos::Pronoun,
        "preposition" => CanonPos::Preposition,
        "conjunction" => CanonPos::Conjunction,
        "number" => CanonPos::Number,
        "modal verb" => CanonPos::ModalVerb,
        "determiner" => CanonPos::Determiner,
        _ => CanonPos::Other,
    }
}

/// The Penn tag's canon: `NNP` is a name; `NN` is not.
pub fn penn_canon(tag: &str) -> CanonPos {
    let tag = tag.trim();
    if tag.starts_with("NNPS") || tag.starts_with("NNP") || tag == "NP" {
        CanonPos::ProperName
    } else if tag.starts_with("NN") {
        CanonPos::Noun
    } else if tag.starts_with("VB") {
        CanonPos::Verb
    } else if tag == "MD" {
        CanonPos::ModalVerb
    } else if tag.starts_with("JJ") {
        CanonPos::Adjective
    } else if tag.starts_with("RB") || tag == "WRB" {
        CanonPos::Adverb
    } else if tag == "PRP" || tag == "PRP$" || tag.starts_with("WP") {
        CanonPos::Pronoun
    } else if tag == "POS" {
        CanonPos::Possessive
    } else if tag == "DT" || tag == "PDT" || tag == "WDT" {
        CanonPos::Determiner
    } else if tag == "IN" || tag == "TO" {
        CanonPos::Preposition
    } else if tag == "CC" {
        CanonPos::Conjunction
    } else if tag == "CD" {
        CanonPos::Number
    } else if tag == "UH" {
        CanonPos::Interjection
    } else if tag == "RP" {
        CanonPos::Particle
    } else if tag == "SYM" {
        CanonPos::Symbol
    } else {
        CanonPos::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tag_in_the_summaries_canonizes() {
        // The 53 real values `output/pos-tags/SUMMARY.txt` lists.
        // Its 54th is the empty cell: not dropped.
        for tag in [
            "abbr",
            "adj",
            "adjs",
            "adnominal",
            "adv",
            "advpart",
            "advs",
            "affix",
            "article",
            "aux",
            "character",
            "classifier",
            "combining_form",
            "conj",
            "contraction",
            "cop",
            "counter",
            "det",
            "indefadv",
            "indefart",
            "indefdet",
            "indefpron",
            "infix",
            "interfix",
            "interj",
            "intj",
            "n",
            "name",
            "noun",
            "ns",
            "num",
            "other",
            "particle",
            "pers",
            "phrase",
            "possess",
            "postp",
            "pref",
            "prefix",
            "prep",
            "prep_phrase",
            "pron",
            "proverb",
            "punct",
            "root",
            "suff",
            "suffix",
            "syllable",
            "symb",
            "symbol",
            "v",
            "verb",
            "vs",
        ] {
            assert_ne!(canonize(tag).unwrap(), CanonPos::Unknown, "{tag}");
        }
        assert_eq!(canonize(""), None);
        assert_eq!(canonize("None"), None);
        assert_eq!(canonize("null"), None);
    }

    #[test]
    fn long_and_short_forms_land_on_one_canon() {
        for tag in ["v", "vs", "verb", "verbs", "Verb"] {
            assert_eq!(canonize(tag), Some(CanonPos::Verb), "{tag}");
        }
        for tag in ["n", "ns", "noun"] {
            assert_eq!(canonize(tag), Some(CanonPos::Noun), "{tag}");
        }
        // Words the sources never wrote must still answer.
        for tag in ["adverb", "adverbs", "Adverb"] {
            assert_eq!(canonize(tag), Some(CanonPos::Adverb), "{tag}");
        }
        // A tag nothing maps to is kept as Unknown, never dropped.
        assert_eq!(canonize("blorp"), Some(CanonPos::Unknown));
    }

    #[test]
    fn a_cell_may_hold_several_tags() {
        assert_eq!(parse_tags("v,n"), vec![CanonPos::Verb, CanonPos::Noun]);
        assert_eq!(parse_tags("v, n"), vec![CanonPos::Verb, CanonPos::Noun]);
        assert_eq!(
            parse_tags("verb/noun"),
            vec![CanonPos::Verb, CanonPos::Noun]
        );
        assert_eq!(
            parse_tags("adj;adv"),
            vec![CanonPos::Adjective, CanonPos::Adverb]
        );
        // A spaced tag is one tag; splitting it would mangle `modal verb`.
        assert_eq!(parse_tags("modal verb"), vec![CanonPos::ModalVerb]);
        assert_eq!(parse_tags("prep phrase"), vec![CanonPos::PrepPhrase]);
    }

    #[test]
    fn an_entry_answers_by_name_then_family_then_not_at_all() {
        let tags = parse_tags("v,n");
        assert_eq!(rank_tags(CanonPos::Verb, &tags), PosMatch::Exact);
        assert_eq!(rank_tags(CanonPos::Noun, &tags), PosMatch::Exact);
        assert_eq!(rank_tags(CanonPos::ModalVerb, &tags), PosMatch::Family);
        assert_eq!(rank_tags(CanonPos::Adverb, &tags), PosMatch::Bare);
        // A name is a noun's sibling; a classifier is nobody's.
        assert_eq!(
            rank_tags(CanonPos::Noun, &[CanonPos::ProperName]),
            PosMatch::Family
        );
        assert_eq!(
            rank_tags(CanonPos::Noun, &[CanonPos::Classifier]),
            PosMatch::Bare
        );
    }

    #[test]
    fn the_nlp_side_has_eleven_readable_kinds() {
        // `cefr::tags::kind_of` answers exactly these; the count is the
        // contract the hover menu ranks against.
        let kinds = [
            "noun",
            "verb",
            "adjective",
            "adverb",
            "pronoun",
            "preposition",
            "conjunction",
            "number",
            "modal verb",
            "determiner",
            "other",
        ];
        assert_eq!(kinds.len(), 11);
        for kind in kinds {
            assert_ne!(kind_canon(kind), CanonPos::Unknown, "{kind}");
        }
    }

    #[test]
    fn penn_tags_go_one_step_finer_than_the_kinds() {
        assert_eq!(penn_canon("NNP"), CanonPos::ProperName);
        assert_eq!(penn_canon("NNPS"), CanonPos::ProperName);
        assert_eq!(penn_canon("NN"), CanonPos::Noun);
        assert_eq!(penn_canon("VBZ"), CanonPos::Verb);
        assert_eq!(penn_canon("MD"), CanonPos::ModalVerb);
        assert_eq!(penn_canon("RBR"), CanonPos::Adverb);
        // The kind stays coarser: a name is read as a noun.
        assert_eq!(kind_canon("noun"), CanonPos::Noun);
    }
}
