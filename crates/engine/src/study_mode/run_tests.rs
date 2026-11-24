//! A pane's run: held before planning, planned on a real PDF from the page
//! it turned on at, its gate, the known cards' trail and how pages stand.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::Arc;

use mareader_reflow::Typography;
use mareader_study::mode::{Card, Unit, Via, plan_from};

use crate::study_mode::counts::Lengths;

use super::*;

const PAGES: usize = 6;

fn document() -> Arc<Document> {
    let bytes = mareader_pdf::sample::pdf(PAGES, "Sample");
    let opened = Document::open(Arc::new(bytes), &Typography::default());
    Arc::new(opened.unwrap())
}

fn size(pages: usize) -> Size {
    Size {
        unit: Unit::Pages,
        pages,
        words: 12,
    }
}

fn sheet(document: &Arc<Document>, saved: Option<usize>, current: usize) -> Sheet<'_> {
    Sheet {
        document,
        wake: Arc::new(|| {}),
        inline: true,
        size: size(3),
        saved,
        start: None,
        current,
    }
}

/// A run planned by hand on nine pages of three-page batches.
fn planned(unlocked: usize) -> Run {
    let mut run = Run::new(Mode::Proactive, None, 0);
    run.sizes = vec![400; 9];
    run.batches = plan_from(&run.sizes, size(3), 0);
    run.layout = Some((None, 9));
    run.size = Some(size(3));
    run.unlocked = unlocked;
    run
}

fn shown(lemma: &str) -> Shown {
    let card = Card {
        key: Key::new(lemma, None),
        form: lemma.to_owned(),
        sentence: String::new(),
        band: 5,
        count: 1,
        also: Vec::new(),
    };
    Shown::new(card, String::new())
}

#[test]
fn an_unplanned_run_holds_the_pane_where_it_was() {
    let run = Run::new(Mode::Proactive, None, 4);
    assert!(!run.planned());
    assert_eq!(run.gate(), Some(4));
    assert!(run.gate_batch().is_none());
}

#[test]
fn a_fresh_book_locks_its_first_batch_and_a_finished_one_nothing() {
    let doc = document();
    let mut run = Run::new(Mode::Proactive, None, 0);
    assert_eq!(run.plan(sheet(&doc, None, 0)), Ok(true));
    assert!(run.planned());
    assert_eq!(run.gate(), Some(0));
    assert_eq!(run.gate_batch().map(|b| b.from), Some(0));
    assert_eq!(run.plan(sheet(&doc, None, 0)), Ok(false), "nothing moved");

    let mut done = Run::new(Mode::Proactive, None, 0);
    assert_eq!(done.plan(sheet(&doc, Some(PAGES), 0)), Ok(true));
    assert_eq!(
        done.gate(),
        Some(0),
        "turning on starts over where it stands"
    );
    done.restale();
    assert_eq!(done.plan(sheet(&doc, Some(PAGES), 0)), Ok(true));
    assert_eq!(done.gate(), None, "a saved gate past the end unlocks all");
}

#[test]
fn the_first_batch_starts_where_the_mode_turned_on() {
    let doc = document();
    let mut run = Run::new(Mode::Proactive, None, 2);
    assert_eq!(run.plan(sheet(&doc, None, 2)), Ok(true));
    assert_eq!(run.start, 2);
    assert_eq!(run.gate(), Some(2));
    assert_eq!(run.gate_batch().map(|b| b.from), Some(2));
    assert_eq!(run.back(false), Some(1), "Read back shows the page before");
    assert_eq!(run.back(true), None, "a lock keeps the pages before shut");
    assert_eq!((run.floor(true), run.floor(false)), (2, 0));
}

#[test]
fn the_gate_follows_unlocking_and_the_batch_before_it_reads_again() {
    let mut run = planned(3);
    assert_eq!(run.gate(), Some(3));
    assert_eq!(run.gate_batch().map(|b| (b.from, b.to)), Some((3, 5)));
    assert_eq!(run.back(true), Some(2));
    run.unlock_through(5);
    assert_eq!(run.gate(), Some(6));
    run.unlock_through(8);
    assert_eq!(run.gate(), None);
    assert!(planned(0).back(false).is_none());
}

