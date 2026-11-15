//! A right-to-left book with ruby and runs in their own family.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::{Block, Content, Dir, Text, Typography};

const DIRECTION: &[u8] = include_bytes!("books/direction.epub");

fn open() -> Book {
    Book::open(Arc::new(DIRECTION.to_vec()), &Typography::default()).unwrap()
}

/// The first text block holding `words`, and its frame's direction.
fn block<'a>(book: &'a Book, words: &str) -> (&'a Text, Dir) {
    book.blocks()
        .iter()
        .find_map(|b: &Block| match &b.content {
            Content::Text(t) if t.text.contains(words) => Some((t, b.frame.dir)),
            _ => None,
        })
        .unwrap()
}

fn page_text(book: &Book, page: usize) -> String {
    let lines = book.text(page).into_iter().flat_map(|l| l.chars);
    lines.map(|c| c.ch).collect()
}

#[test]
fn the_spine_turns_its_pages_right_to_left() {
    let book = open();
    assert_eq!(book.title(), Some("Directions and Notes"));
    assert!(book.right_to_left());
}

#[test]
fn direction_comes_from_the_root_and_each_element() {
    let book = open();
    assert_eq!(block(&book, "هذا كتاب").1, Dir::Rtl);
    assert_eq!(block(&book, "זהו").1, Dir::Rtl);
    assert_eq!(block(&book, "An English").1, Dir::Ltr);
    // The Japanese chapter names no direction.
    assert_eq!(block(&book, "新幹線").1, Dir::Auto);
}

#[test]
fn right_to_left_lines_start_at_the_right() {
    let book = open();
    let line = |words: &str| {
        let lines = (0..book.page_count()).flat_map(|p| book.text(p));
        lines
            .map(|l| l.chars)
            .find(|c| c.iter().map(|c| c.ch).collect::<String>().contains(words))
            .unwrap()
    };
    let span = |chars: &[mareader_reflow::PageChar]| {
        let left = chars.iter().map(|c| c.rect[0]);
        let right = chars.iter().map(|c| c.rect[0] + c.rect[2]);
        (left.fold(f32::MAX, f32::min), right.fold(0.0, f32::max))
    };
    let (short, english) = (span(&line("الثالث")), span(&line("An English")));
    // A short item ends at the right, its list indent aside.
    assert!(short.0 > english.0 + 200.0, "{short:?} {english:?}");
    assert!((short.1 - english.1).abs() < 70.0, "{short:?} {english:?}");
    // Aligned `left`, a right-to-left line keeps to the left.
    let sided = span(&line("الأيسر"));
    assert!((sided.0 - english.0).abs() < 2.0, "{sided:?} {english:?}");
}

#[test]
fn ruby_notes_sit_over_their_base_and_stay_out_of_the_text() {
    let book = open();
    let (text, _) = block(&book, "漢字");
    let notes: Vec<(&str, &str)> = text
        .ruby
        .iter()
        .map(|a| (&text.text[a.range.clone()], a.text.as_str()))
        .collect();
    assert_eq!(notes, [("漢字", "かんじ"), ("読", "よ"), ("方", "かた")]);
    // Fallback parentheses are hidden.
    assert!(!text.text.contains('（'), "{}", text.text);
    let (heading, _) = block(&book, "振");
    let marks: Vec<&str> = heading.ruby.iter().map(|a| a.text.as_str()).collect();
    assert_eq!(
        (heading.text.as_str(), marks),
        ("振仮名", vec!["ふ", "が", "な"])
    );
    let (latin, _) = block(&book, "Ruby works");
    let base = latin.ruby.first().map(|a| &latin.text[a.range.clone()]);
    assert_eq!(base, Some("EPUB"));
    let pages: String = (0..book.page_count())
        .map(|p| page_text(&book, p))
        .collect();
    assert!(pages.contains("東京") && !pages.contains("とうきょう"));
}

#[test]
fn a_run_in_another_family_gets_its_own_face() {
    let book = open();
    let (text, _) = block(&book, "a run in");
    let faced: Vec<&Vec<String>> = text
        .spans
        .iter()
        .filter(|s| s.emphasis.face > 0)
        .filter_map(|s| text.faces.get(usize::from(s.emphasis.face) - 1))
        .collect();
    assert_eq!(faced.len(), 1, "{:?}", text.spans);
    assert!(faced[0].iter().any(|n| n.contains("mono")), "{faced:?}");
    // Used only by that run, its font still loads.
    let families: Vec<&str> = book.faces().iter().map(|f| f.family.as_str()).collect();
    for name in ["book arabic", "book japanese", "book mono"] {
        assert!(families.iter().any(|f| f.ends_with(name)), "{families:?}");
    }
}

#[test]
fn every_page_draws() {
    crate::common::every_page_draws(&open());
}
