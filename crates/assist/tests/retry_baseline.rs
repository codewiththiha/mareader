//! Every AI task against a model that is busy once, then answers.
//!
//! The organizer and a page rewrite wait and ask again. Explain, study cards
//! and the judge fail on the first Busy; Phase 2 of the AI plan gives them a
//! retry and flips those three tests. See `docs/assist.md`.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use fm_bridge::Bridge;
use mareader_assist::{Apple, Assist, Loose, Organized, PageJob, Request, Rewrite, Rewrote, WAITS};
use mareader_settings::{AiSettings, Provider, SummaryStyle};
use mareader_study::Why;
use mareader_study::mode::{Attempt, Key, Wanted};
use mareader_study::page::{Piece, Role};

/// What the helper says when the model is busy.
const BUSY: &str = r#"{"error":"the model is busy","code":"rate_limited"}"#;

const QUICK: &str =
    r#"{"pos":"adjective","meaning":"Fast.","synonyms":["swift"],"usages":["Be quick."]}"#;
const BACKS: &str =
    r#"{"cards":[{"n":1,"meaning":"Hidden.","synonyms":[],"usage":"An arcane rule."}]}"#;
const VERDICT: &str = r#"{"verdict":"close","feedback":"Nearly: here it means roamed."}"#;
const SHELVING: &str = r#"{"shelves":["Poetry"]}"#;
const FILING: &str = r#"{"placements":[{"book":"b1","shelf":1},{"book":"b2","shelf":0}]}"#;
const SUMMARY: &str = r#"{"paragraphs":["The fox ran home before the rain."]}"#;

/// The service on a stand-in helper: busy once, then `answers` in turn.
fn assist(dir: &Path, answers: &[&str]) -> Assist {
    let count = dir.join("asked");
    let mut script = format!(
        "#!/usr/bin/env bash\nread -r _request || true\n\
         n=$(cat \"{0}\" 2>/dev/null || echo 0)\necho $((n + 1)) > \"{0}\"\n\
         case $n in\n0) printf '%s\\n' '{BUSY}' ;;\n",
        count.display()
    );
    for (i, answer) in answers.iter().enumerate() {
        let when = match i + 1 == answers.len() {
            true => "*".to_owned(),
            false => (i + 1).to_string(),
        };
        let lines = format!("'{{\"structured\":{answer}}}' '{{\"done\":true}}'");
        script.push_str(&format!("{when}) printf '%s\\n' {lines} ;;\n"));
    }
    script.push_str("esac\n");
    let path = dir.join("FMBridge");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    let assist = Assist::new(dir.to_owned(), Arc::new(|| {})).unwrap();
    let assist = assist.without_keychain();
    assist.with_apple(Apple::new(Bridge::new(&path)))
}

/// How many requests reached the helper.
fn asked(dir: &Path) -> u32 {
    let text = std::fs::read_to_string(dir.join("asked")).unwrap_or_default();
    text.trim().parse().unwrap_or(0)
}

/// Apple Intelligence explaining words; every other task follows it.
fn settings() -> AiSettings {
    let mut ai = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    let id = ai.add_preset(Provider::Apple, "m");
    ai.tasks.explain.preset = Some(id);
    ai
}

/// Polls `take` until it answers, or fails after thirty seconds.
fn answer<T>(mut take: impl FnMut() -> Option<T>) -> T {
    let start = Instant::now();
    loop {
        if let Some(got) = take() {
            return got;
        }
        assert!(start.elapsed() < Duration::from_secs(30), "no answer");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn wanted(lemma: &str) -> Wanted {
    Wanted {
        key: Key::new(lemma, None),
        form: lemma.to_owned(),
        sentence: format!("An {lemma} rule."),
    }
}

#[test]
fn explain_fails_on_the_first_busy() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(dir.path(), &[QUICK]);
    let explainer = a.configure(&settings());
    let got = explainer.explain("quick", "The quick fox", &|_| {});
    // Known gap: Phase 2 gives Explain one 2 s wait, then this answers.
    assert_eq!(got.err().map(|f| f.why), Some(Why::Busy));
    assert_eq!(asked(dir.path()), 1, "never asked again");
}

#[test]
fn study_cards_fail_on_the_first_busy() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(dir.path(), &[BACKS]);
    let _explainer = a.configure(&settings());
    let mut job = a.cards().write(vec![wanted("arcane")]);
    let backs = answer(|| job.take());
    // Known gap: Phase 2 waits 3 s, then 12 s, as the organizer does.
    assert_eq!(backs.err().map(|f| f.why), Some(Why::Busy));
    assert_eq!(asked(dir.path()), 1, "never asked again");
}

#[test]
fn the_judge_fails_on_the_first_busy() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(dir.path(), &[VERDICT]);
    let _explainer = a.configure(&settings());
    let attempt = Attempt {
        form: "wandered".to_owned(),
        lemma: "wander".to_owned(),
        sentence: "He wandered off.".to_owned(),
        answer: "walked".to_owned(),
    };
    let mut job = a.judge().judge(attempt);
    let judged = answer(|| job.take());
    // Known gap: Phase 2 gives the judge one 2 s wait, then this answers.
    assert_eq!(judged.err().map(|f| f.why), Some(Why::Busy));
    assert_eq!(asked(dir.path()), 1, "never asked again");
}

#[test]
fn the_organizer_waits_and_asks_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(dir.path(), &[SHELVING, FILING]);
    let _explainer = a.configure(&settings());
    let loose = |(i, title): (u128, &str)| Loose {
        key: i,
        title: title.to_owned(),
        author: String::new(),
    };
    let request = Request {
        books: [(1, "Odes"), (2, "Taxes")].into_iter().map(loose).collect(),
        shelves: Vec::new(),
        batch: 50,
        whole: true,
    };
    let start = Instant::now();
    let mut run = a.organize(request);
    let plan = answer(|| {
        run.poll().into_iter().find_map(|event| match event {
            Organized::Done(plan) => Some(plan),
            Organized::Progress(_) => None,
        })
    });
    let plan = plan.map_err(|f| f.why).unwrap();
    assert_eq!(plan.placed.len(), 1);
    assert_eq!(asked(dir.path()), 3, "busy, then shelving and filing");
    assert!(start.elapsed() >= WAITS[0], "it waited {:?}", WAITS[0]);
}

#[test]
fn a_page_rewrite_waits_and_asks_again() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = assist(dir.path(), &[SUMMARY]);
    let _explainer = a.configure(&settings());
    let job = PageJob {
        rewrite: Rewrite::Summarize(SummaryStyle::Paragraphs),
        pieces: vec![Piece::text(Role::Para, "The fox ran home.")],
        pages: 1,
    };
    let start = Instant::now();
    let mut rewriting = a.rewrite(job);
    let done = answer(|| {
        rewriting.poll().into_iter().find_map(|news| match news {
            Rewrote::Done(done) => Some(done),
            Rewrote::Partial(_) => None,
        })
    });
    assert!(done.is_ok(), "{:?}", done.err());
    assert_eq!(asked(dir.path()), 2, "busy, then the summary");
    assert!(start.elapsed() >= WAITS[0], "it waited {:?}", WAITS[0]);
}
