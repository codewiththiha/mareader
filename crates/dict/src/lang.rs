//! Languages by code, and a word's language read off its script.

/// The writing a character belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Script {
    Myanmar,
    Kana,
    Han,
    Hangul,
    Thai,
    Latin,
}

/// A language the app names: its BCP-47 tag, English name, the code packs
/// and the AI dictionary keep it under, and the scripts that write it.
///
/// The code is a pack's where one joins the language (`jp`, `zh`), else
/// the tag; both Chinese scripts share `zh`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub tag: &'static str,
    pub name: &'static str,
    pub code: &'static str,
    scripts: &'static [Script],
}

const fn one(
    tag: &'static str,
    name: &'static str,
    code: &'static str,
    scripts: &'static [Script],
) -> Language {
    Language {
        tag,
        name,
        code,
        scripts,
    }
}

use Script::{Han, Hangul, Kana, Latin, Myanmar, Thai};

/// Every language: those AI tasks write in, packs' among them; by name.
pub const LANGUAGES: [Language; 40] = [
    one("ar", "Arabic", "ar", &[]),
    one("bn", "Bengali", "bn", &[]),
    one("my", "Burmese", "my", &[Myanmar]),
    one("zh-Hans", "Chinese (Simplified)", "zh", &[Han]),
    one("zh-Hant", "Chinese (Traditional)", "zh", &[Han]),
    one("cs", "Czech", "cs", &[]),
    one("da", "Danish", "da", &[]),
    one("nl", "Dutch", "nl", &[]),
    one("en", "English", "en", &[Latin]),
    one("fil", "Filipino", "fil", &[]),
    one("fi", "Finnish", "fi", &[]),
    one("fr", "French", "fr", &[Latin]),
    one("de", "German", "de", &[Latin]),
    one("el", "Greek", "el", &[]),
    one("he", "Hebrew", "he", &[]),
    one("hi", "Hindi", "hi", &[]),
    one("hu", "Hungarian", "hu", &[]),
    one("id", "Indonesian", "id", &[]),
    one("it", "Italian", "it", &[]),
    one("ja", "Japanese", "jp", &[Kana, Han]),
    one("km", "Khmer", "km", &[]),
    one("ko", "Korean", "ko", &[Hangul, Han]),
    one("lo", "Lao", "lo", &[]),
    one("ms", "Malay", "ms", &[]),
    one("ne", "Nepali", "ne", &[]),
    one("nb", "Norwegian", "nb", &[]),
    one("fa", "Persian", "fa", &[]),
    one("pl", "Polish", "pl", &[]),
    one("pt", "Portuguese", "pt", &[]),
    one("ro", "Romanian", "ro", &[]),
    one("ru", "Russian", "ru", &[]),
    one("si", "Sinhala", "si", &[]),
    one("es", "Spanish", "es", &[Latin]),
    one("sv", "Swedish", "sv", &[]),
    one("ta", "Tamil", "ta", &[]),
    one("th", "Thai", "th", &[Thai]),
    one("tr", "Turkish", "tr", &[]),
    one("uk", "Ukrainian", "uk", &[]),
    one("ur", "Urdu", "ur", &[]),
    one("vi", "Vietnamese", "vi", &[]),
];

/// A language's English name by code; both Chinese scripts are "Chinese".
/// An unknown code is uppercased; a name the AI dictionary kept, lower
/// case, gets its capitals back.
pub fn name(code: &str) -> String {
    let known = LANGUAGES.iter().find(|l| l.code == code);
    known.map_or_else(|| unknown(code), |l| base(l.name).to_owned())
}

/// "Chinese" of "Chinese (Simplified)".
fn base(name: &str) -> &str {
    name.split_once(" (").map_or(name, |(base, _)| base)
}

fn unknown(code: &str) -> String {
    if code.chars().count() <= 3 {
        return code.to_uppercase();
    }
    let mut after_letter = false;
    let mut named = String::with_capacity(code.len());
    for ch in code.chars() {
        match after_letter {
            true => named.push(ch),
            false => named.extend(ch.to_uppercase()),
        }
        after_letter = ch.is_alphabetic();
    }
    named
}

