//! A card's sound line, in the accent the voice and settings pick.

use mareader_g2p::Dialect;
pub use mareader_g2p::notation::Beat;
use mareader_settings::{Accent, Notation, PronunciationSettings};

/// Longer selections get no pronunciation.
const MOST_WORDS: usize = 3;
const MOST_CHARS: usize = 40;

/// The tag a line wears when both accents show.
pub(super) fn tag(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::American => "US",
        Dialect::British => "UK",
    }
}

/// The tag a stand-in accent wears, as `≈ UK`.
pub(super) fn near_tag(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::American => "≈ US",
        Dialect::British => "≈ UK",
    }
}

/// The accent's name, as people say it.
pub fn named(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::American => "American",
        Dialect::British => "British",
    }
}

/// One accent on a card: its tag, then each word's beats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// "US", "UK" or "≈ UK"; empty when the voice's own accent shows.
    pub tag: &'static str,
    pub words: Vec<Vec<Beat>>,
    /// IPA sits between slashes.
    pub slashed: bool,
    /// Why a stand-in accent shows, told on hover.
    pub why: Option<String>,
}

/// The voice that says English words on cards, as `Emma`, `en-GB`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnglishVoice {
    pub name: String,
    pub tag: String,
}

/// Which English cards write, from the voice's language tag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spoken {
    /// American or British: cards write just that.
    Exactly(Dialect),
    /// Another English, as `Australian`: the nearer of the two.
    Nearest {
        dialect: Dialect,
        english: &'static str,
    },
    /// No English voice, or one not saying which: both show.
    Unsure,
}

/// Englishes without a lexicon of their own, and their nearer accent.
const ENGLISHES: [(&str, &str, Dialect); 15] = [
    ("AU", "Australian", Dialect::British),
    ("NZ", "New Zealand", Dialect::British),
    ("IE", "Irish", Dialect::British),
    ("ZA", "South African", Dialect::British),
    ("IN", "Indian", Dialect::British),
    ("SG", "Singaporean", Dialect::British),
    ("HK", "Hong Kong", Dialect::British),
    ("MY", "Malaysian", Dialect::British),
    ("NG", "Nigerian", Dialect::British),
    ("KE", "Kenyan", Dialect::British),
    ("TZ", "Tanzanian", Dialect::British),
    ("PK", "Pakistani", Dialect::British),
    ("JM", "Jamaican", Dialect::British),
    ("CA", "Canadian", Dialect::American),
    ("PH", "Philippine", Dialect::American),
];

/// What a voice tagged `tag`, as `en-AU`, means for the cards.
pub fn spoken(tag: Option<&str>) -> Spoken {
    let Some(tag) = tag else {
        return Spoken::Unsure;
    };
    let mut parts = tag.split(['-', '_']);
    if !parts.next().is_some_and(|p| p.eq_ignore_ascii_case("en")) {
        return Spoken::Unsure;
    }
    // Past a script, as `Latn`, to the region: two letters.
    let region = parts.find(|p| p.len() != 4).filter(|p| p.len() == 2);
    let Some(region) = region.map(str::to_ascii_uppercase) else {
        return Spoken::Unsure;
    };
    match region.as_str() {
        "US" => Spoken::Exactly(Dialect::American),
        "GB" | "UK" => Spoken::Exactly(Dialect::British),
        _ => match ENGLISHES.iter().find(|(r, ..)| *r == region) {
            Some(&(_, english, dialect)) => Spoken::Nearest { dialect, english },
            None => Spoken::Unsure,
        },
    }
}

/// The accents a card writes, as `settings` and the voice ask.
pub fn shown(settings: &PronunciationSettings, spoken: Spoken) -> &'static [Dialect] {
    const BOTH: &[Dialect] = &[Dialect::American, Dialect::British];
    let one = |d: Dialect| -> &'static [Dialect] {
        match d {
            Dialect::American => &[Dialect::American],
            Dialect::British => &[Dialect::British],
        }
    };
    if !settings.follow_voice {
        return match settings.accent {
            Accent::Both => BOTH,
            Accent::American => one(Dialect::American),
            Accent::British => one(Dialect::British),
        };
    }
    match spoken {
        Spoken::Exactly(d) | Spoken::Nearest { dialect: d, .. } => one(d),
        Spoken::Unsure => BOTH,
    }
}

/// The accent picker's value while the voice decides.
pub fn accent(settings: &PronunciationSettings, spoken: Spoken) -> Accent {
    match shown(settings, spoken) {
        [Dialect::American] => Accent::American,
        [Dialect::British] => Accent::British,
        _ => Accent::Both,
    }
}

/// What a card hears: the word read aloud, and how card words sound.
#[derive(Clone, Copy, Default)]
pub struct Heard<'a> {
    pub saying: Option<&'a str>,
    pub sounds: &'a [(String, Vec<Line>)],
}

impl Heard<'_> {
    pub fn saying(&self, word: &str) -> bool {
        self.saying == Some(word)
    }

    pub fn lines(&self, word: &str) -> &[Line] {
        let found = self.sounds.iter().find(|(w, _)| w == word);
        found.map_or(&[], |(_, lines)| lines.as_slice())
    }
}

/// Why `dialect` stands in for the voice's `english`.
pub fn stand_in(english: &str, dialect: Dialect) -> String {
    let dialect = named(dialect);
    format!(
        "Your voice speaks {english} English. Cards know only American \
         and British, so this is {dialect}, the nearer."
    )
}

