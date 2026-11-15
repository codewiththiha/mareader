//! `epub:trigger`: clicks that show, hide and play other elements.
use std::collections::BTreeMap;
use std::sync::Arc;

use mareader_epub::{Action, Book, Destination, Inserts, Triggered};
use mareader_reflow::{Block, Content, Typography};

use crate::common::text;

const BOOK: &[u8] = include_bytes!("books/triggers.epub");
const CHIME: &str = "/OEBPS/media/chime.mp3";

fn open() -> Book {
    Book::open(Arc::new(BOOK.to_vec()), &Typography::default()).unwrap()
}

/// What each trigger link on the pages sets off, in page order.
fn triggers(book: &Book) -> Vec<Vec<Triggered>> {
    let pages = 0..book.page_count();
    let mut seen: Vec<usize> = Vec::new();
    for link in pages.flat_map(|p| book.links(p)) {
        if let Destination::Trigger(n) = link.to
            && !seen.contains(&n)
        {
            seen.push(n);
        }
    }
    seen.into_iter().map(|n| book.trigger(n)).collect()
}

/// `book` laid with each target in `shown` as it says.
fn with(book: &Book, shown: &[(usize, bool)]) -> Book {
    let shown: BTreeMap<_, _> = shown.iter().copied().collect();
    let list = book.triggered(&shown).into_iter().map(Arc::new).collect();
    let inserts = Arc::new(Inserts { key: 1, list });
    book.reset(&Typography::default(), Some(&inserts)).unwrap()
}

/// The book's block holding `needle`, and where in it.
fn find<'b>(blocks: &'b [Block], needle: &str) -> (&'b Block, std::ops::Range<usize>) {
    for b in blocks {
        if let Content::Text(t) = &b.content
            && let Some(at) = t.text.find(needle)
        {
            return (b, at..at + needle.len());
        }
    }
    panic!("no block holds {needle:?}")
}

/// Whether every byte of `range` is hidden in `block`, or none is.
fn hidden(block: &Block, range: std::ops::Range<usize>) -> Option<bool> {
    let Content::Text(t) = &block.content else {
        return None;
    };
    let covered = |i: usize| (t.spans.iter()).any(|s| s.range.contains(&i) && s.emphasis.hidden);
    let all = range.clone().all(covered);
    let none = !range.clone().any(covered);
    (all || none).then_some(all)
}

fn chime(action: Action) -> Triggered {
    Triggered::Clip {
        action,
        path: CHIME.to_owned(),
        video: false,
    }
}

#[test]
fn clicks_on_observers_set_off_their_triggers() {
    let book = open();
    let got = triggers(&book);
    let sea = Triggered::Clip {
        action: Action::Play,
        path: "/OEBPS/media/sea.mp4".to_owned(),
        video: true,
    };
    let want = vec![
        vec![Triggered::Show(0, true)],
        vec![Triggered::Show(0, false)],
        vec![Triggered::Show(1, false)],
        vec![Triggered::Show(2, true)],
        vec![Triggered::Show(3, false)],
        vec![chime(Action::Play)],
        vec![chime(Action::Mute), Triggered::Show(4, true)],
        vec![sea],
        vec![chime(Action::Play)],
        vec![Triggered::Show(5, true)],
        vec![Triggered::Show(6, true)],
        vec![Triggered::Show(7, false)],
        vec![Triggered::Show(8, true)],
        vec![Triggered::Show(9, true)],
        vec![Triggered::Show(10, true)],
        vec![Triggered::Show(11, true)],
    ];
    assert_eq!(got, want);
    // A hidden sound has no card; the hover trigger makes no link.
    let all = text(&book);
    assert!(!all.contains("Play the audio"), "{all}");
    assert!(all.contains("Play the video: Waves"), "{all}");
}

#[test]
fn a_hidden_answer_is_laid_in_place_once_shown() {
    let book = open();
    assert!(!text(&book).contains("forty-two"));
    let all = text(&with(&book, &[(0, true)]));
    let answer = all.find("The answer is forty-two.").expect(&all);
    assert!(all.find("Hide answer").unwrap() < answer);
    assert!(answer < all.find("After the answer.").unwrap());
    // Styled as the chapter's other prose is.
    let ins = book.triggered(&BTreeMap::from([(0, true)]));
    let answer = ins[0].blocks.first().unwrap();
    let (after, _) = find(book.blocks(), "After the answer.");
    assert_eq!(answer.frame.look, after.frame.look);
    // Asked to stay as the book opens: nothing to lay.
    assert!(book.triggered(&BTreeMap::from([(0, false)])).is_empty());
    let all = text(&with(&book, &[(0, true), (0, false)]));
    assert!(!all.contains("forty-two"), "{all}");
}

#[test]
fn hiding_keeps_the_room_and_showing_gives_it_back() {
    let book = open();
    let blocks = book.blocks();
    let (hint, range) = find(blocks, "try counting sevens");
    assert_eq!(hidden(hint, range.clone()), Some(false));
    let ins = book.triggered(&BTreeMap::from([(1, true), (1, false)]));
    let laid = ins[0].blocks.first().unwrap();
    assert_eq!(hidden(laid, range.clone()), Some(true));
    // The words around it stay, with the same text: lines don't move.
    let (Content::Text(a), Content::Text(b)) = (&hint.content, &laid.content) else {
        panic!("not text");
    };
    assert_eq!(a.text, b.text);
    // A collapsed space before it may go with it; it looks the same.
    assert_eq!(hidden(laid, 0..range.start - 1), Some(false));
    // An inline `display: none` takes no room until it shows.
    assert!(!text(&book).contains("read slowly"));
    let ins = book.triggered(&BTreeMap::from([(2, true)]));
    let (tip, range) = find(&ins[0].blocks, "read slowly");
    assert_eq!(hidden(tip, range), Some(false));
    // The book's own `visibility: hidden` starts hidden too.
    let (muted, range) = find(blocks, "(muted)");
    assert_eq!(hidden(muted, range), Some(true));
}

