//! Parts of speech: a pack's tags folded to the kinds our tagger speaks.

/// A canonical word class. The pack's own spelling stays on the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Verb,
    Aux,
    Cop,
    Noun,
    Name,
    Counter,
    Adjective,
    Adverb,
    Pronoun,
    Possessive,
    Determiner,
    Article,
    Preposition,
    Postposition,
    Conjunction,
    Interjection,
    Number,
    Particle,
    Symbol,
    Abbreviation,
    Suffix,
    Prefix,
    Phrase,
    Character,
    Other,
}

/// The kinds the tagger can name: the vocabulary both sides agree on.
pub const NLP_KINDS: [&str; 11] = [
    "verb",
    "noun",
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

impl Kind {
    /// The spelling the store, the wire and the card use.
    pub fn as_str(&self) -> &'static str {
        match self {
            Kind::Verb => "verb",
            Kind::Aux => "aux",
            Kind::Cop => "cop",
            Kind::Noun => "noun",
            Kind::Name => "name",
            Kind::Counter => "counter",
            Kind::Adjective => "adjective",
            Kind::Adverb => "adverb",
            Kind::Pronoun => "pronoun",
            Kind::Possessive => "possessive",
            Kind::Determiner => "determiner",
            Kind::Article => "article",
            Kind::Preposition => "preposition",
            Kind::Postposition => "postposition",
            Kind::Conjunction => "conjunction",
            Kind::Interjection => "interjection",
            Kind::Number => "number",
            Kind::Particle => "particle",
            Kind::Symbol => "symbol",
            Kind::Abbreviation => "abbreviation",
            Kind::Suffix => "suffix",
            Kind::Prefix => "prefix",
            Kind::Phrase => "phrase",
            Kind::Character => "character",
            Kind::Other => "other",
        }
    }
}

/// Every spelling a pack carries or plausibly will.
const TAGS: &[(&str, Kind)] = &[
    ("v", Kind::Verb),
    ("verb", Kind::Verb),
    ("vs", Kind::Verb),
    ("vs-i", Kind::Verb),
    ("vs-s", Kind::Verb),
    ("vt", Kind::Verb),
    ("vi", Kind::Verb),
    ("phrasal verb", Kind::Verb),
    ("infinitive", Kind::Verb),
    ("aux", Kind::Aux),
    ("auxiliary", Kind::Aux),
    ("auxiliary verb", Kind::Aux),
    ("modal", Kind::Aux),
    ("modal verb", Kind::Aux),
    ("cop", Kind::Cop),
    ("copula", Kind::Cop),
    ("n", Kind::Noun),
    ("noun", Kind::Noun),
    ("ns", Kind::Noun),
    ("common noun", Kind::Noun),
    ("n-adv", Kind::Noun),
    ("n-pr", Kind::Noun),
    ("n-t", Kind::Noun),
    ("n-suf", Kind::Noun),
    ("n-pref", Kind::Noun),
    ("n-adj", Kind::Noun),
    ("name", Kind::Name),
    ("proper noun", Kind::Name),
    ("ctr", Kind::Counter),
    ("counter", Kind::Counter),
    ("classifier", Kind::Counter),
    ("adj", Kind::Adjective),
    ("adjective", Kind::Adjective),
    ("adjs", Kind::Adjective),
    ("adnominal", Kind::Adjective),
    ("adj-na", Kind::Adjective),
    ("adj-no", Kind::Adjective),
    ("adj-i", Kind::Adjective),
    ("adj-ix", Kind::Adjective),
    ("adv", Kind::Adverb),
    ("adverb", Kind::Adverb),
    ("advs", Kind::Adverb),
    ("advpart", Kind::Adverb),
    ("adv-to", Kind::Adverb),
    ("pron", Kind::Pronoun),
    ("pronoun", Kind::Pronoun),
    ("pers", Kind::Pronoun),
    ("indefpron", Kind::Pronoun),
    ("pn", Kind::Pronoun),
    ("possess", Kind::Possessive),
    ("possessive", Kind::Possessive),
    ("det", Kind::Determiner),
    ("determiner", Kind::Determiner),
    ("indefdet", Kind::Determiner),
    ("article", Kind::Article),
    ("indefart", Kind::Article),
    ("prep", Kind::Preposition),
    ("preposition", Kind::Preposition),
    ("postp", Kind::Postposition),
    ("postposition", Kind::Postposition),
    ("conj", Kind::Conjunction),
    ("conjunction", Kind::Conjunction),
    ("intj", Kind::Interjection),
    ("interj", Kind::Interjection),
    ("interjection", Kind::Interjection),
    ("int", Kind::Interjection),
    ("num", Kind::Number),
    ("number", Kind::Number),
    ("numeral", Kind::Number),
    ("particle", Kind::Particle),
    ("prt", Kind::Particle),
    ("symb", Kind::Symbol),
    ("symbol", Kind::Symbol),
    ("punct", Kind::Symbol),
    ("punctuation", Kind::Symbol),
    ("abbr", Kind::Abbreviation),
    ("abbreviation", Kind::Abbreviation),
    ("suff", Kind::Suffix),
    ("suffix", Kind::Suffix),
    ("pref", Kind::Prefix),
    ("prefix", Kind::Prefix),
    ("phrase", Kind::Phrase),
    ("prep_phrase", Kind::Phrase),
    ("proverb", Kind::Phrase),
    ("contraction", Kind::Phrase),
    ("idiom", Kind::Phrase),
    ("exp", Kind::Phrase),
    ("expression", Kind::Phrase),
    ("character", Kind::Character),
    ("syllabogram", Kind::Character),
    ("syllable", Kind::Character),
    ("combining_form", Kind::Character),
    ("radical", Kind::Character),
    ("root", Kind::Character),
    ("other", Kind::Other),
    ("unc", Kind::Other),
    ("unknown", Kind::Other),
];

