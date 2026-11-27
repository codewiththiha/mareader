//! Space's notes: one group per book, the chips, the search and the rail.

use std::path::PathBuf;

use mareader_library::{ContentId, DeviceId, Library, Measure, NewMark, Opened, Pen, Spot};

use super::*;

fn book(lib: &mut Library, title: &str, content: u128) -> BookId {
    let opened = Opened {
        path: PathBuf::from(format!("/{title}.pdf")),
        measure: Measure {
            content: ContentId(content),
            stamp: Default::default(),
        },
        format: Format::Pdf,
        title: title.to_owned(),
        author: None,
    };
    lib.record_open(opened, 1_000)
}

fn at(page: usize) -> Spot {
    Spot::Page {
        page,
        rect: [0.1, 0.2, 0.8, 0.3],
    }
}

fn lit(words: &str, page: usize) -> NewMark {
    NewMark::highlight(words.to_owned(), at(page), Pen::Full, None)
}

/// Dune: two highlights, one noted, and Explain's own mark. Emma, marked
/// later: a bookmark.
fn shelf() -> (Library, BookId, BookId) {
    let mut lib = Library::new(DeviceId(1));
    let (dune, emma) = (book(&mut lib, "Dune", 1), book(&mut lib, "Emma", 2));
    let late = lib.add_mark(dune, lit("spice must flow", 9), 2_000);
    lib.add_mark(dune, lit("fear is the mind-killer", 3), 3_000);
    let explained = NewMark::explain("arrakis".into(), String::new(), at(1));
    lib.add_mark(dune, explained, 4_000);
    if let Some(id) = late {
        lib.set_note(id, Some("Ask about the guild".into()), 5_000);
    }
    lib.add_mark(emma, NewMark::bookmark("Chapter 3".into(), at(40)), 6_000);
    (lib, dune, emma)
}

fn quotes(view: &NotesView) -> Vec<&str> {
    let rows = view.groups.iter().flat_map(|g| &g.rows);
    rows.map(|r| r.quote.as_str()).collect()
}

#[test]
fn every_book_groups_its_notes_in_reading_order_and_explain_stays_out() {
    let (lib, ..) = shelf();
    let view = notes(&lib, &NotesFilter::default());
    let titles: Vec<&str> = view.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(titles, ["Emma", "Dune"], "the latest marked book first");
    let want = ["Chapter 3", "fear is the mind-killer", "spice must flow"];
    assert_eq!(quotes(&view), want);
    assert_eq!((view.total, view.shown), (3, 3));
    assert_eq!(view.counts, [3, 1, 1]);
    let pages: Vec<Option<usize>> = view.groups[1].rows.iter().map(|r| r.page).collect();
    assert_eq!(pages, [Some(3), Some(9)]);
}

#[test]
fn a_book_the_chips_and_the_search_narrow_the_rows() {
    let (lib, dune, _) = shelf();
    let mut filter = NotesFilter {
        book: Some(dune),
        ..NotesFilter::default()
    };
    let view = notes(&lib, &filter);
    assert_eq!(
        quotes(&view),
        ["fear is the mind-killer", "spice must flow"]
    );
    assert_eq!(view.counts, [2, 1, 0], "the chips count the book picked");
    let rail: Vec<(&str, usize)> = view
        .books
        .iter()
        .map(|s| (s.title.as_str(), s.count))
        .collect();
    assert_eq!(
        rail,
        [("Dune", 2), ("Emma", 1)],
        "the rail keeps every book"
    );
    filter.kept = Kept::Notes;
    assert_eq!(quotes(&notes(&lib, &filter)), ["spice must flow"]);
    filter.kept = Kept::All;
    filter.text = "GUILD".into();
    assert_eq!(
        quotes(&notes(&lib, &filter)),
        ["spice must flow"],
        "notes are searched"
    );
    filter.book = None;
    filter.text = String::new();
    filter.kept = Kept::Bookmarks;
    assert_eq!(quotes(&notes(&lib, &filter)), ["Chapter 3"]);
}

#[test]
fn the_seeded_marks_fill_three_books() {
    let mut lib = Library::new(DeviceId(1));
    for (i, title) in ["Ann", "Bea", "Cy"].iter().enumerate() {
        book(&mut lib, title, i as u128 + 1);
    }
    assert_eq!(seed_marks(&mut lib, 10_000_000_000), 9);
    let view = notes(&lib, &NotesFilter::default());
    assert_eq!(view.groups.len(), 3);
    assert_eq!(view.counts, [9, 4, 2]);
}

#[test]
fn saved_ai_selections_appear_in_all_and_search() {
    let (mut lib, ..) = shelf();
    seed_ai_notes(&mut lib, 8_000);
    let all = notes(&lib, &NotesFilter::default());
    assert_eq!(all.total, 4);
    assert_eq!(all.counts[0], 4);
    assert_eq!(all.ai_groups.iter().map(|g| g.rows.len()).sum::<usize>(), 1);
    let filter = NotesFilter { text: "passage".into(), ..NotesFilter::default() };
    let found = notes(&lib, &filter);
    assert_eq!(found.shown, 1);
    let filter = NotesFilter { kept: Kept::Bookmarks, ..NotesFilter::default() };
    assert!(notes(&lib, &filter).ai_groups.is_empty());
}
