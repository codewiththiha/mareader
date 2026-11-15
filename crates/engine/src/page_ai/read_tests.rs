//! A book's pages read off the frame, one at a time, and a group as one.

use std::sync::Arc;

use super::*;
use crate::sample;
use crate::testing::{TRIES, WAIT, asked, waited, wakes};

/// Every page the run sends, waiting at most a second for the last of them.
fn landed(got: &Receiver<Read>, count: usize) -> Vec<Read> {
    let mut reads = Vec::new();
    for _ in 0..TRIES {
        while let Ok(read) = got.try_recv() {
            reads.push(read);
        }
        if reads.len() >= count {
            return reads;
        }
        std::thread::sleep(WAIT);
    }
    reads
}

#[test]
fn a_page_read_in_the_frame_comes_back_with_its_text() {
    let (_, document) = sample::book(2).unwrap();
    let (wake, _) = asked();
    let got = pages(wake, Arc::clone(&document), vec![0], true);
    let (at, found) = got.try_recv().expect("an inline read is done");
    assert_eq!(at, 0);
    let (source, span) = found.expect("the sample's first page has text");
    assert!(!source.is_blank(), "the page's text is in the source");
    assert!(matches!(span, PageSpan::Page { page } if page == 0));
}

#[test]
fn a_page_beyond_the_book_comes_back_without_text() {
    let (_, document) = sample::book(2).unwrap();
    let (wake, _) = asked();
    let got = pages(wake, Arc::clone(&document), vec![40], true);
    let (at, found) = got.try_recv().expect("an inline read is done");
    assert_eq!(at, 40, "the page asked for is the one answered");
    assert!(found.is_none(), "a page with no text has nothing in it");
}

#[test]
fn pages_read_off_the_frame_arrive_in_order_and_ask_for_one() {
    let (_, document) = sample::book(3).unwrap();
    let (wake, kept) = asked();
    let got = pages(wake, Arc::clone(&document), vec![0, 1, 2], false);
    let reads = landed(&got, 3);
    let order: Vec<usize> = reads.iter().map(|(at, _)| *at).collect();
    assert_eq!(order, vec![0, 1, 2], "in the order asked for");
    let every = reads.iter().all(|(_, found)| found.is_some());
    assert!(every, "each page read came back with its text");
    assert!(waited(|| wakes(&kept) >= 3), "a frame was asked for each");
}

#[test]
fn a_group_of_pages_is_read_as_one_source() {
    let (_, document) = sample::book(4).unwrap();
    let (wake, kept) = asked();
    let got = group(wake, Arc::clone(&document), (1, 2), true);
    let found = got.try_recv().expect("an inline group is done");
    let source = found.expect("the group's pages have text");
    assert!(!source.is_blank(), "the group's text is in it");
    assert!(waited(|| wakes(&kept) > 0), "a frame was asked for");
}

#[test]
fn a_group_beyond_the_book_is_read_as_without_text() {
    let (_, document) = sample::book(2).unwrap();
    let (wake, _) = asked();
    let got = group(wake, Arc::clone(&document), (40, 41), true);
    let found = got.try_recv().expect("an inline group is done");
    assert!(found.is_none(), "no page in it has any text to read");
}
