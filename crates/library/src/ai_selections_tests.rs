use crate::disk::Folder;
use crate::test_support::{opened, unsent};
use crate::tests::save;
use crate::{BookId, Data, DeviceId, Format, Kind, Library, NewAiSelection, Spot};

const A: DeviceId = DeviceId(1);
const B: DeviceId = DeviceId(2);

fn selection(spot: Spot, text: &str) -> NewAiSelection {
    NewAiSelection {
        skill: "simplify".into(),
        name: "Simplify".into(),
        spot,
        source: "a passage".into(),
        result: text.into(),
        by: "example model".into(),
    }
}

#[test]
fn selections_round_trip_and_overlaps_remain_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.epub", 1, Format::Epub, None), 1_000);
    let spot = Spot::Blocks {
        from: [3, 4],
        to: [5, 120],
    };
    let first = lib
        .add_ai_selection(book, selection(spot, "one"), 2_000)
        .unwrap();
    let second = lib
        .add_ai_selection(book, selection(spot, "two"), 2_100)
        .unwrap();
    assert_ne!(first, second);
    assert!(
        lib.add_ai_selection(BookId(99), selection(spot, "x"), 2_200)
            .is_none()
    );
    save(&folder, &mut lib);
    let mut back = folder.load(A).library;
    let saved = back.ai_selection(first).unwrap();
    assert_eq!(
        (saved.spot, saved.source.as_str(), saved.result.as_str()),
        (spot, "a passage", "one")
    );
    assert_eq!(back.ai_selections(book).len(), 2);
    back.take_dirty();
    assert!(back.set_ai_selection_alive(first, false, 3_000));
    let rows = unsent(&mut back);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, Kind::AiSelection);
    assert_eq!(back.ai_selections(book).len(), 1);
    assert!(back.set_ai_selection_alive(first, true, 3_100));
}

#[test]
fn selections_sync_and_erased_book_data_cannot_be_restored() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let spot = Spot::Chars {
        from: [0, 4],
        to: [1, 9],
    };
    let id = lib
        .add_ai_selection(book, selection(spot, "answer"), 2_000)
        .unwrap();
    assert_eq!(lib.receipt(&[book]).ai_selections, 1);
    assert!(lib.receipt(&[book]).has_data());
    let mut other = Library::new(B);
    other.absorb(unsent(&mut lib), 2_100);
    assert_eq!(other.ai_selection(id).unwrap().spot, spot);
    assert!(other.set_ai_selection_alive(id, false, 2_200));
    lib.absorb(unsent(&mut other), 2_300);
    assert!(lib.ai_selection(id).is_none());
    assert!(lib.set_ai_selection_alive(id, true, 2_400));
    lib.remove_books(&[book], Data::Delete, 2_500);
    let saved = lib.ai_selections.get(&id).unwrap();
    assert!(saved.source.is_empty() && saved.result.is_empty());
    lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 2_600);
    assert!(!lib.set_ai_selection_alive(id, true, 2_700));
}
