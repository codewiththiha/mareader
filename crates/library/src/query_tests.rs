use std::path::PathBuf;

use crate::folder::Mode;
use crate::query::fold;
use crate::run_tests::import;
use crate::test_support::measure;
use crate::{BookId, Data, DeviceId, Format, Found, Library, Opened, Query, SUGGEST_LIMIT, Target};

const DEVICE: DeviceId = DeviceId(7);

/// A book opened from `path`, read at `when`.
fn add(lib: &mut Library, path: &str, title: &str, author: Option<&str>, when: u64) -> BookId {
    let content = 1 + lib.books().count() as u128;
    let opened = Opened {
        path: PathBuf::from(path),
        measure: measure(content),
        format: Format::Pdf,
        title: title.into(),
        author: author.map(Into::into),
    };
    lib.record_open(opened, when)
}

fn keeps(lib: &Library, id: BookId, text: &str) -> bool {
    let book = lib.book(id).unwrap();
    Query::new(text).keeps(lib, book)
}

fn titles(lib: &Library, text: &str) -> Vec<String> {
    let found = lib.suggest(&Query::new(text), SUGGEST_LIMIT);
    let name = |t: Target| match t {
        Target::Book(id) => lib.book(id).unwrap().title.value.clone(),
        Target::Shelf(id) => format!("[{}]", lib.shelf(id).unwrap().name.value),
    };
    found.into_iter().map(|s| name(s.target)).collect()
}

#[test]
fn a_blank_query_keeps_everything_and_suggests_nothing() {
    let mut lib = Library::new(DEVICE);
    let id = add(&mut lib, "/b/dune.pdf", "Dune", None, 1);
    for blank in ["", "   ", "\t"] {
        assert!(Query::new(blank).is_empty());
        assert!(keeps(&lib, id, blank));
        assert!(titles(&lib, blank).is_empty());
    }
}

#[test]
fn every_word_must_turn_up_somewhere_in_any_order_and_case() {
    let mut lib = Library::new(DEVICE);
    let id = add(
        &mut lib,
        "/b/dune.pdf",
        "Dune Messiah",
        Some("Frank Herbert"),
        1,
    );
    for yes in ["dune", "DUNE", "messiah", "herbert dune", "frank  dune"] {
        assert!(keeps(&lib, id, yes), "{yes}");
    }
    for no in ["foundation", "dune asimov"] {
        assert!(!keeps(&lib, id, no), "{no}");
    }
}

#[test]
fn accents_quotes_and_dashes_never_matter() {
    let mut lib = Library::new(DEVICE);
    let id = add(
        &mut lib,
        "/b/x.pdf",
        "Les Misérables",
        Some("Émile Zoła"),
        1,
    );
    let quote = add(&mut lib, "/b/y.pdf", "O’Brien’s Notes — Part One", None, 1);
    assert!(keeps(&lib, id, "miserables"));
    assert!(keeps(&lib, id, "MISÉRABLES"));
    assert!(keeps(&lib, id, "emile zola"));
    assert!(keeps(&lib, quote, "o'brien's"));
    assert!(keeps(&lib, quote, "notes - part"));
    assert_eq!(fold('Ł'), 'l');
    assert_eq!(fold('Œ'), 'œ', "a ligature is kept, not half-spelled");
}

#[test]
fn a_subsequence_that_holds_its_shape_matches_and_a_scatter_does_not() {
    let mut lib = Library::new(DEVICE);
    let proofs = add(&mut lib, "/b/p.pdf", "Mathematical Proofs", None, 1);
    let dark = add(&mut lib, "/b/d.pdf", "The Left Hand of Darkness", None, 1);
    let long = add(
        &mut lib,
        "/b/z.pdf",
        "A Brief Collection of Zebra Essays",
        None,
        1,
    );
    assert!(keeps(&lib, proofs, "mthmtcl prfs"));
    assert!(keeps(&lib, dark, "tlhod"), "initials are word starts");
    assert!(!keeps(&lib, long, "xyz"));
    assert!(!keeps(&lib, proofs, "sfoorp"), "order matters");
    assert!(!keeps(&lib, proofs, "proofss"), "so does count");
}

#[test]
fn a_word_start_outranks_the_middle_of_a_word() {
    let mut lib = Library::new(DEVICE);
    add(&mut lib, "/b/1.pdf", "Alchemy", None, 1);
    add(&mut lib, "/b/2.pdf", "Mystery Novels", None, 1);
    add(&mut lib, "/b/3.pdf", "MysteryNovels", None, 1);
    let got = titles(&lib, "nov");
    assert_eq!(
        got,
        ["Mystery Novels", "MysteryNovels"],
        "camel case starts a word"
    );
    let got = titles(&lib, "my");
    assert_eq!(got[2], "Alchemy", "{got:?}");
}