/// A language's code by English name, in any case, as packs and the AI
/// dictionary keep it; a variant, as "Chinese (Simplified)", joins its
/// language. A name not listed keeps itself, lower case.
pub fn code_of(name: &str) -> String {
    let same = |n: &str| n.eq_ignore_ascii_case(name) || n.eq_ignore_ascii_case(base(name));
    let is = |l: &&Language| same(l.name) || same(base(l.name));
    let known = LANGUAGES.iter().find(is);
    known.map_or_else(|| name.to_lowercase(), |l| l.code.to_owned())
}

fn script_of(ch: char) -> Option<Script> {
    match u32::from(ch) {
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => Some(Script::Myanmar),
        0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Some(Script::Kana),
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F => {
            Some(Script::Han)
        }
        0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F => {
            Some(Script::Hangul)
        }
        0x0E00..=0x0E7F => Some(Script::Thai),
        0x41..=0x5A | 0x61..=0x7A | 0xC0..=0x24F => Some(Script::Latin),
        _ => None,
    }
}

/// The language of `langs` using fewest scripts that writes `text`;
/// Latin first names none.
pub fn detect(text: &str, langs: &[&str]) -> Option<&'static str> {
    let mut found: Vec<Script> = text.chars().filter_map(script_of).collect();
    if found.first()? == &Script::Latin {
        return None;
    }
    found.retain(|s| *s != Script::Latin);
    let mut speakers: Vec<(&'static str, usize)> = LANGUAGES
        .iter()
        .filter(|l| langs.contains(&l.code))
        .filter(|l| found.iter().all(|s| l.scripts.contains(s)))
        .map(|l| (l.code, l.scripts.len()))
        .collect();
    speakers.sort_unstable_by_key(|(code, n)| (*n, *code));
    // Both Chinese scripts are one language.
    speakers.dedup();
    match speakers.as_slice() {
        [(one, _)] => Some(*one),
        [(one, a), (_, b), ..] if a < b => Some(*one),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_with_one_speaker_names_its_language() {
        let all = ["en", "my", "jp", "fr"];
        assert_eq!(detect("မီး", &all), Some("my"));
        assert_eq!(detect("ひかり", &all), Some("jp"));
        assert_eq!(detect("（光）", &all), Some("jp"));
        assert_eq!(detect("light", &all), None);
        assert_eq!(detect("2026", &all), None);
        assert_eq!(detect("မီး", &["en", "jp"]), None);
    }

    #[test]
    fn han_alone_is_chinese_and_with_kana_japanese() {
        let all = ["en", "jp", "zh", "ko", "th"];
        assert_eq!(detect("日本", &all), Some("zh"));
        assert_eq!(detect("光る", &all), Some("jp"));
        assert_eq!(detect("日本", &["en", "jp"]), Some("jp"));
        assert_eq!(detect("사랑", &all), Some("ko"));
        assert_eq!(detect("รัก", &all), Some("th"));
        assert_eq!(detect("사랑", &["en", "jp"]), None);
    }

    #[test]
    fn languages_are_named_in_english() {
        assert_eq!(name("my"), "Burmese");
        assert_eq!(name("de"), "German");
        assert_eq!(name("xx"), "XX");
        assert_eq!(code_of("Japanese"), "jp");
        assert_eq!(code_of("burmese"), "my", "a name in any case");
        assert_eq!(code_of("Italian"), "it", "no pack: its tag");
        assert_eq!(name("it"), "Italian");
        assert_eq!(name("italian"), "Italian", "a name kept before tags");
        assert_eq!(code_of("Chinese (Simplified)"), "zh");
        assert_eq!(code_of("Chinese (Traditional)"), "zh");
        assert_eq!(name("zh"), "Chinese");
        assert_eq!(code_of("Portuguese (Brazil)"), "pt");
        assert_eq!(code_of("Klingon"), "klingon", "not listed");
    }

    #[test]
    fn one_table_gives_each_language_one_tag_and_name() {
        let tags: std::collections::HashSet<_> = LANGUAGES.iter().map(|l| l.tag).collect();
        assert_eq!(tags.len(), LANGUAGES.len());
        let names: Vec<&str> = LANGUAGES.iter().map(|l| l.name).collect();
        assert!(names.is_sorted(), "by name");
        let ja = LANGUAGES.iter().find(|l| l.tag == "ja").map(|l| l.code);
        assert_eq!(ja, Some("jp"), "a pack's code");
    }
}