/// The table's answer for one tag already lowercased.
fn exact(tag: &str) -> Option<Kind> {
    TAGS.iter()
        .find_map(|(spelling, kind)| (*spelling == tag).then_some(*kind))
}

/// Fold one pack tag, however it is spelled, to a canonical kind.
pub fn kind_of(tag: &str) -> Kind {
    let tag = tag.trim().to_lowercase();
    if tag.is_empty() {
        return Kind::Other;
    }
    if let Some(kind) = exact(&tag) {
        return kind;
    }
    // A tag nobody wrote down folds by shape, never by guesswork.
    if tag.starts_with("adv") || tag.ends_with("adv") {
        return Kind::Adverb;
    }
    if tag.starts_with("adj") || tag.ends_with("adj") {
        return Kind::Adjective;
    }
    if tag.starts_with("int") {
        return Kind::Interjection;
    }
    if tag.ends_with("pron") {
        return Kind::Pronoun;
    }
    if tag.ends_with("det") {
        return Kind::Determiner;
    }
    if tag.ends_with("art") {
        return Kind::Article;
    }
    if tag.ends_with("suff") {
        return Kind::Suffix;
    }
    if tag.ends_with("pref") {
        return Kind::Prefix;
    }
    if tag.ends_with("symb") {
        return Kind::Symbol;
    }
    if tag.ends_with("abbr") {
        return Kind::Abbreviation;
    }
    Kind::Other
}

/// Split a `pos` field into its tags; a row may carry several at once.
pub fn split_tags(pos: &str) -> Vec<&str> {
    let pos = pos.trim();
    if pos.is_empty() {
        return Vec::new();
    }
    // A whole string the table knows is one tag, not two words.
    if exact(&pos.to_lowercase()).is_some() {
        return vec![pos];
    }
    let mut tags: Vec<&str> = Vec::new();
    for piece in pos.split(|c: char| c.is_whitespace() || ",;|/".contains(c)) {
        let piece = piece.trim();
        if piece.is_empty() || tags.contains(&piece) {
            continue;
        }
        tags.push(piece);
    }
    tags
}

/// Every kind a `pos` field names, deduped in the field's own order.
pub fn kinds_of(pos: &str) -> Vec<Kind> {
    let mut kinds: Vec<Kind> = Vec::new();
    for tag in split_tags(pos) {
        let kind = kind_of(tag);
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    if kinds.is_empty() {
        kinds.push(Kind::Other);
    }
    kinds
}

/// The kinds of a `pos` field as the store writes them, space joined.
pub fn kinds_field(pos: &str) -> String {
    let mut out = String::new();
    for kind in kinds_of(pos) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(kind.as_str());
    }
    out
}