#[test]
fn a_deck_for_a_batch_no_longer_at_the_gate_is_dropped() {
    let mut run = planned(3);
    let first = run.batches[0].clone();
    run.deck = DeckState::Failed {
        batch: first,
        why: String::new(),
        setup: None,
    };
    run.drop_stale_deck();
    assert!(matches!(run.deck, DeckState::None));
}

#[test]
fn known_cards_leave_the_deck_and_undo_puts_them_back_in_place() {
    let built = Built {
        cards: vec![shown("arcane"), shown("brisk"), shown("candid")],
        found: 3,
        skipped_known: 0,
        via: Via::Dictionary,
    };
    let batch = planned(0).batches[0].clone();
    let mut open = Open::new(batch, built, Some(20));
    assert_eq!(open.allowance.left(0.0), Some(60.0), "3 cards at 20 s");

    let key = Key::new("brisk", None);
    let (at, taken) = open.take_known(&key).unwrap();
    assert_eq!(at, 1);
    open.keep_known(at, taken, Prior::New);
    assert_eq!(open.cards.len(), 2);
    assert!(open.take_known(&key).is_none());

    assert_eq!(open.undo_known(), Some((key, Prior::New)));
    assert_eq!(open.cards[1].card.key.lemma, "brisk");
    assert!(!open.can_undo());
    assert_eq!(open.undo_known(), None);
}

#[test]
fn decks_ahead_follow_the_batches_still_coming() {
    let mut run = planned(0);
    assert_eq!(
        run.upcoming(2).iter().map(|b| b.from).collect::<Vec<_>>(),
        [3, 6]
    );
    let failed = |batch: Batch| Ahead::Failed {
        batch,
        why: String::new(),
        setup: None,
    };
    let (second, third) = (run.batches[1].clone(), run.batches[2].clone());
    run.ahead = vec![failed(second.clone()), failed(third.clone())];
    run.trim_ahead(1);
    assert!(
        run.has_ahead(&second) && !run.has_ahead(&third),
        "past the window"
    );
    run.unlock_through(2);
    assert!(run.take_ahead(&second).is_some(), "the gate takes its deck");
    run.ahead.push(failed(second));
    run.trim_ahead(2);
    assert!(
        run.ahead.is_empty(),
        "a batch now at the gate is no longer ahead"
    );
    assert!(!run.busy());
}

#[test]
fn a_new_recipe_forgets_the_decks_not_yet_open() {
    let mut run = planned(0);
    assert!(
        !run.follow("auto|b2".to_owned()),
        "the first recipe is only noted"
    );
    let first = run.batches[0].clone();
    run.deck = DeckState::Failed {
        batch: first,
        why: String::new(),
        setup: None,
    };
    run.fallback = Some(0);
    assert!(
        !run.follow("auto|b2".to_owned()),
        "the same recipe keeps all"
    );
    assert!(run.follow("ai|b2".to_owned()));
    assert!(matches!(run.deck, DeckState::None));
    assert_eq!(run.fallback, None);
}

fn built() -> Built {
    Built {
        cards: vec![shown("arcane")],
        found: 1,
        skipped_known: 0,
        via: Via::Dictionary,
    }
}

#[test]
fn a_deck_left_unfinished_is_offered_on_coming_back() {
    let mut run = planned(3);
    let batch = run.batches[1].clone();
    let mut open = Open::new(batch.clone(), built(), Some(5));
    open.away = true;
    run.deck = DeckState::Open(open);
    assert_eq!(run.arrived(2), None, "the first frame only notes the page");
    assert_eq!(run.arrived(3), Some(batch));
    assert_eq!(run.arrived(3), None, "asked once per return");
    assert!(run.reopen());
    assert!(run.shown_deck().is_some_and(|o| !o.away));
    assert!(!run.reopen(), "nothing left to open");
}