/// Whether `word` is English short enough to sound out.
pub(super) fn speakable(word: &str) -> bool {
    let fits = |c: char| c.is_ascii_alphabetic() || matches!(c, '\'' | '-' | ' ');
    !word.is_empty()
        && word.len() <= MOST_CHARS
        && word.split_whitespace().count() <= MOST_WORDS
        && word.chars().any(|c| c.is_ascii_alphabetic())
        && word.chars().all(fits)
}

/// The phonemes, or None where a word wasn't known.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub(super) fn understood(phonemes: &str) -> Option<String> {
    let said = phonemes.trim();
    let known = !said.is_empty() && !said.contains(mareader_g2p::UNKNOWN);
    known.then(|| said.to_owned())
}

/// `phonemes` written as `notation` asks.
pub fn format(phonemes: &str, dialect: Dialect, notation: Notation) -> Line {
    use mareader_g2p::notation;
    let letters = |w: &str| w.chars().filter(|c| c.is_alphabetic()).collect();
    let spoken = phonemes.split_whitespace().map(letters);
    let words = spoken.filter(|w: &String| !w.is_empty()).map(|w| {
        let one = |text| {
            vec![Beat {
                text,
                stressed: false,
            }]
        };
        match notation {
            Notation::Ipa => one(notation::ipa(&w, dialect)),
            Notation::Raw => one(notation::raw(&w)),
            Notation::Easy => notation::easy(&w, dialect),
        }
    });
    Line {
        tag: "",
        words: words.collect(),
        slashed: notation != Notation::Easy,
        why: None,
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        let word = |w: &Vec<Beat>| {
            let beat = |b: &Beat| match b.stressed {
                true => b.text.to_uppercase(),
                false => b.text.clone(),
            };
            w.iter().map(beat).collect::<Vec<_>>().join("·")
        };
        line.words.iter().map(word).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn each_notation_writes_the_phonemes_its_way() {
        let us = |n| text(&format("jˈɑt", Dialect::American, n));
        assert_eq!(us(Notation::Ipa), "jɑːt");
        assert_eq!(us(Notation::Easy), "yaat");
        assert_eq!(us(Notation::Raw), "jˈɑt");
        let two = format("ɡˈʊd mˈɔɹnɪŋ,", Dialect::American, Notation::Easy);
        assert_eq!(text(&two), "gud MOR·ning");
        assert!(!two.slashed);
        assert!(format("jˈɒt", Dialect::British, Notation::Ipa).slashed);
    }

    #[test]
    fn only_short_english_is_sounded_out_and_unknowns_show_nothing() {
        assert!(speakable("yacht"));
        assert!(speakable("give up"));
        assert!(speakable("don't"));
        assert!(!speakable("မင်္ဂလာ"));
        assert!(!speakable("1984"));
        assert!(!speakable("one two three four"));
        assert!(!speakable(""));
        assert_eq!(understood(" jˈɑt "), Some("jˈɑt".to_owned()));
        assert_eq!(understood("ʤ❓"), None);
        assert_eq!(understood(""), None);
    }

    #[test]
    fn cards_find_their_words_lines() {
        let line = format("jˈɑt", Dialect::American, Notation::Easy);
        let sounds = [("yacht".to_owned(), vec![line.clone()])];
        let heard = Heard {
            saying: Some("yacht"),
            sounds: &sounds,
        };
        assert!(heard.saying("yacht"));
        assert_eq!(heard.lines("yacht"), [line]);
        assert!(heard.lines("boat").is_empty());
    }

    #[test]
    fn a_voice_s_tag_picks_its_english_or_the_nearest() {
        use Dialect::{American, British};
        let tag = |t| spoken(Some(t));
        assert_eq!(tag("en-US"), Spoken::Exactly(American));
        assert_eq!(tag("en_GB"), Spoken::Exactly(British));
        assert_eq!(tag("en-Latn-GB"), Spoken::Exactly(British));
        assert_eq!(tag("en-GB-scotland"), Spoken::Exactly(British));
        let au = Spoken::Nearest {
            dialect: British,
            english: "Australian",
        };
        assert_eq!(tag("en-au"), au);
        let ca = Spoken::Nearest {
            dialect: American,
            english: "Canadian",
        };
        assert_eq!(tag("en-CA"), ca);
        for unsure in ["en", "en-001", "en-XX", "fr-FR", "es"] {
            assert_eq!(tag(unsure), Spoken::Unsure, "{unsure}");
        }
        assert_eq!(spoken(None), Spoken::Unsure);
    }

    #[test]
    fn the_voice_decides_unless_told_not_to() {
        use Dialect::{American, British};
        let mut s = PronunciationSettings::default();
        assert_eq!(shown(&s, Spoken::Exactly(British)), [British]);
        let au = spoken(Some("en-AU"));
        assert_eq!(shown(&s, au), [British]);
        assert_eq!(shown(&s, Spoken::Unsure), [American, British]);
        assert_eq!(accent(&s, Spoken::Exactly(American)), Accent::American);
        s.follow_voice = false;
        s.accent = Accent::American;
        assert_eq!(shown(&s, Spoken::Exactly(British)), [American]);
        let why = stand_in("Australian", British);
        assert!(why.contains("Australian English") && why.contains("British"));
    }
}
