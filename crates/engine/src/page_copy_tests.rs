//! Pages copied off the frame, and the toast each copy reports.
//!
//! Four pages or fewer, and any reflowable book, are read in the frame that
//! asked; a whole PDF is read on a helper thread, so a test waits on the copy
//! rather than on a count of frames.

use std::sync::Arc;

use super::*;
use crate::sample;
use crate::testing::{TRIES, WAIT, asked, waited, wakes};

/// The copy that lands, waiting on one read off the frame.
fn landed(copy: &mut PageCopy) -> Option<Read> {
    for _ in 0..TRIES {
        if let Some(read) = copy.poll() {
            return Some(read);
        }
        std::thread::sleep(WAIT);
    }
    None
}

#[test]
fn a_page_or_fewer_is_copied_in_the_frame_that_asked() {
    let (_, document) = sample::book(2).unwrap();
    let mut copy = PageCopy::default();
    copy.start(Arc::clone(&document), vec![0]);
    let read = copy.poll().expect("a page or fewer is read here");
    assert!(read.text.contains("Page 1 of 2"), "the page is in it");
    assert_eq!(read.toast.message, "Copied the page's text");
    assert!(copy.poll().is_none(), "one copy lands once");
}

#[test]
fn a_whole_book_is_copied_off_the_frame_that_asked() {
    let (_, document) = sample::book(6).unwrap();
    let (wake, kept) = asked();
    let mut copy = PageCopy::default();
    copy.attach(wake);
    copy.start(Arc::clone(&document), (0..6).collect());
    let read = landed(&mut copy).expect("a book's copy lands");
    assert_eq!(read.toast.message, "Copied the text of 6 pages");
    let found = read.text.matches("Sphinx of black quartz").count();
    assert_eq!(found, 6, "every page's text is in the copy");
    assert!(waited(|| wakes(&kept) > 0), "a frame was asked for");
}

#[test]
fn pages_without_text_say_that_there_is_nothing_to_copy() {
    let (_, document) = sample::book(1).unwrap();
    let mut copy = PageCopy::default();
    copy.start(Arc::clone(&document), vec![40]);
    let read = copy.poll().expect("a page or fewer is read here");
    assert!(read.text.is_empty(), "page 40 of one page has no text");
    assert_eq!(read.toast.message, "No text to copy on these pages");
}

#[test]
fn a_newer_copy_replaces_the_one_being_read() {
    let (_, document) = sample::book(2).unwrap();
    let mut copy = PageCopy::default();
    copy.start(Arc::clone(&document), vec![1]);
    copy.start(Arc::clone(&document), vec![0]);
    let read = copy.poll().expect("a page or fewer is read here");
    assert!(read.text.contains("Page 1 of 2"), "the newer is kept");
    assert!(!read.text.contains("Page 2 of 2"), "the older one went");
    assert_eq!(read.toast.message, "Copied the page's text");
}