/// Fold the tagger's verdict to an agreement kind: a Penn tag,
/// or one of `NLP_KINDS`.
pub fn kind_of_nlp(value: &str) -> &'static str {
    // Mirrors cefr-rs's `kind_of`; the wasm side cannot depend on it.
    let lowered = value.trim().to_lowercase();
    if let Some(kind) = NLP_KINDS.iter().copied().find(|kind| *kind == lowered) {
        return kind;
    }
    let trimmed = value.trim();
    // LanguageTool appends suffixes (`NN:U`, `IN/that`); strip them.
    let tag = match trimmed.find([':', '/']) {
        Some(end) => &trimmed[..end],
        None => trimmed,
    };
    if tag.starts_with("VB") {
        "verb"
    } else if tag.starts_with("NN") || tag.starts_with("NP") {
        "noun"
    } else if tag.starts_with("JJ") {
        "adjective"
    } else if tag.starts_with("RB") {
        "adverb"
    } else if tag == "PRP" || tag.starts_with("WP") {
        "pronoun"
    } else if tag == "IN" || tag == "TO" {
        "preposition"
    } else if tag == "CC" {
        "conjunction"
    } else if tag == "CD" {
        "number"
    } else if tag == "MD" {
        "modal verb"
    } else if tag == "DT" || tag == "PDT" || tag == "WDT" {
        "determiner"
    } else {
        "other"
    }
}

/// Whether the tagger's verdict agrees with a row's kind.
/// Absent agreement leaves the row weak.
pub fn agrees(nlp: &str, kind: Kind) -> bool {
    match kind_of_nlp(nlp) {
        "verb" | "modal verb" => matches!(kind, Kind::Verb | Kind::Aux | Kind::Cop),
        "noun" => matches!(kind, Kind::Noun | Kind::Name | Kind::Counter),
        "adjective" => kind == Kind::Adjective,
        "adverb" => kind == Kind::Adverb,
        "pronoun" => matches!(kind, Kind::Pronoun | Kind::Possessive),
        "determiner" => matches!(kind, Kind::Determiner | Kind::Article | Kind::Possessive),
        "preposition" => matches!(kind, Kind::Preposition | Kind::Postposition),
        "conjunction" => kind == Kind::Conjunction,
        "number" => kind == Kind::Number,
        _ => false,
    }
}

/// How well one row answers the word the reader hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// A tag of the row agrees with the tagger's verdict.
    Strong,
    /// The word matched; the verdict was absent or agreed with nothing.
    Weak,
}

/// The rank a row's `pos` field earns against the tagger's verdict.
pub fn rank(pos: &str, nlp: Option<&str>) -> Match {
    let Some(nlp) = nlp else {
        return Match::Weak;
    };
    if split_tags(pos).iter().any(|tag| agrees(nlp, kind_of(tag))) {
        return Match::Strong;
    }
    Match::Weak
}