#[test]
fn the_title_outranks_the_author_which_outranks_the_address() {
    let mut lib = Library::new(DEVICE);
    add(&mut lib, "/x/a.pdf", "Notes on Dune", None, 1);
    add(
        &mut lib,
        "/x/b.pdf",
        "Collected Papers",
        Some("Dune White"),
        2,
    );
    add(&mut lib, "/dune/raw.pdf", "Raw Pages", None, 3);
    let got = titles(&lib, "dune");
    assert_eq!(got, ["Notes on Dune", "Collected Papers", "Raw Pages"]);
}

#[test]
fn ties_go_to_the_book_read_last() {
    let mut lib = Library::new(DEVICE);
    add(&mut lib, "/x/a.pdf", "Poems I", None, 10);
    add(&mut lib, "/x/b.pdf", "Poems II", None, 30);
    add(&mut lib, "/x/c.pdf", "Poems III", None, 20);
    assert_eq!(titles(&lib, "poems"), ["Poems II", "Poems III", "Poems I"]);
}

#[test]
fn the_address_is_the_path_inside_its_folder_not_the_whole_disk() {
    let mut lib = Library::new(DEVICE);
    let found = Found {
        path: "/home/me/Books/Scifi/1984-report.pdf".into(),
        format: Format::Pdf,
        measure: measure(9),
        title: Some("Untitled scan".into()),
        author: None,
    };
    let (_, landed) = import(&mut lib, "/home/me/Books", Mode::InPlace, &[found]);
    let id = landed.placed[0];
    let loose = add(&mut lib, "/home/me/Downloads/paper.pdf", "A Paper", None, 3);
    let book = lib.book(id).unwrap();
    assert_eq!(lib.address(book), "Scifi/1984-report.pdf");
    assert_eq!(lib.address(lib.book(loose).unwrap()), "Downloads/paper.pdf");
    assert!(keeps(&lib, id, "scifi 1984"));
    assert!(
        !keeps(&lib, id, "home"),
        "the disk's own folders match nothing"
    );
}

#[test]
fn shelves_are_suggested_by_name_and_lit_where_they_matched() {
    let mut lib = Library::new(DEVICE);
    add(&mut lib, "/x/a.pdf", "Fictional Worlds", None, 1);
    let shelf = lib.create_shelf("Science Fiction", None, 2).unwrap();
    // An even score: the book read goes first.
    assert_eq!(
        titles(&lib, "fiction"),
        ["Fictional Worlds", "[Science Fiction]"]
    );
    let found = lib.suggest(&Query::new("sci fic"), SUGGEST_LIMIT);
    assert_eq!(found[0].target, Target::Shelf(shelf));
    assert_eq!(found[0].hit.title, [0..3, 8..11]);
    assert!(Query::new("fiction science").keeps_name("Science Fiction"));
    assert!(!Query::new("science crime").keeps_name("Science Fiction"));
}

#[test]
fn spans_are_byte_ranges_of_the_text_as_written() {
    let mut lib = Library::new(DEVICE);
    let id = add(&mut lib, "/x/a.pdf", "Élan Vital", Some("Henri Bergson"), 1);
    let hit = Query::new("vital berg")
        .rank(&lib, lib.book(id).unwrap())
        .unwrap();
    let title = &lib.book(id).unwrap().title.value;
    assert_eq!(&title[hit.title[0].clone()], "Vital");
    assert_eq!(&"Henri Bergson"[hit.author[0].clone()], "Berg");
    assert_eq!(hit.author.len(), 1);
    let hit = Query::new("elan")
        .rank(&lib, lib.book(id).unwrap())
        .unwrap();
    assert_eq!(
        &title[hit.title[0].clone()],
        "Élan",
        "accented chars span whole"
    );
}

#[test]
fn missing_books_and_twins_are_found_and_removed_books_are_not() {
    let mut lib = Library::new(DEVICE);
    let id = add(&mut lib, "/x/raven.pdf", "The Raven", None, 1);
    let twin = lib.twin(id, 2).unwrap();
    let gone = add(&mut lib, "/x/lost.pdf", "The Lost Raven", None, 3);
    lib.check_present(|p| !p.ends_with("lost.pdf"));
    assert!(lib.book(gone).unwrap().missing());
    let got = titles(&lib, "raven");
    assert_eq!(got.len(), 3, "{got:?}");
    lib.remove_books(&[twin], Data::Keep, 4);
    assert!(!titles(&lib, "raven").contains(&"The Raven (2)".to_owned()));
}

#[test]
fn a_revision_moves_with_books_and_shelves_but_not_with_pages() {
    let mut lib = Library::new(DEVICE);
    let start = lib.revision();
    let id = add(&mut lib, "/x/a.pdf", "A", None, 1);
    let added = lib.revision();
    assert_ne!(start, added);
    let page = crate::Position {
        page: 3,
        pages: 9,
        place: None,
        spine: None,
    };
    lib.record_position(id, page, 2);
    assert_eq!(lib.revision(), added, "a page turn leaves the listing be");
    lib.rename_book(id, "B", 3);
    assert_ne!(lib.revision(), added);
    let other = Library::new(DEVICE);
    assert_ne!(
        other.revision(),
        lib.revision(),
        "never shared between libraries"
    );
}
