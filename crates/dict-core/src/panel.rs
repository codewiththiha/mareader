//! Pack choice and pair resolution, pure over any pack row.

use crate::lang::{HUB, detect};

/// The facts one pack row lends the panel's logic.
pub trait PackLike {
    /// The pack's id.
    fn pack_id(&self) -> &str;
    /// The language the pack's headwords are in.
    fn source(&self) -> &str;
    /// The language the pack answers in.
    fn target(&self) -> &str;
    /// Whether the pack's database stands built.
    fn built(&self) -> bool;
}

/// The built pack that answers a card: the asked language's, or the
/// first one built.
pub fn answer_pack<'a, P: PackLike>(rows: &'a [P], lang: Option<&str>) -> Option<&'a P> {
    rows.iter()
        .find(|pack| pack.built() && lang.is_some_and(|want| pack.target() == want))
        .or_else(|| rows.iter().find(|pack| pack.built()))
}

/// Every built pack's id, in row order.
pub fn built_packs<P: PackLike>(rows: &[P]) -> Vec<String> {
    rows.iter()
        .filter(|pack| pack.built())
        .map(|pack| pack.pack_id().to_string())
        .collect()
}

/// The built packs `langs` names: an id, or a `source-target` pair.
/// Empty asks every pack.
pub fn named_packs<P: PackLike>(rows: &[P], langs: &[String]) -> Vec<String> {
    if langs.is_empty() {
        return built_packs(rows);
    }
    rows.iter()
        .filter(|pack| {
            pack.built()
                && (langs.iter().any(|lang| lang == pack.pack_id())
                    || langs
                        .iter()
                        .any(|lang| lang == &format!("{}-{}", pack.source(), pack.target())))
        })
        .map(|pack| pack.pack_id().to_string())
        .collect()
}

/// Every language a built pack joins: the shores a pair may name.
pub fn languages<P: PackLike>(rows: &[P]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pack in rows.iter().filter(|pack| pack.built()) {
        for lang in [pack.source(), pack.target()] {
            if !out.iter().any(|have| have == lang) {
                out.push(lang.to_string());
            }
        }
    }
    out
}

/// What stands between two shores, over the packs that are built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// One pack holds both shores.
    Direct,
    /// Two packs, joined at the tongue between them.
    Bridged,
    /// Neither a pack nor a way through.
    None,
}

/// The way between two shores, over the packs that are built.
pub fn route<P: PackLike>(rows: &[P], from: &str, to: &str) -> Route {
    let joins = |a: &str, b: &str| {
        rows.iter()
            .filter(|pack| pack.built())
            .any(|pack| {
                (pack.source() == a && pack.target() == b)
                    || (pack.source() == b && pack.target() == a)
            })
    };
    if joins(from, to) {
        return Route::Direct;
    }
    // A shore on the hub needs no pack to reach it.
    let reaches = |lang: &str| lang == HUB || joins(lang, HUB);
    if reaches(from) && reaches(to) {
        return Route::Bridged;
    }
    Route::None
}

/// The two shores a panel asks: the word's, and the answer's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pair {
    /// The typed language; `None` reads it off the word.
    pub from: Option<String>,
    /// The answering language; `None` takes the first other shore.
    pub to: Option<String>,
}

/// What the panel is asking, once the word has had its say.
#[derive(Debug, Clone, PartialEq)]
pub struct Asked {
    pub from: String,
    pub to: String,
    /// The language the word named for itself, when it named one.
    pub detected: Option<String>,
    pub route: Route,
}