#[test]
fn a_deck_read_with_no_limit_opens_again_over_its_pages() {
    let mut run = planned(6);
    let batch = run.batches[1].clone();
    run.kept.push(Open::new(batch.clone(), built(), None));
    run.arrived(1);
    assert_eq!(run.arrived(4), Some(batch.clone()));
    assert!(run.reopen());
    assert_eq!(run.shown_deck().map(|o| o.batch.from), Some(3));
    assert_eq!(run.arrived(1), None, "no offer while one is open");
    assert!(run.close_review());
    assert_eq!(run.kept.len(), 1, "closing keeps it for next time");
}

/// A deck being made for `pages`, its writer `via`, not yet stepped.
fn making(pages: &Batch, via: Via) -> Box<DeckJob> {
    let ask = crate::study_mode::deck_job::Ask {
        document: document(),
        pages: (pages.from..=pages.to).collect(),
        level: 4,
        max: 10,
        known: Known::default(),
        to: None,
        via,
        writer: None,
    };
    Box::new(DeckJob::new(Arc::new(|| {}), ask))
}

#[test]
fn locked_pages_stand_as_their_decks_do() {
    use crate::study_mode::Stand::{Ready, Shut, Writing};
    let mut run = planned(3);
    let ai = |_: &Batch| Some(Via::Ai);
    assert_eq!(run.stands(false, ai), [(3..6, Shut), (6..9, Shut)]);
    let (second, third) = (run.batches[1].clone(), run.batches[2].clone());
    run.deck = DeckState::Open(Open::new(second.clone(), built(), None));
    let job = making(&third, Via::Ai);
    run.ahead.push(Ahead::Making { batch: third, job });
    assert_eq!(run.stands(false, ai), [(3..6, Ready), (6..9, Writing)]);
    run.ahead.clear();
    run.start = 2;
    let dictionary = run.stands(true, |_| Some(Via::Dictionary));
    assert_eq!(dictionary, [(0..2, Shut), (3..6, Ready), (6..9, Ready)]);
    let why = String::new();
    run.deck = DeckState::Failed {
        batch: second,
        why,
        setup: None,
    };
    let nobody = run.stands(false, |_| None);
    assert_eq!(nobody, [(3..6, Shut), (6..9, Shut)], "failed, or nobody");
    let unplanned = Run::new(Mode::Proactive, None, 4);
    assert_eq!(unplanned.stands(false, ai), [(4..usize::MAX, Shut)]);
}

/// The sample book planned in words mode, `target` words a batch.
fn by_words(document: &Arc<Document>, target: usize) -> Sheet<'_> {
    Sheet {
        size: Size {
            unit: Unit::Words,
            pages: 3,
            words: target,
        },
        ..sheet(document, None, 0)
    }
}

#[test]
fn words_mode_counts_the_pages_before_it_plans_by_their_words() {
    let doc = document();
    let mut run = Run::new(Mode::Proactive, None, 0);
    let size = by_words(&doc, 4).size;
    assert_eq!(run.plan(by_words(&doc, 4)), Ok(false), "counting first");
    assert_eq!((run.planned(), run.gate()), (false, Some(0)));
    assert!(run.wants_count(size));
    let none = Known::default();
    while run.count(size, &mut Lengths(true), &none, 3) {}
    assert!(!run.wants_count(size) && !run.counting());
    assert_eq!(run.plan(by_words(&doc, 4)), Ok(true));
    let b = &run.batches;
    assert_eq!((b[0].from, b[b.len() - 1].to), (0, PAGES - 1));
    assert!(b.windows(2).all(|w| w[1].from == w[0].to + 1));
    assert!(b.iter().any(|b| b.words > 0), "the sample has long words");
}

#[test]
fn words_mode_without_the_word_data_plans_by_pages() {
    let doc = document();
    let mut run = Run::new(Mode::Proactive, None, 0);
    let size = by_words(&doc, 4).size;
    assert_eq!(run.plan(by_words(&doc, 4)), Ok(false));
    assert!(run.count(size, &mut Lengths(false), &Known::default(), 3));
    assert_eq!(run.plan(by_words(&doc, 4)), Ok(true));
    let spans: Vec<_> = run.batches.iter().map(|b| (b.from, b.to)).collect();
    assert_eq!(spans, [(0, 2), (3, 5)]);
}
