use super::*;
use mareader_settings::PageTask;
use mareader_study::Why;

fn asked(pane: u64, text: &str) -> Asked {
    Asked {
        pane: PaneId(pane),
        doc: 1,
        skill: Skill::Builtin(PageTask::Translate),
        name: "Translate".to_owned(),
        anchor: Anchor::Text(vec![(2, 4..9)]),
        book: None,
        spot: None,
        path: None,
        layout: None,
        auto_pin: false,
        text: text.to_owned(),
    }
}

fn scripted(script: Scripted) -> State {
    let mut state = State::default();
    state.set_runner(Arc::new(script));
    state
}

#[test]
fn an_answer_lands_once_and_says_who_gave_it() {
    let mut state = scripted(Scripted::Echo);
    let id = state.start(asked(1, " Bonjour "));
    assert_eq!(state.get(id).map(|a| a.by.as_str()), Some("Sample answers"));
    assert!(state.running());
    let (landed, changed) = state.receive();
    assert_eq!((landed, changed), (vec![id], true));
    let answer = state.get(id).map(|a| a.answer.clone());
    assert_eq!(answer, Some(Answer::Ready("Bonjour".into())));
    assert_eq!(state.receive(), (Vec::new(), false), "it lands once");
    assert!(!state.running());
}

#[test]
fn closing_a_selection_hides_it_without_stopping_the_run() {
    let mut state = scripted(Scripted::Silent);
    let id = state.start(asked(1, "x"));
    assert!(state.close(id));
    assert!(!state.get(id).unwrap().open);
    assert!(state.running());
    assert!(!state.close(id));
    assert!(state.show(id));
    assert!(state.get(id).unwrap().open);
    assert!(state.remove(id));
    assert!(!state.running());
}

#[test]
fn a_failure_lands_and_retrying_asks_again() {
    let mut state = scripted(Scripted::Failing);
    let id = state.start(asked(1, "x"));
    let _ = state.receive();
    let failed = state.get(id).map(|a| &a.answer);
    assert!(matches!(failed, Some(Answer::Failed(f)) if f.why == Why::Offline));
    state.set_runner(Arc::new(Scripted::Echo));
    assert!(state.retry(id));
    assert_eq!(state.get(id).map(|a| &a.answer), Some(&Answer::Waiting));
    let _ = state.receive();
    let text = state.get(id).map(|a| a.answer.text().to_owned());
    assert_eq!(text.as_deref(), Some("x"));
}

#[test]
fn selection_answers_are_not_evicted_or_lost_when_panes_change() {
    let mut state = scripted(Scripted::Silent);
    let first = state.start(asked(1, "first"));
    for _ in 0..MOST {
        state.start(asked(2, "more"));
    }
    assert_eq!(state.asks().len(), MOST + 1);
    state.keep_pane(PaneId(1), None);
    state.keep_read(|_| None);
    assert!(state.get(first).is_some());
    assert!(state.running());
}

#[test]
fn page_menu_asks_remain_transient_and_bounded() {
    let mut state = scripted(Scripted::Silent);
    let page = || Asked {
        anchor: Anchor::Spot {
            page: 0,
            x: 0.0,
            y: 0.0,
            near: 0,
        },
        ..asked(1, "page")
    };
    let first = state.start(page());
    for _ in 0..MOST {
        state.start(page());
    }
    assert!(state.get(first).is_none());
    let last = state.asks().last().unwrap().id;
    assert!(state.close(last));
    assert!(state.get(last).is_none());
    state.keep_pane(PaneId(1), None);
    assert!(state.asks().is_empty());
}

#[test]
fn an_anchor_starts_on_its_first_page() {
    let spot = Anchor::Spot {
        page: 4,
        x: 1.0,
        y: 2.0,
        near: 30,
    };
    assert_eq!((spot.page(), Anchor::Text(Vec::new()).page()), (4, 0));
    assert_eq!(spot.reveal(), (4, 30..31));
    assert_eq!(asked(1, "x").anchor.reveal(), (2, 4..9));
}
