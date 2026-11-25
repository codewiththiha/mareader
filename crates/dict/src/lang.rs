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

/// Every language a pack joins: code, name and scripts.
const LANGUAGES: [(&str, &str, &[Script]); 9] = [
    ("en", "English", &[Script::Latin]),
    ("fr", "French", &[Script::Latin]),
    ("es", "Spanish", &[Script::Latin]),
    ("de", "German", &[Script::Latin]),
    ("my", "Burmese", &[Script::Myanmar]),
    ("jp", "Japanese", &[Script::Kana, Script::Han]),
    ("zh", "Chinese", &[Script::Han]),
    ("ko", "Korean", &[Script::Hangul, Script::Han]),
    ("th", "Thai", &[Script::Thai]),
];

/// A language's English name. An unknown code is uppercased; a name the
/// AI dictionary keeps, lower case, gets its capitals back.
pub fn name(code: &str) -> String {
    let known = LANGUAGES.iter().find(|(c, ..)| *c == code);
    known.map_or_else(|| unknown(code), |(_, n, _)| (*n).to_owned())
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
/// dictionary keep it; one no pack joins keeps its name, lower case. A
/// variant, as "Chinese (Simplified)", joins its language's pack.
pub fn code_of(name: &str) -> String {
    let base = name.split_once(" (").map_or(name, |(base, _)| base);
    let same = |n: &str| n.eq_ignore_ascii_case(name) || n.eq_ignore_ascii_case(base);
    let known = LANGUAGES.iter().find(|(_, n, _)| same(n));
    known.map_or_else(|| name.to_lowercase(), |(c, ..)| (*c).to_owned())
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
        .filter(|(code, _, scripts)| {
            langs.contains(code) && found.iter().all(|s| scripts.contains(s))
        })
        .map(|(code, _, scripts)| (*code, scripts.len()))
        .collect();
    speakers.sort_by_key(|(_, n)| *n);
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
        assert_eq!(code_of("Italian"), "italian");
        assert_eq!(name("italian"), "Italian");
        assert_eq!(code_of("Chinese (Simplified)"), "zh");
        assert_eq!(code_of("Chinese (Traditional)"), "zh");
        let kept = code_of("Portuguese (Brazil)");
        assert_eq!(name(&kept), "Portuguese (Brazil)");
    }
}
