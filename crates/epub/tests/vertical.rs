//! A Japanese chapter written in columns down the page, then a level one.
use std::sync::Arc;

use mareader_epub::Book;
use mareader_reflow::{Block, Content, Orient, PageChar, Typography};

const VERTICAL: &[u8] = include_bytes!("books/vertical.epub");

fn open() -> Book {
    Book::open(Arc::new(VERTICAL.to_vec()), &Typography::default()).unwrap()
}

/// The first block whose text holds `words`.
fn block<'a>(book: &'a Book, words: &str) -> &'a Block {
    let mut blocks = book.blocks().iter();
    blocks
        .find(|b| matches!(&b.content, Content::Text(t) if t.text.contains(words)))
        .unwrap()
}

/// The first page and line holding `words`.
fn line(book: &Book, words: &str) -> (usize, Vec<PageChar>) {
    (0..book.page_count())
        .find_map(|p| {
            let lines = book.text(p).into_iter().map(|l| l.chars);
            let mut found =
                lines.filter(|c| c.iter().map(|c| c.ch).collect::<String>().contains(words));
            found.next().map(|c| (p, c))
        })
        .unwrap()
}

#[test]
fn a_vertical_book_turns_its_pages_right_to_left() {
    let book = open();
    assert_eq!(book.title(), Some("Columns Down the Page"));
    // The spine names no direction; the writing decides.
    assert!(book.right_to_left());
}

#[test]
fn only_the_vertical_chapter_turns() {
    let book = open();
    assert!(block(&book, "こんにちは").frame.turned);
    assert!(!block(&book, "left level").frame.turned);
    let (first, _) = line(&book, "こんにちは");
    let (last, _) = line(&book, "left level");
    assert!(book.turned(first));
    assert!(!book.turned(last));
}

#[test]
fn orientation_and_combined_digits_are_kept() {
    let book = open();
    assert_eq!(block(&book, "全部立てる").frame.orient, Orient::Upright);
    assert_eq!(block(&book, "こんにちは").frame.orient, Orient::Mixed);
    let Content::Text(text) = &block(&book, "こんにちは").content else {
        unreachable!()
    };
    let combined: Vec<&str> = text.combine.iter().map(|r| &text.text[r.clone()]).collect();
    assert_eq!(combined, ["10", "21"]);
}

#[test]
fn vertical_lines_run_down_and_columns_go_left() {
    let book = open();
    let (page, chars) = line(&book, "こんにちは");
    let (lefts, tops): (Vec<f32>, Vec<f32>) = chars.iter().map(|c| (c.rect[0], c.rect[1])).unzip();
    // One column: its letters share an x and step downward.
    let spread = lefts
        .iter()
        .fold(0.0f32, |m, x| m.max((x - lefts[0]).abs()));
    assert!(spread < 1.0, "{lefts:?}");
    assert!(tops.windows(2).all(|w| w[1] >= w[0]), "{tops:?}");
    // Each column on the page sits left of the one before.
    let columns: Vec<f32> = book
        .text(page)
        .iter()
        .filter_map(|l| l.chars.first().map(|c| c.rect[0]))
        .collect();
    assert!(columns.len() > 3, "{columns:?}");
    assert!(columns.windows(2).all(|w| w[1] < w[0]), "{columns:?}");
    let (width, _) = book.page_size();
    assert!(columns[0] > width / 2.0, "{columns:?}");
}

#[test]
fn a_tall_picture_stays_tall() {
    let book = open();
    let pictures = (0..book.page_count()).flat_map(|p| book.pictures(p));
    let tower = pictures.max_by(|a, b| a[3].total_cmp(&b[3])).unwrap();
    assert!(tower[3] > tower[2] * 1.5, "{tower:?}");
}

#[test]
fn the_long_chapter_fills_several_pages() {
    let book = open();
    let turned = (0..book.page_count()).filter(|&p| book.turned(p)).count();
    assert!(turned >= 2, "{turned}");
    let (width, height) = book.page_size();
    for page in 0..book.page_count() {
        for c in book.text(page).iter().flat_map(|l| &l.chars) {
            let [x, y, w, h] = c.rect;
            assert!(
                x >= 0.0 && x + w <= width && y >= 0.0 && y + h <= height,
                "{page} {c:?}"
            );
        }
    }
}

#[test]
fn every_page_draws() {
    crate::common::every_page_draws(&open());
}
