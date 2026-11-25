//! Ways between two languages: one pack either way round, or two
//! meeting at English.

use crate::lang;
use crate::packs::Pack;

/// The language every bridge passes through.
pub const HUB: &str = "en";

/// One pack, asked from its headwords or from its definitions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hop {
    pub pack: &'static Pack,
    /// Asked from the definition side, back to the headword.
    pub reverse: bool,
}

/// One way from a language to another: one hop, or two through `HUB`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub hops: Vec<Hop>,
}

/// Every plan from `from` to `to` over `packs`, best first.
pub fn plans(from: &str, to: &str, packs: &[&'static Pack]) -> Vec<Plan> {
    let mut out = direct(from, to, packs);
    if from != HUB && to != HUB {
        for a in direct(from, HUB, packs) {
            for b in direct(HUB, to, packs) {
                let hops = a.hops.iter().chain(&b.hops).copied().collect();
                out.push(Plan { hops });
            }
        }
    }
    out
}

fn direct(from: &str, to: &str, packs: &[&'static Pack]) -> Vec<Plan> {
    let forward = packs.iter().filter(|p| p.source == from && p.target == to);
    let back = packs.iter().filter(|p| p.source == to && p.target == from);
    let hop = |reverse| {
        move |pack: &&'static Pack| Plan {
            hops: vec![Hop { pack, reverse }],
        }
    };
    forward.map(hop(false)).chain(back.map(hop(true))).collect()
}

/// Every language the packs join, in pack order.
pub fn languages(packs: &[&'static Pack]) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for lang in packs.iter().flat_map(|p| [p.source, p.target]) {
        if !out.contains(&lang) {
            out.push(lang);
        }
    }
    out
}

/// The languages an English word is looked up in: each pack's other
/// side, then each kept in the AI dictionary, English included.
///
/// The one list every dictionary-language picker offers.
pub fn targets<'a>(packs: &[&'static Pack], ai: &'a [String]) -> Vec<&'a str> {
    let sides = packs.iter().flat_map(|p| [p.source, p.target]);
    let packs = sides.filter(|l| *l != HUB);
    let mut out: Vec<&'a str> = Vec::new();
    for lang in packs.chain(ai.iter().map(String::as_str)) {
        if !out.contains(&lang) {
            out.push(lang);
        }
    }
    out
}

/// English, then [`targets`]; none when nothing answers.
pub fn languages_with<'a>(packs: &[&'static Pack], ai: &'a [String]) -> Vec<&'a str> {
    let targets = targets(packs, ai);
    if targets.is_empty() {
        return targets;
    }
    let others = targets.into_iter().filter(|l| *l != HUB);
    std::iter::once(HUB).chain(others).collect()
}

/// The language a card answers in: `chosen` when `langs` offers it, else
/// the first offered (Automatic); none when nothing is offered.
pub fn answer_in<'a>(chosen: Option<&str>, langs: &[&'a str]) -> Option<&'a str> {
    let picked = chosen.and_then(|c| langs.iter().copied().find(|l| *l == c));
    picked.or_else(|| langs.first().copied())
}

/// Two languages a search names; none leaves each to be worked out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pair {
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Chosen languages stand; else script, then English, names `from`.
pub fn settle(pair: &Pair, word: &str, langs: &[&str]) -> Option<(String, String)> {
    if langs.len() < 2 {
        return None;
    }
    let known = |l: &Option<String>| l.clone().filter(|l| langs.contains(&l.as_str()));
    let from = known(&pair.from)
        .or_else(|| lang::detect(word, langs).map(str::to_owned))
        .unwrap_or_else(|| HUB.to_owned());
    let to = known(&pair.to)
        .filter(|t| *t != from)
        .or_else(|| langs.iter().find(|l| **l != from).map(|l| (*l).to_owned()))?;
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::PACKS;

    fn all() -> Vec<&'static Pack> {
        PACKS.iter().collect()
    }

    #[test]
    fn a_pack_answers_both_ways_and_two_meet_at_english() {
        let packs = all();
        let en_jp = plans("en", "jp", &packs);
        assert_eq!(en_jp.first().map(|p| p.hops.len()), Some(1));
        let back = plans("my", "en", &packs);
        assert!(
            back.first()
                .is_some_and(|p| p.hops.iter().all(|h| h.reverse))
        );
        let bridge = plans("my", "jp", &packs);
        let hops = bridge.first().map(|p| p.hops.clone()).unwrap_or_default();
        assert_eq!(hops.len(), 2);
        assert!(
            hops.first()
                .is_some_and(|h| h.reverse && h.pack.target == "my")
        );
        assert!(
            hops.get(1)
                .is_some_and(|h| !h.reverse && h.pack.target == "jp")
        );
        assert!(plans("my", "it", &packs).is_empty());
    }

    #[test]
    fn a_pair_settles_from_the_choice_then_the_word() {
        let packs = languages(&all());
        let none = Pair::default();
        let got = settle(&none, "မီး", &packs);
        assert_eq!(got, Some(("my".into(), "en".into())));
        let got = settle(&none, "light", &packs);
        assert_eq!(got, Some(("en".into(), "my".into())));
        let chosen = Pair {
            from: Some("en".into()),
            to: Some("en".into()),
        };
        assert_eq!(settle(&chosen, "x", &packs).map(|p| p.1), Some("my".into()));
        assert_eq!(settle(&none, "x", &[]), None);
    }

    #[test]
    fn the_ai_dictionarys_languages_join_after_the_packs() {
        let my: Vec<&'static Pack> = PACKS.iter().filter(|p| p.id == "mcfnlp-en-my").collect();
        let ai = ["my".to_owned(), "russian".to_owned()];
        assert_eq!(languages_with(&my, &ai), ["en", "my", "russian"]);
        assert_eq!(languages_with(&[], &ai), ["en", "my", "russian"]);
        assert!(languages_with(&[], &[]).is_empty());
        assert_eq!(targets(&my, &ai), ["my", "russian"]);
        let en = ["en".to_owned()];
        let english = targets(&my, &en);
        assert_eq!(english, ["my", "en"], "English meanings of English words");
        let offered = targets(&my, &ai);
        assert_eq!(answer_in(Some("russian"), &offered), Some("russian"));
        let gone = answer_in(Some("jp"), &offered);
        assert_eq!(gone, Some("my"), "a choice no longer offered: Automatic");
        assert_eq!(answer_in(None, &offered), Some("my"));
        assert_eq!(answer_in(Some("my"), &[]), None);
        let to = Pair {
            from: None,
            to: Some("russian".into()),
        };
        let got = settle(&to, "quick", &languages_with(&[], &ai));
        assert_eq!(got, Some(("en".into(), "russian".into())));
    }
}
