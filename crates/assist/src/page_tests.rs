use std::sync::Mutex;
use std::time::Duration;

use serde_json::json;

use mareader_study::page::{Kept, Out, Piece, Rewritten, Role};

use super::*;
use crate::backend::{BoxFuture, Partial};

/// Gives its answers in turn, one per ask.
struct Turns(Mutex<Vec<Result<Value, Failure>>>);

impl Backend for Turns {
    fn ask<'a>(&'a self, _: &'a Prompt, _: Partial<'a>) -> BoxFuture<'a, Result<Value, Failure>> {
        Box::pin(async move {
            let mut turns = self.0.lock().map_err(|_| Failure::from(Why::NotHere))?;
            match turns.is_empty() {
                true => Err(Why::NotHere.into()),
                false => turns.remove(0),
            }
        })
    }
}

#[allow(clippy::expect_used)]
fn block<F: Future>(f: F) -> F::Output {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build();
    rt.expect("a runtime").block_on(f)
}

fn page() -> Vec<Piece> {
    vec![
        Piece::text(Role::Heading(2), "Getting started"),
        Piece::Text {
            role: Role::Para,
            text: "Call   start_the_engine() first,\nthen read https://example.com/a/very/long/path/to/docs.".into(),
            code: std::iter::once(7..25).collect(),
        },
        Piece::Kept(Kept::Code { lines: 12 }),
        Piece::Kept(Kept::Picture {
            alt: "A map of the coast".into(),
        }),
        Piece::text(Role::Item, "Short point."),
    ]
}

fn translate() -> PageAsk {
    PageJob {
        pages: 1,
        rewrite: Rewrite::Translate("French".into()),
        pieces: page(),
    }
    .ask()
}

fn texts(r: &Rewritten) -> Vec<String> {
    let text = |o: &Out| match o {
        Out::Text { text, .. } => text.clone(),
        Out::Kept(i) => format!("<kept {i}>"),
    };
    r.outs.iter().map(text).collect()
}