#[test]
fn a_hidden_box_fades_with_its_text() {
    let book = open();
    let (boxed, range) = find(book.blocks(), "A boxed note.");
    let own = boxed.frame.boxes.len();
    assert!(own > 0);
    let ins = book.triggered(&BTreeMap::from([(3, false)]));
    let laid = ins[0].blocks.first().unwrap();
    assert_eq!(hidden(laid, range), Some(true));
    let faded = laid.frame.boxes.last().unwrap();
    let opacity = faded.finish.as_ref().map_or(1.0, |f| f.opacity);
    assert!(opacity < 0.01, "{opacity}");
}

#[test]
fn a_hidden_ending_shows_after_its_chapter_not_before_the_next() {
    let book = open();
    let pages = book.page_count();
    let shown = with(&book, &[(5, true)]);
    let all = text(&shown);
    let ending = all.find("The ending, revealed.").expect(&all);
    assert!(all.find("Reveal the ending").unwrap() < ending);
    assert_eq!(shown.page_count(), pages);
}

/// The book's pictures that draw, and those that only keep their room.
fn pictures(blocks: &[Block]) -> (usize, usize) {
    let images = blocks
        .iter()
        .filter(|b| matches!(b.content, Content::Image { .. }));
    let (hidden, shown): (Vec<_>, Vec<_>) = images.partition(|b| b.frame.hidden);
    (shown.len(), hidden.len())
}

#[test]
fn a_hidden_table_row_shows_in_its_table() {
    let book = open();
    assert!(!text(&book).contains("tide"));
    let rows = |blocks: &[Block]| {
        let tables = blocks.iter().filter_map(|b| match &b.content {
            Content::Table(t) => Some(t.rows.len()),
            _ => None,
        });
        tables.collect::<Vec<_>>()
    };
    // A `display: none` row is left out, not laid blank.
    assert_eq!(rows(book.blocks()), [2]);
    let ins = book.triggered(&BTreeMap::from([(6, true)]));
    assert_eq!(rows(&ins[0].blocks), [3]);
    let shown = with(&book, &[(6, true)]);
    assert!(text(&shown).contains("the sea rising"));
    assert_eq!(shown.page_count(), book.page_count());
}

#[test]
fn rules_and_pictures_hide_and_keep_their_room() {
    let book = open();
    let rule = |blocks: &[Block]| {
        let rules = blocks.iter().filter(|b| matches!(b.content, Content::Rule));
        rules.map(|b| b.frame.hidden).collect::<Vec<_>>()
    };
    assert_eq!(rule(book.blocks()), [false]);
    let ins = book.triggered(&BTreeMap::from([(7, false)]));
    assert_eq!(ins.len(), 1);
    assert_eq!(rule(&ins[0].blocks), [true]);
    // The book's `visibility: hidden` picture keeps its room, undrawn.
    assert_eq!(pictures(book.blocks()).1, 1);
    let ins = book.triggered(&BTreeMap::from([(8, true)]));
    assert_eq!(pictures(&ins[0].blocks), (1, 0));
    let hidden = book.blocks().iter().find(|b| b.frame.hidden);
    let Some(Content::Image { links, .. }) = hidden.map(|b| &b.content) else {
        panic!("no hidden picture");
    };
    assert!(links.is_empty(), "a hidden picture leads nowhere");
    // A `display: none` picture with no content shows once asked.
    let ins = book.triggered(&BTreeMap::from([(9, true)]));
    let added: usize = ins.iter().map(|i| pictures(&i.blocks).0).sum();
    assert_eq!(added, 1);
}

#[test]
fn a_small_picture_in_a_line_sets_off_its_trigger() {
    let book = open();
    let note = |b: &Book| text(b).contains("Here is the note.");
    assert!(!note(&book));
    let pages = 0..book.page_count();
    let tiny = pages.flat_map(|p| book.links(p)).find_map(|l| match l.to {
        Destination::Trigger(n) if book.trigger(n) == [Triggered::Show(10, true)] => Some(l),
        _ => None,
    });
    let tiny = tiny.expect("the picture is a link");
    // A box the size of one line's picture, not the paragraph.
    let [_, _, w, h] = tiny.rect;
    assert!(w > 0.0 && w < 40.0 && h < 40.0, "{:?}", tiny.rect);
    assert!(note(&with(&book, &[(10, true)])));
}

#[test]
fn a_fixed_page_shows_its_target_on_the_same_page() {
    let book = open();
    let pages = book.page_count();
    let shown = with(&book, &[(11, true)]);
    let all = text(&shown);
    let note = all.find("The fixed note.").expect(&all);
    assert!(all.find("Show the fixed note").unwrap() < note);
    assert!(note < all.find("Below the note.").unwrap());
    assert_eq!(shown.page_count(), pages);
    // The page is laid again whole, its blocks on one sheet.
    let ins = book.triggered(&BTreeMap::from([(11, true)]));
    let sheets = ins[0].blocks.iter().map(|b| b.frame.sheet.is_some());
    assert!(sheets.into_iter().all(|s| s));
}

#[test]
fn targets_of_two_chapters_show_together() {
    let book = open();
    let shown = with(&book, &[(0, true), (5, true), (6, true)]);
    let all = text(&shown);
    for want in ["forty-two", "The ending, revealed.", "the sea rising"] {
        assert!(all.contains(want), "{want}: {all}");
    }
    // Links in what is laid again still lead where they did.
    let back = triggers(&shown);
    assert!(back.contains(&vec![Triggered::Show(0, false)]), "{back:?}");
}
