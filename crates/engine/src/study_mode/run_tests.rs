//! A pane's run: held before planning, planned on a real PDF, its gate and
//! the known cards' trail.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::sync::Arc;

use mareader_reflow::Typography;
use mareader_study::mode::{Card, Unit};

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
        chars: 0,
    }
}

fn sheet(document: &Arc<Document>, saved: Option<usize>, current: usize) -> Sheet<'_> {
    Sheet {
        document,
        wake: Arc::new(|| {}),
        inline: true,
        size: size(3),
        saved,
        current,
    }
}

/// A run planned by hand on nine pages of three-page batches.
fn planned(unlocked: usize) -> Run {
    let mut run = Run::new(Mode::Proactive, None, 0);
    run.sizes = vec![400; 9];
    run.batches = plan(&run.sizes, size(3));
    run.layout = Some((None, 9));
    run.size = Some(size(3));
    run.unlocked = unlocked;
    run
}

fn shown(lemma: &str) -> Shown {
    Shown {
        card: Card {
            key: Key::new(lemma, None),
            form: lemma.to_owned(),
            sentence: String::new(),
            band: 5,
            count: 1,
            also: Vec::new(),
        },
        meaning: String::new(),
    }
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
    assert_eq!(done.gate(), None);
    assert!(done.gate_batch().is_none());
}

#[test]
fn the_gate_follows_unlocking_and_the_batch_before_it_reads_again() {
    let mut run = planned(3);
    assert_eq!(run.gate(), Some(3));
    assert_eq!(run.gate_batch().map(|b| (b.from, b.to)), Some((3, 5)));
    assert_eq!(run.previous().map(|b| (b.from, b.to)), Some((0, 2)));
    run.unlock_through(5);
    assert_eq!(run.gate(), Some(6));
    run.unlock_through(8);
    assert_eq!(run.gate(), None);
    assert!(planned(0).previous().is_none());
}

#[test]
fn a_deck_for_a_batch_no_longer_at_the_gate_is_dropped() {
    let mut run = planned(3);
    let first = run.batches[0].clone();
    run.deck = DeckState::Failed {
        batch: first,
        why: String::new(),
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
    };
    let batch = planned(0).batches[0].clone();
    let mut open = Open::new(batch, built, 6, 0.0);
    assert_eq!(open.floor.left(0.0), 18.0);

    let key = Key::new("brisk", None);
    let (at, taken) = open.take_known(&key).unwrap();
    assert_eq!(at, 1);
    open.keep_known(at, taken, Prior::New);
    assert_eq!(open.cards.len(), 2);
    assert_eq!(open.floor.left(0.0), 12.0);
    assert!(open.take_known(&key).is_none());

    assert_eq!(open.undo_known(), Some((key, Prior::New)));
    assert_eq!(open.cards[1].card.key.lemma, "brisk");
    assert_eq!(open.floor.left(0.0), 18.0);
    assert!(!open.can_undo());
    assert_eq!(open.undo_known(), None);
}
