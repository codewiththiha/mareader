//! A library card's counts: the notes and words Space shows for its book.

use std::path::PathBuf;

use mareader_library::{ContentId, DeviceId, Library, Measure, NewMark, NewWord, Opened, Spot};

use super::*;

fn book(lib: &mut Library, title: &str, content: u128) -> BookId {
    let opened = Opened {
        path: PathBuf::from(format!("/{title}.epub")),
        measure: Measure {
            content: ContentId(content),
            stamp: Default::default(),
        },
        format: Format::Epub,
        title: title.to_owned(),
        author: None,
    };
    lib.record_open(opened, 1_000)
}

fn word(lemma: &str) -> NewWord {
    NewWord {
        lemma: lemma.into(),
        form: lemma.into(),
        context: format!("The {lemma} stood by the door."),
        ..NewWord::default()
    }
}

#[test]
fn a_book_counts_what_spaces_rooms_show_for_it() {
    let mut lib = Library::new(DeviceId(1));
    let (dune, emma) = (book(&mut lib, "Dune", 1), book(&mut lib, "Emma", 2));
    let lone = book(&mut lib, "Lone", 3);
    let at = Spot::Page {
        page: 2,
        rect: [0.1, 0.2, 0.8, 0.3],
    };
    lib.add_mark(dune, NewMark::bookmark("Chapter 3".into(), at), 2_000);
    let explained = NewMark::explain("spice".into(), String::new(), at);
    lib.add_mark(dune, explained, 3_000);
    for lemma in ["lintel", "sconce"] {
        lib.record_word(dune, word(lemma), 4_000);
    }
    lib.record_word(emma, word("lintel"), 5_000);
    let counts = tally(&lib);
    let dune_tally = Tally { notes: 1, words: 2 };
    let explain_out = "Explain's own mark stays out";
    assert_eq!(counts.get(&dune), Some(&dune_tally), "{explain_out}");
    assert_eq!(counts.get(&emma), Some(&Tally { notes: 0, words: 1 }));
    assert_eq!(counts.get(&lone), None, "a book with neither isn't listed");
    let all = view(&lib, &Filter::default());
    let rail: Vec<usize> = all.books.iter().map(|s| s.count).collect();
    assert_eq!(rail, [2, 1], "the same counts as Vocabs' rail");
    let notes = notes(&lib, &NotesFilter::default());
    assert_eq!(notes.total, 1, "the same count as the Notes room");
}