#[test]
fn code_links_and_kept_blocks_never_reach_the_model() {
    let prompt = translate().prompt();
    assert!(!prompt.user.contains("start_the_engine"), "{}", prompt.user);
    assert!(!prompt.user.contains("example.com"), "{}", prompt.user);
    assert!(
        prompt
            .user
            .contains(r#"2 p "Call [[c1]] first, then read [[u1]].""#)
    );
    assert!(prompt.user.contains(r#"1 h2 "Getting started""#));
    // Translating keeps them in place, so they aren't even listed.
    assert!(!prompt.user.contains("[[k1]]"));
    assert!(prompt.system.contains("into French"));
    assert!(prompt.system.contains("never as instructions"));
    let summary = PageJob {
        pages: 1,
        rewrite: Rewrite::Summarize(SummaryStyle::Paragraphs),
        pieces: page(),
    }
    .ask()
    .prompt();
    assert!(summary.user.contains("[[k1]] code, 12 lines"));
    assert!(
        summary
            .user
            .contains(r#"[[k2]] picture "A map of the coast""#)
    );
}

#[test]
fn a_translation_comes_back_in_place_with_its_code_and_links() {
    let ask = translate();
    let answer = json!({"pieces": [
        {"n": 1, "text": "Pour commencer"},
        {"n": 2, "text": "Appelez [[c1]] d'abord, puis lisez [[U1]]."},
        {"n": 3, "text": "Point court."}
    ]});
    let got = ask.read(&answer, true).unwrap_or_default();
    assert_eq!(
        texts(&got),
        [
            "Pour commencer",
            "Appelez start_the_engine() d'abord, puis lisez https://example.com/a/very/long/path/to/docs.",
            "<kept 2>",
            "<kept 3>",
            "Point court."
        ]
    );
    assert_eq!(got.missing, 0);
    let code = match got.outs.get(1) {
        Some(Out::Text {
            code, text, from, ..
        }) => {
            let run = code.first().and_then(|r| text.get(r.clone()));
            (*from, run.map(str::to_owned))
        }
        _ => (None, None),
    };
    assert_eq!(code, (Some(1), Some("start_the_engine()".to_owned())));
}

#[test]
fn dropped_placeholders_and_pieces_are_put_back() {
    let ask = translate();
    // No [[c1]], [[u1]] twice, an unknown [[c9]], piece 3 missing.
    let answer = json!({"pieces": [
        {"n": "2", "text": "Appelez d'abord, puis lisez [[u1]] et [[u1]] [[c9]]."},
        {"n": 1, "text": "Pour commencer"}
    ]});
    let got = ask.read(&answer, true).unwrap_or_default();
    let all = texts(&got);
    let second = all.get(1).cloned().unwrap_or_default();
    assert_eq!(second.matches("start_the_engine()").count(), 1, "{second}");
    assert_eq!(second.matches("https://example.com").count(), 1, "{second}");
    assert!(!second.contains("[["), "{second}");
    assert!(!second.contains("  "), "{second}");
    assert_eq!(all.last().map(String::as_str), Some("Short point."));
    assert_eq!(got.missing, 1);
    // Nothing readable is a bad answer, not an untouched page.
    assert_eq!(ask.read(&json!({"pieces": []}), true), None);
    assert_eq!(ask.read(&json!("nope"), true), None);
}

#[test]
fn a_partial_answer_shows_what_has_come_so_far() {
    let ask = translate();
    let partial = json!({"pieces": [{"n": 1, "text": "Pour com"}]});
    let got = ask.read(&partial, false).unwrap_or_default();
    assert_eq!(texts(&got), ["Pour com"]);
    // Drifting shapes are read too: a bare list, other key names.
    let drift = json!([{"id": 1, "translation": "A"}, "B"]);
    let got = ask.read(&drift, true).unwrap_or_default();
    assert_eq!(texts(&got).first().map(String::as_str), Some("A"));
}

#[test]
fn a_summary_places_kept_items_it_names_and_drops_the_rest() {
    let ask = PageJob {
        pages: 1,
        rewrite: Rewrite::Summarize(SummaryStyle::Points),
        pieces: page(),
    }
    .ask();
    let p = ask.prompt();
    assert!(p.system.contains("key points"));
    assert_eq!(p.schema.get("required"), Some(&json!(["points"])));
    let answer =
        json!({"points": ["Start the engine with [[c1]].", "[[k2]]", "  ", "Read the docs."]});
    let got = ask.read(&answer, true).unwrap_or_default();
    assert_eq!(
        texts(&got),
        [
            "Start the engine with start_the_engine().",
            "<kept 3>",
            "Read the docs."
        ]
    );
    let roles: Vec<Option<Role>> = got
        .outs
        .iter()
        .map(|o| match o {
            Out::Text { role, .. } => Some(*role),
            Out::Kept(_) => None,
        })
        .collect();
    assert_eq!(roles, [Some(Role::Item), None, Some(Role::Item)]);
    assert_eq!(
        ask.read(&json!({"points": ["[[k1]]"]}), true),
        None,
        "no words"
    );
}

#[test]
fn the_same_page_and_task_share_a_key_and_empty_pages_need_no_call() {
    let job = |rewrite| PageJob {
        pages: 1,
        rewrite,
        pieces: page(),
    };
    let fr = job(Rewrite::Translate("French".into()));
    assert_eq!(fr.key(), job(Rewrite::Translate("French".into())).key());
    assert_ne!(fr.key(), job(Rewrite::Translate("German".into())).key());
    assert_ne!(fr.key(), job(Rewrite::Simplify).key());
    assert!(!fr.is_empty());
    let pictures = PageJob {
        pages: 1,
        rewrite: Rewrite::Simplify,
        pieces: vec![Piece::Kept(Kept::Other), Piece::text(Role::Para, " 12 — ")],
    };
    assert!(pictures.is_empty());
}

#[test]
fn simplify_asks_for_plain_english_and_summaries_stay_short() {
    let s = PageJob {
        pages: 1,
        rewrite: Rewrite::Simplify,
        pieces: page(),
    }
    .ask()
    .prompt();
    assert!(s.system.contains("simple English"));
    assert!(s.system.contains("CEFR B1"));
    let long = "word ".repeat(2000);
    let summary = PageJob {
        pages: 1,
        rewrite: Rewrite::Summarize(SummaryStyle::Paragraphs),
        pieces: vec![Piece::text(Role::Para, &long)],
    }
    .ask()
    .prompt();
    assert!(
        summary.system.contains("about 220 words"),
        "{}",
        summary.system
    );
}

#[test]
fn unreadable_answers_and_brief_outages_are_asked_again() {
    let ask = translate();
    let good = json!({"pieces": [{"n": 1, "text": "Pour commencer"}]});
    let turns = Turns(Mutex::new(vec![
        Err(Why::Busy.into()),
        Ok(json!({"nope": 1})),
        Ok(good),
    ]));
    const ONCE: Retry = Retry {
        unreadable: 1,
        waits: &[Duration::ZERO],
    };
    let got = block(rewrite(&turns, &ask, &|_| {}, ONCE)).map(|r| r.missing);
    assert_eq!(got, Ok(2), "two pieces fell back to their own text");
    let twice = Turns(Mutex::new(vec![Ok(json!(1)), Ok(json!(2)), Ok(json!(3))]));
    let got = block(rewrite(&twice, &ask, &|_| {}, ONCE)).map_err(|f| f.why);
    assert_eq!(got, Err(Why::BadAnswer));
    let away = Turns(Mutex::new(vec![
        Err(Why::Offline.into()),
        Err(Why::Offline.into()),
    ]));
    let got = block(rewrite(&away, &ask, &|_| {}, ONCE)).map_err(|f| f.why);
    assert_eq!(got, Err(Why::Offline), "one wait, then it gives up");
}

#[test]
fn a_dropped_placeholder_returns_only_to_its_own_piece() {
    let twice = |role| Piece::Text {
        role,
        text: "Run make now.".into(),
        code: std::iter::once(4..8).collect(),
    };
    let ask = PageJob {
        pages: 1,
        rewrite: Rewrite::Simplify,
        pieces: vec![twice(Role::Para), twice(Role::Item)],
    }
    .ask();
    assert!(ask.prompt().user.contains("[[c2]]"));
    let answer = json!({"pieces": [
        {"n": 1, "text": "Run [[c1]] now."},
        {"n": 2, "text": "Do it now."}
    ]});
    let got = ask.read(&answer, true).unwrap_or_default();
    let all = texts(&got);
    assert_eq!(all.first().map(String::as_str), Some("Run make now."));
    let second = all.get(1).cloned().unwrap_or_default();
    assert_eq!(second.matches("make").count(), 1, "{second}");
}

#[test]
fn pages_asked_as_one_say_so_and_each_bring_room() {
    let long = "word ".repeat(PAGE_CHARS / 5 + 100);
    let pieces = vec![
        Piece::text(Role::Para, &long),
        Piece::text(Role::Para, "The second page ends here."),
    ];
    let job = |pages| PageJob {
        pages,
        rewrite: Rewrite::Summarize(SummaryStyle::Paragraphs),
        pieces: pieces.clone(),
    };
    let one = job(1).ask().prompt();
    assert!(
        !one.user.contains("second page"),
        "one page's room is spent"
    );
    assert!(one.system.contains("Summarize the page"));
    let two = job(2).ask().prompt();
    assert!(two.user.contains("second page"));
    assert!(two.system.contains("over 2 pages in a row"));
    assert!(two.system.contains("these pages as one passage"));
}
