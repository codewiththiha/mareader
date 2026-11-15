//! Searches asked of books: indexed on a thread, or here for a test.

use std::sync::Arc;

use super::*;
use crate::sample;
use crate::testing::{TRIES, WAIT, asked, waited, wakes};

/// Polls until `doc`'s search has every page in, a second at most.
fn done(search: &mut Search, doc: DocId, book: &Arc<Document>) -> bool {
    for _ in 0..TRIES {
        search.poll();
        if search.found(doc, book).is_some_and(Found::done) {
            return true;
        }
        std::thread::sleep(WAIT);
    }
    search.found(doc, book).is_some_and(Found::done)
}

/// A search that builds and answers here, and a `Waker` that counts frames.
fn here() -> (Search, Waker) {
    let mut search = Search::default();
    search.set_inline(true);
    let (wake, _) = asked();
    (search, wake)
}

#[test]
fn the_first_ask_indexes_the_book_and_finds_the_query() {
    let (doc, document) = sample::book(3).unwrap();
    let (mut search, wake) = here();
    search.ask(wake, doc, &document, "Sphinx");
    assert!(done(&mut search, doc, &document), "every page is in");
    let found = search.found(doc, &document).expect("the search is kept");
    assert_eq!(found.query, "Sphinx");
    assert_eq!(found.hits.len(), 3, "one hit on each of three pages");
    assert!(!found.more, "fewer hits than the most kept");
    assert_eq!(found.indexed, found.pages, "the whole book was read");
}

#[test]
fn asking_the_same_query_again_keeps_what_was_found() {
    let (doc, document) = sample::book(2).unwrap();
    let (mut search, wake) = here();
    search.ask(Arc::clone(&wake), doc, &document, "quartz");
    assert!(done(&mut search, doc, &document), "every page is in");
    let before = search.found(doc, &document).expect("kept").clone();
    search.ask(Arc::clone(&wake), doc, &document, "quartz");
    let after = search.found(doc, &document).expect("kept");
    assert_eq!(after, &before, "the hits found first are kept");
    assert_eq!(search.len(), 1, "one book asked twice is one search");
}

#[test]
fn a_new_query_replaces_the_hits_the_last_one_found() {
    let (doc, document) = sample::book(3).unwrap();
    let (mut search, wake) = here();
    search.ask(Arc::clone(&wake), doc, &document, "Sphinx");
    assert!(done(&mut search, doc, &document), "every page is in");
    search.ask(Arc::clone(&wake), doc, &document, "sideways");
    assert!(done(&mut search, doc, &document), "every page is in");
    let found = search.found(doc, &document).expect("kept");
    assert_eq!(found.query, "sideways", "the hits answer the last ask");
    assert_eq!(found.hits.len(), 3, "one hit on each of three pages");
}

#[test]
fn a_query_that_is_not_in_the_book_finds_nothing() {
    let (doc, document) = sample::book(2).unwrap();
    let (mut search, wake) = here();
    search.ask(wake, doc, &document, "zzzqqq");
    assert!(done(&mut search, doc, &document), "every page is in");
    let found = search.found(doc, &document).expect("kept");
    assert!(found.hits.is_empty(), "nothing in the book matches");
    assert!(!found.more, "nothing was cut, for nothing was found");
}

#[test]
fn only_the_last_few_books_searched_are_kept() {
    let books = sample::books(4, 2).unwrap();
    let (mut search, wake) = here();
    for (doc, document) in &books {
        search.ask(Arc::clone(&wake), *doc, document, "Sphinx");
    }
    assert_eq!(search.len(), 3, "the least recently searched went");
    let (gone, older) = books.first().expect("four books opened");
    let dropped = search.found(*gone, older);
    assert!(dropped.is_none(), "the book first asked is forgotten");
    let (kept, newest) = books.last().expect("four books opened");
    assert!(done(&mut search, *kept, newest), "the last asked is kept");
}

#[test]
fn closing_a_book_forgets_its_search_and_its_thread() {
    let (doc, document) = sample::book(2).unwrap();
    let (mut search, wake) = here();
    search.ask(wake, doc, &document, "Sphinx");
    search.forget_closed(|id| (id == doc).then_some(&document));
    assert_eq!(search.len(), 1, "an open book's search is kept");
    search.forget_closed(|_| None);
    assert!(search.is_empty(), "a closed book's search went");
}

#[test]
fn a_search_is_keyed_by_the_book_it_reads_not_its_id() {
    let (doc, one) = sample::book(2).unwrap();
    let (_, other) = sample::book(2).unwrap();
    let (mut search, wake) = here();
    search.ask(wake, doc, &one, "Sphinx");
    assert!(done(&mut search, doc, &one), "every page is in");
    let found = search.found(doc, &other);
    assert!(found.is_none(), "a book set again is another search");
}

#[test]
fn the_index_builds_off_the_frame_and_asks_for_one() {
    let (doc, document) = sample::book(3).unwrap();
    let mut search = Search::default();
    let (wake, kept) = asked();
    search.ask(wake, doc, &document, "Sphinx");
    assert!(done(&mut search, doc, &document), "every page is in");
    let found = search.found(doc, &document).expect("kept");
    assert_eq!(found.query, "Sphinx", "the thread answered the ask");
    assert!(waited(|| wakes(&kept) > 0), "a frame was asked for");
}
