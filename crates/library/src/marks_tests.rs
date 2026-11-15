//! Marks: made once per spot, removed and brought back, saved, traded.

use crate::disk::Folder;
use crate::test_support::{opened, unsent};
use crate::tests::save;
use crate::{
    BookId, DeviceId, Format, Gloss, Kind, Library, MarkId, MarkKind, NewMark, Pen, Spot, Style,
};

const A: DeviceId = DeviceId(1);
const B: DeviceId = DeviceId(2);

fn fox(x: f32) -> NewMark {
    let spot = Spot::Page {
        page: 0,
        rect: [x, 40.0, 18.0, 10.0],
    };
    NewMark::explain("fox".into(), "the quick brown fox jumps".into(), spot)
}

fn words(lib: &Library, book: BookId) -> Vec<String> {
    lib.marks(book).iter().map(|m| m.word.clone()).collect()
}

#[test]
fn a_word_is_marked_once_per_spot() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let first = lib.add_mark(book, fox(100.0), 2_000).unwrap();
    // A selection a hair off is the same spot; another place is not.
    assert_eq!(lib.add_mark(book, fox(100.4), 2_100), Some(first));
    let other = lib.add_mark(book, fox(300.0), 2_200).unwrap();
    assert_ne!(other, first);
    assert_eq!(words(&lib, book), ["fox", "fox"]);
    // A book the library doesn't know gets none.
    assert_eq!(lib.add_mark(BookId(99), fox(1.0), 2_300), None);
}

#[test]
fn a_moved_file_keeps_its_shelves_and_marks() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/old/a.pdf", 1, Format::Pdf, None), 1_000);
    let shelf = lib.create_shelf("Essays", None, 1_100).unwrap();
    assert!(lib.file(book, shelf, 1_200));
    lib.add_mark(book, fox(100.0), 1_300).unwrap();
    assert_eq!(
        lib.record_open(opened("/new/b.pdf", 1, Format::Pdf, None), 2_000),
        book
    );
    assert!(lib.shelves_of(book).contains(&shelf));
    assert_eq!(words(&lib, book), ["fox"]);
}

#[test]
fn removing_and_undo_bring_a_mark_back_as_it_was() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let id = lib.add_mark(book, fox(100.0), 2_000).unwrap();
    lib.take_dirty();
    assert_eq!(lib.set_marks_alive(&[id], false, 3_000), 1);
    assert!(lib.marks(book).is_empty() && lib.mark(id).is_none());
    let kinds: Vec<Kind> = unsent(&mut lib).iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Kind::Mark], "a removal is a tombstone row");
    assert_eq!(lib.set_marks_alive(&[id], false, 3_100), 0, "already gone");
    assert_eq!(lib.set_marks_alive(&[id], true, 3_200), 1);
    assert_eq!(lib.mark(id).map(|m| m.word.as_str()), Some("fox"));
}

#[test]
fn every_mark_change_moves_the_marks_stamp() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let opened_at = lib.marks_stamp();
    let id = lib.add_mark(book, fox(100.0), 2_000).unwrap();
    let added = lib.marks_stamp();
    assert_ne!(added, opened_at);
    let _ = lib.marks(book);
    assert_eq!(lib.marks_stamp(), added, "reading leaves it be");
    assert!(lib.set_note(id, Some("sly".into()), 2_100));
    let noted = lib.marks_stamp();
    assert_ne!(noted, added);
    lib.set_marks_alive(&[id], false, 2_200);
    assert_ne!(lib.marks_stamp(), noted);
}

#[test]
fn marks_are_saved_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.epub", 1, Format::Pdf, None), 1_000);
    let spot = Spot::Text {
        block: 3,
        start: 40,
        end: 47,
    };
    let word = NewMark::explain("customs".into(), "past the customs house".into(), spot);
    let id = lib.add_mark(book, word.clone(), 2_000).unwrap();
    save(&folder, &mut lib);
    let back = folder.load(A).library;
    let mark = back.mark(id).unwrap();
    assert_eq!((mark.word.as_str(), mark.spot), ("customs", word.spot));
    assert_eq!(mark.context, word.context);
}

#[test]
fn marks_travel_and_a_removal_wins_over_an_older_keep() {
    let mut a = Library::new(A);
    let mut b = Library::new(B);
    let book = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    b.record_open(opened("/elsewhere/a.pdf", 1, Format::Pdf, None), 1_100);
    let id = a.add_mark(book, fox(100.0), 2_000).unwrap();
    b.absorb(unsent(&mut a), 2_500);
    assert_eq!(words(&b, book), ["fox"]);
    b.set_marks_alive(&[id], false, 3_000);
    a.absorb(unsent(&mut b), 3_500);
    assert!(a.marks(book).is_empty(), "the removal came across");
}

