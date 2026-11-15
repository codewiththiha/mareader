//! Results set as pages: the size they settle at, and the thread that sets.

use std::sync::Arc;

use super::*;
use crate::sample;
use mareader_reflow::Role;

/// The column a result is set in, and the pixels per page unit.
const ROOM: (f32, f32) = (300.0, 400.0);
const SCALE: f32 = 2.0;

/// A paragraph long enough to need the column it is set in.
fn sentence(at: usize) -> String {
    format!("A paragraph of words that sets in the column, number {at}.")
}

/// A job of `count` paragraphs, drawn over `pages`, showing `part` of them.
fn job(count: usize, pages: usize, part: usize) -> Job {
    let blocks = (0..count)
        .map(|at| Block::text(Role::Paragraph, &sentence(at)))
        .collect();
    Job {
        blocks: Arc::new(blocks),
        typo: Typography::default(),
        room: ROOM,
        scale: SCALE,
        book: None,
        pages,
        part,
        note: None,
    }
}

#[test]
fn a_result_is_set_in_the_room_it_was_given() {
    let made = set(&job(3, 4, 0)).expect("three paragraphs set");
    assert_eq!(made.size.0, ROOM.0, "the column keeps its width");
    assert!(!made.pixels.rgba.is_empty(), "the page drew");
    assert!(made.pdf, "without a book the result is a PDF's");
}

#[test]
fn a_result_drawn_over_a_book_is_not_a_pdf_s() {
    let (_, document) = sample::book(1).unwrap();
    let mut one = job(3, 4, 0);
    one.book = Some(document);
    let made = set(&one).expect("three paragraphs set");
    assert!(!made.pdf, "a book's result is drawn over the page");
}

#[test]
fn a_page_past_the_result_draws_the_note_instead() {
    let without = set(&job(1, 4, 9));
    assert!(without.is_none(), "past the end nothing is drawn");
    let mut one = job(1, 4, 9);
    let note = vec![Block::text(Role::Paragraph, "The rest is in the book.")];
    one.note = Some(Arc::new(note));
    assert!(set(&one).is_some(), "past the end the note is drawn");
}

#[test]
fn a_result_too_long_for_its_pages_shrinks_to_fit_them() {
    let short = set(&job(2, 1, 0)).expect("two paragraphs set");
    let long = set(&job(60, 1, 0)).expect("sixty paragraphs set");
    assert!(long.size.1 > short.size.1, "one page takes the rest");
    assert_eq!(long.size.0, ROOM.0, "the column's width is kept");
}

#[test]
fn a_worker_starts_only_where_threads_do() {
    let threads = !cfg!(target_arch = "wasm32");
    assert_eq!(worker().is_some(), threads, "a worker starts where it can");
}

#[test]
fn the_worker_sets_what_it_is_asked_and_names_the_slot() {
    let Some((jobs, news)) = worker() else {
        return;
    };
    jobs.send((7, 3, job(2, 4, 0))).expect("the worker listens");
    let done = news.recv().expect("the worker sends back what it set");
    assert_eq!(done.slot, 7, "the slot the job was asked for");
    assert_eq!(done.key, 3, "the key the job was asked under");
    assert!(done.set.is_some(), "the job set a page");
    drop(jobs);
    assert!(news.recv().is_err(), "the worker goes when the asks do");
}