/// The pair a search asks, the word having had its say.
pub fn resolve<P: PackLike>(rows: &[P], ask: &Pair, query: &str) -> Option<Asked> {
    let langs = languages(rows);
    if langs.len() < 2 {
        return None;
    }
    let detected = detect(query, &langs);
    // A chosen shore stands; the word only speaks when none was.
    let from = match &ask.from {
        Some(lang) if langs.iter().any(|have| have == lang) => lang.clone(),
        _ => detected
            .clone()
            .or_else(|| Some(HUB.to_string()))
            .filter(|lang| langs.iter().any(|have| have == lang))
            .or_else(|| langs.first().cloned())
            .unwrap_or_else(|| HUB.to_string()),
    };
    let to = ask
        .to
        .clone()
        .filter(|lang| *lang != from && langs.iter().any(|have| have == lang))
        .or_else(|| langs.iter().find(|lang| **lang != from).cloned())
        .unwrap_or_else(|| from.clone());
    let route = route(rows, &from, &to);
    Some(Asked {
        from,
        to,
        detected,
        route,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One stub row: an English-sourced pack, its target, its build.
    #[derive(Debug)]
    struct Pack {
        id: &'static str,
        target: &'static str,
        built: bool,
    }

    impl PackLike for Pack {
        fn pack_id(&self) -> &str {
            self.id
        }
        fn source(&self) -> &str {
            "en"
        }
        fn target(&self) -> &str {
            self.target
        }
        fn built(&self) -> bool {
            self.built
        }
    }

    fn pack(id: &'static str, target: &'static str, built: bool) -> Pack {
        Pack { id, target, built }
    }

    #[test]
    fn the_asked_language_answers_before_the_row_order() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let got = answer_pack(&rows, Some("my"));
        assert_eq!(got.map(PackLike::pack_id), Some("b-en-my"));
    }

    #[test]
    fn no_asked_language_answers_the_first_built_pack() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let got = answer_pack(&rows, None);
        assert_eq!(got.map(PackLike::pack_id), Some("a-en-jp"));
    }

    #[test]
    fn a_language_nobody_built_falls_to_one_that_is() {
        let rows = vec![pack("a-en-fr", "fr", false), pack("b-en-my", "my", true)];
        let got = answer_pack(&rows, Some("fr"));
        assert_eq!(got.map(PackLike::pack_id), Some("b-en-my"));
    }

    #[test]
    fn a_pack_still_building_never_answers() {
        let rows = vec![pack("a-en-fr", "fr", false)];
        assert!(answer_pack(&rows, Some("fr")).is_none());
    }

    #[test]
    fn a_named_pack_is_reachable_by_its_pair_too() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let langs = vec!["en-my".to_string()];
        assert_eq!(named_packs(&rows, &langs), vec!["b-en-my"]);
    }

    #[test]
    fn only_a_built_pack_names_its_shores() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-fr", "fr", false)];
        assert_eq!(languages(&rows), vec!["en".to_string(), "jp".to_string()]);
        // One shore on each side of one pack, each named once.
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        assert_eq!(
            languages(&rows),
            vec!["en".to_string(), "jp".to_string(), "my".to_string()]
        );
    }

    #[test]
    fn one_pack_joins_its_own_shores() {
        let rows = vec![pack("a-en-jp", "jp", true)];
        assert_eq!(route(&rows, "en", "jp"), Route::Direct);
        // The way home is the same pack read backwards.
        assert_eq!(route(&rows, "jp", "en"), Route::Direct);
    }

    #[test]
    fn two_shores_meet_over_the_hub() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        assert_eq!(route(&rows, "my", "jp"), Route::Bridged);
        // A shore on the hub needs no pack to reach the hub.
        assert_eq!(route(&rows, "en", "my"), Route::Direct);
    }

    #[test]
    fn a_shore_with_no_pack_has_no_route() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", false)];
        assert_eq!(route(&rows, "my", "jp"), Route::None);
    }

    #[test]
    fn the_word_names_its_own_shore() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let ask = Pair {
            from: None,
            to: Some("jp".to_string()),
        };
        // The word is Myanmar, so the ask is: no pack joins those two.
        let got = resolve(&rows, &ask, "\u{1019}\u{102E}\u{1038}").expect("a pair");
        assert_eq!(got.from, "my");
        assert_eq!(got.detected.as_deref(), Some("my"));
        assert_eq!(got.to, "jp");
        assert_eq!(got.route, Route::Bridged);
    }

    #[test]
    fn a_detected_word_still_asks_one_pack_when_it_can() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        // No target chosen: the first other shore is the hub, one pack away.
        let got = resolve(&rows, &Pair::default(), "\u{1019}\u{102E}\u{1038}").expect("a pair");
        assert_eq!(got.from, "my");
        assert_eq!(got.to, "en");
        assert_eq!(got.route, Route::Direct);
    }

    #[test]
    fn a_chosen_shore_outranks_the_word() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let ask = Pair {
            from: Some("en".to_string()),
            to: Some("my".to_string()),
        };
        // The word is Myanmar; the reader asked from English, so it stands.
        let got = resolve(&rows, &ask, "\u{1019}\u{102E}\u{1038}").expect("a pair");
        assert_eq!(got.from, "en");
        assert_eq!(got.to, "my");
        assert_eq!(got.detected.as_deref(), Some("my"));
    }

    #[test]
    fn a_latin_word_keeps_the_hub() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let got = resolve(&rows, &Pair::default(), "light").expect("a pair");
        assert_eq!(got.from, "en");
        assert_eq!(got.detected, None);
        assert_eq!(got.route, Route::Direct);
    }

    #[test]
    fn the_answer_never_repeats_the_ask() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let ask = Pair {
            from: Some("my".to_string()),
            to: Some("my".to_string()),
        };
        let got = resolve(&rows, &ask, "").expect("a pair");
        assert_eq!(got.from, "my");
        assert_eq!(got.to, "en");
    }

    #[test]
    fn a_chosen_shore_nobody_built_falls_to_one_that_is() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let ask = Pair {
            from: Some("fr".to_string()),
            to: Some("fr".to_string()),
        };
        let got = resolve(&rows, &ask, "").expect("a pair");
        assert_eq!(got.from, "en");
        assert_eq!(got.to, "jp");
    }

    #[test]
    fn no_pack_at_all_is_no_pair() {
        let none: Vec<Pack> = Vec::new();
        assert_eq!(resolve(&none, &Pair::default(), "light"), None);
        assert_eq!(
            resolve(&[pack("a-en-jp", "jp", false)], &Pair::default(), "light"),
            None
        );
    }

    #[test]
    fn an_empty_list_asks_every_built_pack() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-fr", "fr", false)];
        assert_eq!(named_packs(&rows, &[]), vec!["a-en-jp"]);
    }
}