/// The tag to show: the one that agreed, else the row's first tag.
pub fn display_tag<'a>(pos: &'a str, nlp: Option<&str>) -> &'a str {
    let tags = split_tags(pos);
    if let Some(nlp) = nlp {
        for tag in &tags {
            if agrees(nlp, kind_of(tag)) {
                return tag;
            }
        }
    }
    tags.first().copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_apps_packs_fold_tag_by_tag() {
        // mcfnlp-en-my, whose short forms are the reason this table exists.
        assert_eq!(kind_of("n"), Kind::Noun);
        assert_eq!(kind_of("v"), Kind::Verb);
        assert_eq!(kind_of("adj"), Kind::Adjective);
        assert_eq!(kind_of("adv"), Kind::Adverb);
        assert_eq!(kind_of("prep"), Kind::Preposition);
        assert_eq!(kind_of("pron"), Kind::Pronoun);
        assert_eq!(kind_of("interj"), Kind::Interjection);
        assert_eq!(kind_of("det"), Kind::Determiner);
        assert_eq!(kind_of("advpart"), Kind::Adverb);
        assert_eq!(kind_of("conj"), Kind::Conjunction);
        assert_eq!(kind_of("indefpron"), Kind::Pronoun);
        assert_eq!(kind_of("adjs"), Kind::Adjective);
        assert_eq!(kind_of("ns"), Kind::Noun);
        assert_eq!(kind_of("indefdet"), Kind::Determiner);
        assert_eq!(kind_of("indefadv"), Kind::Adverb);
        assert_eq!(kind_of("suff"), Kind::Suffix);
        assert_eq!(kind_of("symb"), Kind::Symbol);
        assert_eq!(kind_of("pers"), Kind::Pronoun);
        assert_eq!(kind_of("pref"), Kind::Prefix);
        assert_eq!(kind_of("abbr"), Kind::Abbreviation);
        assert_eq!(kind_of("possess"), Kind::Possessive);
        assert_eq!(kind_of("vs"), Kind::Verb);
        assert_eq!(kind_of("advs"), Kind::Adverb);
        assert_eq!(kind_of("indefart"), Kind::Article);
        // jmdict-en-jp, the long broad categories.
        assert_eq!(kind_of("noun"), Kind::Noun);
        assert_eq!(kind_of("phrase"), Kind::Phrase);
        assert_eq!(kind_of("suffix"), Kind::Suffix);
        assert_eq!(kind_of("particle"), Kind::Particle);
        assert_eq!(kind_of("prefix"), Kind::Prefix);
        assert_eq!(kind_of("aux"), Kind::Aux);
        assert_eq!(kind_of("counter"), Kind::Counter);
        assert_eq!(kind_of("num"), Kind::Number);
        assert_eq!(kind_of("cop"), Kind::Cop);
        // wiktionary-en-fr.
        assert_eq!(kind_of("name"), Kind::Name);
        assert_eq!(kind_of("prep_phrase"), Kind::Phrase);
        assert_eq!(kind_of("proverb"), Kind::Phrase);
        assert_eq!(kind_of("contraction"), Kind::Phrase);
        assert_eq!(kind_of("postp"), Kind::Postposition);
        assert_eq!(kind_of("symbol"), Kind::Symbol);
    }

    #[test]
    fn an_unwritten_tag_folds_by_shape_and_then_gives_up() {
        assert_eq!(kind_of("adverbial"), Kind::Adverb);
        assert_eq!(kind_of("adjectival"), Kind::Adjective);
        assert_eq!(kind_of("interrogative"), Kind::Interjection);
        assert_eq!(kind_of("demonstrative_pron"), Kind::Pronoun);
        assert_eq!(kind_of("quantifier_det"), Kind::Determiner);
        assert_eq!(kind_of("definite_art"), Kind::Article);
        assert_eq!(kind_of("circumfix_suff"), Kind::Suffix);
        assert_eq!(kind_of("circumfix_pref"), Kind::Prefix);
        assert_eq!(kind_of("logogram_symb"), Kind::Symbol);
        assert_eq!(kind_of("initialism_abbr"), Kind::Abbreviation);
        // Nothing left to go on: the row keeps its raw string instead.
        assert_eq!(kind_of("wibble"), Kind::Other);
        assert_eq!(kind_of(""), Kind::Other);
        assert_eq!(kind_of("   "), Kind::Other);
        // Case is not a distinction the packs make.
        assert_eq!(kind_of(" NOUN "), Kind::Noun);
        assert_eq!(kind_of("V"), Kind::Verb);
    }

    #[test]
    fn a_row_may_be_several_parts_of_speech_at_once() {
        assert_eq!(kinds_of("v, n"), vec![Kind::Verb, Kind::Noun]);
        assert_eq!(kinds_of("n; adj"), vec![Kind::Noun, Kind::Adjective]);
        assert_eq!(kinds_of("verb/noun"), vec![Kind::Verb, Kind::Noun]);
        assert_eq!(kinds_of("n v"), vec![Kind::Noun, Kind::Verb]);
        assert_eq!(
            kinds_of("verb | adj | verb"),
            vec![Kind::Verb, Kind::Adjective]
        );
        assert_eq!(split_tags("v, n"), vec!["v", "n"]);
        assert_eq!(split_tags("noun"), vec!["noun"]);
        assert_eq!(split_tags(""), Vec::<&str>::new());
        // A two-word tag is one tag, not two.
        assert_eq!(split_tags("modal verb"), vec!["modal verb"]);
        assert_eq!(kinds_of("modal verb"), vec![Kind::Aux]);
        assert_eq!(split_tags("proper noun"), vec!["proper noun"]);
        assert_eq!(kinds_of("proper noun"), vec![Kind::Name]);
    }

    #[test]
    fn the_field_the_store_writes_is_canonical() {
        assert_eq!(kinds_field("v, n"), "verb noun");
        assert_eq!(kinds_field("adj"), "adjective");
        assert_eq!(kinds_field("noun | vs"), "noun verb");
        // An empty field is still findable, as a row of no verdict.
        assert_eq!(kinds_field(" "), "other");
        assert_eq!(kinds_field("wibble"), "other");
    }

    #[test]
    fn the_taggers_verdict_reads_as_a_tag_or_as_a_kind() {
        assert_eq!(kind_of_nlp("NNS"), "noun");
        assert_eq!(kind_of_nlp("NNP"), "noun");
        assert_eq!(kind_of_nlp("VBG"), "verb");
        assert_eq!(kind_of_nlp("MD"), "modal verb");
        assert_eq!(kind_of_nlp("DT"), "determiner");
        assert_eq!(kind_of_nlp("IN"), "preposition");
        assert_eq!(kind_of_nlp("CD"), "number");
        assert_eq!(kind_of_nlp("MODAL VERB"), "modal verb");
        assert_eq!(kind_of_nlp(" Noun "), "noun");
        assert_eq!(kind_of_nlp("XX"), "other");
        assert_eq!(kind_of_nlp(""), "other");
        // The suffix LanguageTool appends is not part of the tag.
        assert_eq!(kind_of_nlp("NN:U"), "noun");
        assert_eq!(kind_of_nlp("IN/that"), "preposition");
    }

    #[test]
    fn agreement_is_a_set_and_never_an_exact_hit() {
        assert!(agrees("NNS", Kind::Noun));
        assert!(agrees("noun", Kind::Name));
        assert!(agrees("NNS", Kind::Counter));
        assert!(agrees("VBG", Kind::Aux));
        assert!(agrees("MD", Kind::Verb));
        assert!(agrees("MD", Kind::Cop));
        assert!(agrees("PRP", Kind::Possessive));
        assert!(agrees("DT", Kind::Article));
        assert!(agrees("IN", Kind::Postposition));
        assert!(agrees("JJ", Kind::Adjective));
        assert!(agrees("CD", Kind::Number));
        assert!(!agrees("JJ", Kind::Counter));
        assert!(!agrees("NNS", Kind::Verb));
        // No verdict agrees with nothing: the row is shown, ranked weak.
        assert!(!agrees("other", Kind::Noun));
        assert!(!agrees("", Kind::Verb));
    }

    #[test]
    fn a_row_ranks_strong_only_when_a_tag_agrees() {
        assert_eq!(rank("n", Some("NNS")), Match::Strong);
        assert_eq!(rank("v, n", Some("MD")), Match::Strong);
        assert_eq!(rank("noun | vs", Some("VBZ")), Match::Strong);
        assert_eq!(rank("v", Some("NNS")), Match::Weak);
        assert_eq!(rank("n", None), Match::Weak);
        assert_eq!(rank("", Some("NNS")), Match::Weak);
        // Strong sorts before weak, so one sort orders the card.
        assert!(Match::Strong < Match::Weak);
    }

    #[test]
    fn the_card_shows_the_tag_that_answered() {
        assert_eq!(display_tag("v, n", Some("NNS")), "n");
        assert_eq!(display_tag("v, n", Some("VBZ")), "v");
        assert_eq!(display_tag("noun | vs", Some("VBD")), "vs");
        // No verdict, no agreement: the row speaks for itself.
        assert_eq!(display_tag("v, n", None), "v");
        assert_eq!(display_tag("v, n", Some("JJ")), "v");
        assert_eq!(display_tag("", None), "");
        assert_eq!(display_tag("  ", Some("NNS")), "");
    }
}
