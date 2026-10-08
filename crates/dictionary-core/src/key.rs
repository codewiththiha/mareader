//! Keys: how a word and a query are folded before the store sees them.

/// The characters a copied word carries that carry no meaning.
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200d}' | '\u{00ad}' | '\u{2060}' | '\u{feff}'
    )
}

/// One character's fold: full width, katakana, curly quotes.
fn fold_char(c: char) -> char {
    match c {
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
        '\u{3000}' => ' ',
        '\u{30a1}'..='\u{30f6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        '\u{2018}' | '\u{2019}' | '\u{02bc}' | '\u{2032}' => '\'',
        '\u{201c}' | '\u{201d}' => '"',
        _ => c,
    }
}

/// Fold one word to its key: case, width, kana, whitespace.
pub fn fold_key(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut space = false;
    for c in word.chars() {
        if is_invisible(c) {
            continue;
        }
        let c = fold_char(c);
        if c.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            space = false;
        }
        for lowered in c.to_lowercase() {
            out.push(lowered);
        }
    }
    out
}

/// The keys a query is tried as, most specific first.
pub fn query_keys(query: &str) -> Vec<String> {
    let key = fold_key(query);
    let mut keys = Vec::new();
    if key.is_empty() {
        return keys;
    }
    keys.push(key.clone());
    if key.contains(['-', ' ']) {
        for part in key.split(['-', ' ']) {
            let part = part.to_string();
            if !part.is_empty() && !keys.contains(&part) {
                keys.push(part);
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_lowercase_and_trimmed() {
        assert_eq!(fold_key("Ephemeral"), "ephemeral");
        assert_eq!(fold_key("  palimpsest  "), "palimpsest");
        assert_eq!(fold_key("don't"), "don't");
        assert_eq!(fold_key(""), "");
        assert_eq!(fold_key("   "), "");
    }

    #[test]
    fn inner_whitespace_collapses_to_one_space() {
        assert_eq!(fold_key("well  known"), "well known");
        assert_eq!(fold_key("well\tknown\n"), "well known");
        assert_eq!(fold_key(" a  b "), "a b");
    }

    #[test]
    fn an_invisible_character_never_reaches_the_store() {
        assert_eq!(fold_key("a\u{200b}b"), "ab");
        assert_eq!(fold_key("\u{feff}word"), "word");
        assert_eq!(fold_key("hy\u{00ad}phen"), "hyphen");
    }

    #[test]
    fn width_and_kana_fold_to_the_plain_spelling() {
        assert_eq!(fold_key("ｖｅｒｂ"), "verb");
        assert_eq!(fold_key("ｗｏｒｄ　２"), "word 2");
        assert_eq!(fold_key("カタカナ"), "かたかな");
        // Latin and the long vowel mark are left alone.
        assert_eq!(fold_key("Ｃafé"), "café");
        assert_eq!(fold_key("コーヒー"), "こーひー");
    }

    #[test]
    fn curly_quotes_fold_to_the_straight_one() {
        assert_eq!(fold_key("don\u{2019}t"), "don't");
        assert_eq!(fold_key("\u{201c}quoted\u{201d}"), "\"quoted\"");
    }

    #[test]
    fn a_query_is_tried_whole_before_it_is_tried_in_parts() {
        assert_eq!(query_keys("Palimpsest"), vec!["palimpsest".to_string()]);
        let parts = ["well-known", "well", "known"].map(str::to_string);
        assert_eq!(query_keys("well-known"), parts.to_vec());
        let spaced = ["well known", "well", "known"].map(str::to_string);
        assert_eq!(query_keys("well known"), spaced.to_vec());
        // A contraction is one word; its parts are not words.
        let one = ["don't".to_string()];
        assert_eq!(query_keys("don't"), one.to_vec());
        assert_eq!(query_keys("  "), Vec::<String>::new());
    }
}