#[test]
fn folding_two_originals_moves_the_marks_to_the_keeper() {
    // Here a file edited in place; there the edited file, imported new.
    let mut a = Library::new(A);
    let kept = a.record_open(opened("/notes.md", 1, Format::Pdf, None), 1_000);
    a.record_open(opened("/notes.md", 2, Format::Pdf, None), 2_000);
    let mut b = Library::new(B);
    let gone = b.record_open(opened("/elsewhere/notes.md", 2, Format::Pdf, None), 2_500);
    let id = b.add_mark(gone, fox(100.0), 2_600).unwrap();
    assert!(kept < gone);
    for _ in 0..4 {
        let (to_b, to_a) = (unsent(&mut a), unsent(&mut b));
        a.absorb(to_a, 3_000);
        b.absorb(to_b, 3_000);
    }
    for lib in [&a, &b] {
        assert_eq!(lib.mark(id).map(|m| m.book.value), Some(kept));
        assert_eq!(words(lib, kept), ["fox"]);
    }
}

fn said(meaning: &str) -> Option<Gloss> {
    Some(Gloss {
        meaning: meaning.into(),
        by: "Ollama · gemma".into(),
        ..Gloss::default()
    })
}

#[test]
fn an_answer_is_saved_with_its_mark_and_hides_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let id = lib.add_mark(book, fox(100.0), 2_000).unwrap();
    assert!(lib.set_gloss(id, said("A wild dog."), 2_100));
    save(&folder, &mut lib);
    let mut back = folder.load(A).library;
    let gloss = back.mark(id).and_then(|m| m.gloss.value.clone());
    assert_eq!(gloss, said("A wild dog."));

    back.set_marks_alive(&[id], false, 3_000);
    assert!(
        !back.set_gloss(id, said("Lost."), 3_100),
        "a removed mark takes none"
    );
    back.set_marks_alive(&[id], true, 3_200);
    let gloss = back.mark(id).and_then(|m| m.gloss.value.clone());
    assert_eq!(gloss, said("A wild dog."), "Undo brings the answer back");
}

#[test]
fn the_later_answer_wins_on_every_machine() {
    let mut a = Library::new(A);
    let mut b = Library::new(B);
    let book = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    b.record_open(opened("/elsewhere/a.pdf", 1, Format::Pdf, None), 1_100);
    let id = a.add_mark(book, fox(100.0), 2_000).unwrap();
    a.set_gloss(id, said("First."), 2_100);
    b.absorb(unsent(&mut a), 2_500);
    b.set_gloss(id, said("Asked again."), 3_000);
    a.absorb(unsent(&mut b), 3_500);
    for lib in [&a, &b] {
        let gloss = lib.mark(id).and_then(|m| m.gloss.value.clone());
        assert_eq!(gloss, said("Asked again."));
    }
}

fn note_of(lib: &Library, id: MarkId) -> Option<String> {
    lib.mark(id).and_then(|m| m.note.value.clone())
}

#[test]
fn a_note_is_trimmed_saved_and_cleared_by_blank() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let id = lib.add_mark(book, fox(100.0), 2_000).unwrap();
    assert!(lib.set_note(id, Some("  Check the date.\n".into()), 2_100));
    save(&folder, &mut lib);
    let mut back = folder.load(A).library;
    assert_eq!(note_of(&back, id).as_deref(), Some("Check the date."));
    assert!(back.set_note(id, Some(" \n ".into()), 2_200));
    assert_eq!(note_of(&back, id), None, "blank clears it");
    back.set_marks_alive(&[id], false, 3_000);
    assert!(!back.set_note(id, Some("Lost.".into()), 3_100));
}

#[test]
fn the_later_note_wins_on_every_machine() {
    let mut a = Library::new(A);
    let mut b = Library::new(B);
    let book = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    b.record_open(opened("/elsewhere/a.pdf", 1, Format::Pdf, None), 1_100);
    let id = a.add_mark(book, fox(100.0), 2_000).unwrap();
    a.set_note(id, Some("First.".into()), 2_100);
    b.absorb(unsent(&mut a), 2_500);
    b.set_note(id, Some("Rewritten.".into()), 3_000);
    a.absorb(unsent(&mut b), 3_500);
    for lib in [&a, &b] {
        assert_eq!(note_of(lib, id).as_deref(), Some("Rewritten."));
    }
}

