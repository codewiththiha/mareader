//! What the AI dictionary is given, and the offers its replies raise.

#![allow(clippy::unwrap_used)]

use mareader_study::WordInfo;
use mareader_study::mode::Key as CardKey;

use super::*;

fn back(meaning: &str, pos: Option<Pos>) -> Written {
    Written {
        pos,
        meaning: meaning.into(),
        synonyms: vec!["quick".into()],
        usage: " A brisk walk. ".into(),
        book_only: false,
    }
}

fn wanted(pos: Option<Pos>) -> Wanted {
    Wanted {
        key: CardKey::new("brisk", pos),
        form: "brisker".into(),
        sentence: "The air was brisker.".into(),
    }
}

#[test]
fn a_back_is_kept_under_its_lemma_and_the_ai_class() {
    let who = ("Model", "Japanese");
    let written = back("lively", Some(Pos::Adjective));
    let saved = of_back(&wanted(Some(Pos::Verb)), &written, who).unwrap();
    assert_eq!(saved.word, "brisk", "the lemma, not the form read");
    assert_eq!(saved.pos, "adjective", "the AI's class wins");
    assert_eq!(saved.lang, "jp", "the packs' code");
    assert_eq!(saved.usages, ["A brisk walk."]);
    assert_eq!(saved.model, "Model");
    let tagged = of_back(&wanted(Some(Pos::Verb)), &back("x", None), who);
    assert_eq!(tagged.unwrap().pos, "verb", "else the card's own");
}

#[test]
fn book_words_and_classless_or_empty_backs_stay_out() {
    let who = ("Model", "English");
    let named = Written {
        book_only: true,
        ..back("a character", Some(Pos::Noun))
    };
    assert_eq!(of_back(&wanted(None), &named, who), None);
    assert_eq!(of_back(&wanted(None), &back("lively", None), who), None);
    let blank = back("  ", Some(Pos::Noun));
    assert_eq!(of_back(&wanted(None), &blank, who), None);
}

fn said(pos: &str, book_only: bool) -> Said {
    let info = WordInfo {
        pos: pos.into(),
        meaning: "moving quickly".into(),
        book_only,
        ..WordInfo::default()
    };
    Said {
        info,
        by: "Model".into(),
        word: "Brisk".into(),
    }
}

#[test]
fn an_explain_answer_is_kept_under_the_word_asked() {
    let saved = of_said(&said("Adjective", false), "English").unwrap();
    assert_eq!(saved.word, "Brisk");
    assert_eq!(saved.pos, "adjective");
    assert_eq!(saved.lang, "en");
    assert_eq!(of_said(&said("adjective", true), "English"), None);
    assert_eq!(of_said(&said("", false), "English"), None, "no class");
}

fn reply(saved: Vec<Saved>) -> Reply {
    Reply {
        saved,
        ..Reply::default()
    }
}

#[test]
fn a_differing_kept_meaning_raises_an_offer_until_taken() {
    let mut filing = Filing::default();
    let new = of_said(&said("adjective", false), "English").unwrap();
    let kept = Saved {
        meaning: "cold and fresh".into(),
        ..new.clone()
    };
    let (key, ticket) = (Key::Loose(1), Ticket(7));
    filing.sent(ticket, key, new.clone());
    assert!(!filing.collect(|_| None), "no reply yet");
    let raised = filing.collect(|t| (t == ticket).then(|| reply(vec![kept.clone()])));
    assert!(raised);
    assert_eq!(filing.offer(key).unwrap().kept, kept);
    filing.sent(Ticket(8), key, new.clone());
    assert!(filing.offer(key).is_none(), "a newer answer drops it");
    let quiet = filing.collect(|_| Some(reply(Vec::new())));
    assert!(!quiet, "saved, no offer");
    assert!(filing.take_offer(key).is_none());
}

#[test]
fn ai_rows_lead_only_when_asked() {
    let row = |pack: &'static str, ai: Option<Saved>| mareader_dict::Entry {
        word: "brisk".into(),
        pos: None,
        definition: pack.into(),
        romanization: None,
        sense: None,
        pack,
        via: None,
        fit: mareader_dict::WordMatch::Exact,
        flipped: false,
        ai: ai.map(Box::new),
    };
    let mut reply = Reply::default();
    let ai = Some(Saved::default());
    reply.entries = vec![row("pack", None), row("ai", ai)];
    let packs = |r: &Reply| r.entries.iter().map(|e| e.pack).collect::<Vec<_>>();
    lead(&mut reply, false);
    assert_eq!(packs(&reply), ["pack", "ai"]);
    lead(&mut reply, true);
    assert_eq!(packs(&reply), ["ai", "pack"]);
}
