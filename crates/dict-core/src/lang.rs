//! A word's language, read off the script it is written in.

/// The writing a character belongs to. One row per script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Myanmar,
    Kana,
    Han,
    Latin,
}

/// Every language: its code, the name it is offered by, and its scripts.
const LANGUAGES: &[(&str, &str, &[Script])] = &[
    ("en", "English", &[Script::Latin]),
    ("fr", "French", &[Script::Latin]),
    ("my", "Myanmar", &[Script::Myanmar]),
    ("jp", "Japanese", &[Script::Kana, Script::Han]),
];

/// Every language written in `script`, in table order.
pub fn speakers(script: Script) -> Vec<&'static str> {
    LANGUAGES
        .iter()
        .filter(|(_, _, scripts)| scripts.contains(&script))
        .map(|(code, _, _)| *code)
        .collect()
}

/// The script one character is written in, when some language's.
fn script_of(ch: char) -> Option<Script> {
    let ch = u32::from(ch);
    match ch {
        0x1000..=0x109F | 0xA9E0..=0xA9FF | 0xAA60..=0xAA7F => Some(Script::Myanmar),
        0x3040..=0x309F | 0x30A0..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => Some(Script::Kana),
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF => Some(Script::Han),
        0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x024F => Some(Script::Latin),
        _ => None,
    }
}

/// The script a word is written in: its first letter's, punctuation past.
pub fn script(text: &str) -> Option<Script> {
    text.chars().find_map(script_of)
}

/// The name `code` is offered by; a code no row claims names itself.
pub fn name(code: &str) -> String {
    LANGUAGES
        .iter()
        .find(|(have, _, _)| *have == code)
        .map(|(_, label, _)| (*label).to_string())
        .unwrap_or_else(|| code.to_uppercase())
}

/// The one language `text` can be, of `langs`. Latin names none.
pub fn detect(text: &str, langs: &[String]) -> Option<String> {
    let script = script(text)?;
    // English and French are both Latin: the script cannot choose.
    if script == Script::Latin {
        return None;
    }
    let mut only: Option<&str> = None;
    for lang in speakers(script) {
        if !langs.iter().any(|have| have == lang) {
            continue;
        }
        // Two speakers, both at hand: the script cannot tell them apart.
        if only.is_some() {
            return None;
        }
        only = Some(lang);
    }
    only.map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The languages a panel holds, from the packs that are built.
    fn langs(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|code| code.to_string()).collect()
    }

    #[test]
    fn a_script_with_one_speaker_names_its_language() {
        let all = langs(&["en", "my", "jp"]);
        assert_eq!(detect("မီး", &all).as_deref(), Some("my"));
        assert_eq!(detect("ひかり", &all).as_deref(), Some("jp"));
        // Kanji are Han, and Japanese is the only Han shore here.
        assert_eq!(detect("光", &all).as_deref(), Some("jp"));
    }

    #[test]
    fn latin_names_nothing_at_all() {
        let all = langs(&["en", "my", "jp"]);
        assert_eq!(detect("light", &all), None);
        assert_eq!(detect("Light", &all), None);
        // English and French are both Latin: two speakers, no verdict.
        assert_eq!(speakers(Script::Latin), vec!["en", "fr"]);
        assert_eq!(detect("lumière", &langs(&["en", "fr", "my"])), None);
    }

    #[test]
    fn a_language_nobody_built_is_no_verdict() {
        // Myanmar script with no Myanmar pack: the ask stays where it was.
        assert_eq!(detect("မီး", &langs(&["en", "jp"])), None);
    }

    #[test]
    fn punctuation_and_digits_are_not_a_script() {
        let all = langs(&["en", "my", "jp"]);
        // A leading bracket or a year must not defeat the letter after it.
        assert_eq!(detect("（光）", &all).as_deref(), Some("jp"));
        assert_eq!(detect("  မီး", &all).as_deref(), Some("my"));
        assert_eq!(detect("2026", &all), None);
        assert_eq!(detect("", &all), None);
        assert_eq!(script("···"), None);
    }

    #[test]
    fn the_first_letter_decides_a_mixed_ask() {
        let all = langs(&["en", "my", "jp"]);
        assert_eq!(detect("光 light", &all).as_deref(), Some("jp"));
        // A Latin opening is no verdict, not a Japanese one.
        assert_eq!(detect("light 光", &all), None);
    }

    #[test]
    fn a_code_no_row_claims_names_itself() {
        assert_eq!(name("my"), "Myanmar");
        assert_eq!(name("jp"), "Japanese");
        assert_eq!(name("zz"), "ZZ");
    }
}