#[test]
fn highlights_span_pages_and_keep_their_pen_and_colour() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let spot = Spot::Chars {
        from: [0, 120],
        to: [1, 30],
    };
    let text = "the end of one page and the start of the next".to_owned();
    let new = NewMark::highlight(text.clone(), spot, Pen::Half, Some([255, 190, 0]));
    let id = lib.add_mark(book, new.clone(), 2_000).unwrap();
    // The same text with the same pen is one highlight; another pen is not.
    assert_eq!(lib.add_mark(book, new.clone(), 2_100), Some(id));
    let under = NewMark::highlight(text, spot, Pen::Underline, None);
    assert_ne!(lib.add_mark(book, under, 2_200), Some(id));
    save(&folder, &mut lib);
    let back = folder.load(A).library;
    let mark = back.mark(id).unwrap();
    assert_eq!((mark.kind, mark.pen), (MarkKind::Highlight, Pen::Half));
    assert_eq!((mark.spot, mark.color), (spot, Some([255, 190, 0])));
    assert_eq!(back.marks(book).len(), 2);
}

#[test]
fn a_mark_saved_before_highlights_reads_as_an_explain_mark() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let id = lib.add_mark(book, fox(100.0), 2_000).unwrap();
    let mut row = serde_json::to_value(lib.mark(id).unwrap()).unwrap();
    let fields = row.as_object_mut().unwrap();
    for gone in ["kind", "pen", "color"] {
        fields.remove(gone);
    }
    let mark: crate::Mark = serde_json::from_value(row).unwrap();
    assert_eq!(
        (mark.kind, mark.pen, mark.color),
        (MarkKind::Explain, Pen::Full, None)
    );
}

fn quick() -> NewMark {
    let spot = Spot::Chars {
        from: [0, 4],
        to: [0, 15],
    };
    NewMark::highlight("quick brown".into(), spot, Pen::Full, None)
}

#[test]
fn a_highlight_is_restyled_and_keeps_it_after_a_reload() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let id = lib.add_mark(book, quick(), 2_000).unwrap();
    let style = Style {
        pen: Pen::Underline,
        color: Some([0xff, 0, 0]),
    };
    assert!(lib.set_style(id, style, 2_100));
    save(&folder, &mut lib);
    let back = folder.load(A).library;
    let mark = back.mark(id).unwrap();
    assert_eq!(mark.look(), style);
    assert_eq!(mark.pen, Pen::Full, "the look as made is kept");
    // The AI's marks keep its look; a removed mark takes no style.
    let explained = lib.add_mark(book, fox(100.0), 2_200).unwrap();
    assert!(!lib.set_style(explained, style, 2_300));
    lib.set_marks_alive(&[id], false, 3_000);
    assert!(!lib.set_style(id, style, 3_100));
}

#[test]
fn the_later_style_wins_on_every_machine() {
    let mut a = Library::new(A);
    let mut b = Library::new(B);
    let book = a.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    b.record_open(opened("/elsewhere/a.pdf", 1, Format::Pdf, None), 1_100);
    let id = a.add_mark(book, quick(), 2_000).unwrap();
    let half = Style {
        pen: Pen::Half,
        color: None,
    };
    a.set_style(id, half, 2_100);
    b.absorb(unsent(&mut a), 2_500);
    let under = Style {
        pen: Pen::Underline,
        color: Some([0, 0xb2, 0xf4]),
    };
    b.set_style(id, under, 3_000);
    a.absorb(unsent(&mut b), 3_500);
    for lib in [&a, &b] {
        let mark = lib.mark(id).unwrap();
        assert_eq!(mark.look(), under);
    }
}

#[test]
fn a_bookmark_is_kept_once_carries_a_note_and_takes_no_style() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.epub", 1, Format::Pdf, None), 1_000);
    let spot = Spot::Text {
        block: 4,
        start: 120,
        end: 120,
    };
    let id = lib
        .add_mark(book, NewMark::bookmark("Two".into(), spot), 2_000)
        .unwrap();
    let again = lib.add_mark(book, NewMark::bookmark("Two".into(), spot), 2_100);
    assert_eq!(again, Some(id), "one bookmark per place");
    assert!(lib.set_note(id, Some("Reread".into()), 2_200));
    let style = Style {
        pen: Pen::Half,
        color: None,
    };
    assert!(!lib.set_style(id, style, 2_300));
    save(&folder, &mut lib);
    let back = folder.load(A).library;
    let mark = back.mark(id).unwrap();
    assert_eq!(
        (mark.kind, mark.note.value.as_deref()),
        (MarkKind::Bookmark, Some("Reread"))
    );
}
