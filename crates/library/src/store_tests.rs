#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use rusqlite::Connection;

use crate::disk::Folder;
use crate::store::Store;
use crate::test_support::opened;
use crate::tests::save;
use crate::{DeviceId, Format, Kind, Library, Position, SpinePosition};

const DEVICE: DeviceId = DeviceId(7);

fn sql(dir: &Path) -> Connection {
    Connection::open(dir.join("library.sqlite")).unwrap()
}

#[test]
fn a_save_writes_only_the_rows_that_changed() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    lib.record_open(opened("/b.pdf", 2, Format::Pdf, None), 1_100);
    save(&folder, &mut lib);

    assert!(lib.rename_book(a, "Renamed", 2_000));
    let dirty = lib.take_dirty();
    let changes = lib.changes(&dirty);
    let kinds: Vec<Kind> = changes.put.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Kind::Book, Kind::Locations]);
    assert!(changes.put.iter().all(|r| r.id == a.to_string()));
    // A page turn writes one row: that book's state.
    let position = Position {
        page: 3,
        pages: 9,
        place: None,
        spine: None,
    };
    lib.record_position(a, position, 2_100);
    let dirty = lib.take_dirty();
    let kinds: Vec<Kind> = lib.changes(&dirty).put.iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Kind::State]);
}

#[test]
fn library_json_moves_into_the_store_once() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let shelf = lib.create_shelf("Poems", None, 1_100).unwrap();
    lib.file(a, shelf, 1_200);
    let books: Vec<_> = lib.books.values().collect();
    let shelves: Vec<_> = lib.shelves.values().collect();
    let members: Vec<_> = lib.members.values().collect();
    let json = serde_json::json!({
        "schema": 4, "clock": lib.clock, "books": books,
        "shelves": shelves, "members": members,
    });
    let file = dir.path().join("library.json");
    std::fs::write(&file, json.to_string()).unwrap();

    let loaded = folder.load(DeviceId(99));
    assert!(
        loaded.writable && loaded.notes.is_empty(),
        "{:?}",
        loaded.notes
    );
    assert!(!file.exists() && dir.path().join("library.json.migrated").is_file());
    let back = loaded.library;
    assert_eq!(back.device(), DEVICE);
    assert_eq!(back.book_at(Path::new("/a.pdf")), Some(a));
    assert_eq!(back.shelf_books(shelf).first().map(|b| b.id), Some(a));
    drop(loaded.store);
    // Read again from the store alone.
    let again = folder.load(DEVICE).library;
    assert_eq!(again.book_at(Path::new("/a.pdf")), Some(a));
    // Looking for the files is never a change to save.
    assert!(!again.is_dirty());
}

#[test]
fn a_damaged_store_is_kept_aside_and_a_new_one_started() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("library.sqlite");
    std::fs::write(&file, vec![7u8; 4096]).unwrap();
    let loaded = Folder::new(dir.path()).load(DEVICE);
    assert!(loaded.writable && loaded.library.is_empty());
    assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
    assert!(dir.path().join("library.unreadable.sqlite").is_file());
}

#[test]
fn a_newer_store_is_read_but_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    save(&folder, &mut lib);
    sql(dir.path())
        .pragma_update(None, "user_version", 99)
        .unwrap();

    let loaded = folder.load(DEVICE);
    assert!(!loaded.writable && loaded.store.is_none());
    assert_eq!(loaded.notes.len(), 1);
    assert!(loaded.library.book(a).is_some());
}

#[test]
fn rows_this_build_cant_read_are_kept_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    save(&folder, &mut lib);
    let conn = sql(dir.path());
    let add = "INSERT INTO records(kind, id, body, seq) VALUES(?1, ?2, ?3, 1)";
    conn.execute(add, ["note", "n1", "{}"]).unwrap();
    conn.execute(add, ["book", "broken", "{ nope"]).unwrap();
    drop(conn);

    let mut loaded = folder.load(DEVICE);
    // An unknown kind is a newer build's: silent. A broken row is told.
    assert_eq!(loaded.notes.len(), 1, "{:?}", loaded.notes);
    let id = loaded
        .library
        .record_open(opened("/b.pdf", 2, Format::Pdf, None), 2_000);
    let dirty = loaded.library.take_dirty();
    let changes = loaded.library.changes(&dirty);
    loaded.store.as_mut().unwrap().apply(&changes).unwrap();
    drop(loaded);
    let count = |kind: &str| -> i64 {
        let query = "SELECT count(*) FROM records WHERE kind = ?1";
        sql(dir.path())
            .query_row(query, [kind], |r| r.get(0))
            .unwrap()
    };
    assert_eq!((count("note"), count("book")), (1, 3));
    assert!(folder.load(DEVICE).library.book(id).is_some());
}

#[test]
fn the_change_cursor_names_what_each_save_wrote() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let b = lib.record_open(opened("/b.pdf", 2, Format::Pdf, None), 1_100);
    save(&folder, &mut lib);
    lib.rename_book(b, "Later", 2_000);
    save(&folder, &mut lib);

    let store = Store::open(&dir.path().join("library.sqlite")).unwrap();
    let all = store.changed_since(0).unwrap();
    let first = all.iter().find(|c| c.row.id == a.to_string()).unwrap().seq;
    let since = store.changed_since(first).unwrap();
    let ids: Vec<&str> = since.iter().map(|c| c.row.id.as_str()).collect();
    assert_eq!(ids, [b.to_string()]);
    // Paths are this machine's: they never leave as records.
    assert!(all.iter().all(|c| c.row.kind.synced()));
}

#[test]
fn state_files_move_into_the_store_once() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    save(&folder, &mut lib);
    let position = Position {
        page: 12,
        pages: 40,
        place: None,
        spine: Some(SpinePosition {
            href: "Text/chapter.xhtml".into(),
            item: 4,
            block: 8,
            offset: 17,
        }),
    };
    lib.record_position(a, position.clone(), 1_500);
    let state = lib.states.get(&a).unwrap();
    let file = serde_json::json!({ "schema": 4, "book": a, "state": state });
    let states = dir.path().join("state");
    std::fs::create_dir_all(&states).unwrap();
    std::fs::write(states.join(format!("{a}.json")), file.to_string()).unwrap();

    let loaded = folder.load(DEVICE);
    assert!(loaded.notes.is_empty(), "{:?}", loaded.notes);
    assert_eq!(loaded.library.position(a), Some(position.clone()));
    assert!(!states.exists() && dir.path().join("state.migrated").is_dir());
    drop(loaded);
    assert_eq!(folder.load(DEVICE).library.position(a), Some(position));
}

#[test]
fn erased_words_leave_the_files_while_the_store_stays_open() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let loaded = folder.load(DEVICE);
    let (mut lib, mut store) = (loaded.library, loaded.store.unwrap());
    let a = lib.record_open(opened("/a.pdf", 1, Format::Pdf, None), 1_000);
    let spot = crate::Spot::Page {
        page: 1,
        rect: [0.0, 0.0, 1.0, 1.0],
    };
    let word = crate::NewMark::explain("zebrafinch".into(), "a zebrafinch sang".into(), spot);
    lib.add_mark(a, word, 1_100);
    let mark = lib.marks(a)[0].id;
    assert!(lib.set_note(mark, Some("quokkanote".into()), 1_200));
    // The Explain card's saved answer.
    let gloss = crate::Gloss {
        pos: "noun".into(),
        meaning: "okapimeaning".into(),
        synonyms: vec!["dikdiksynonym".into()],
        usages: vec!["a gerenukusage here".into()],
        by: "Gemini".into(),
    };
    assert!(lib.set_gloss(mark, Some(gloss), 1_250));
    let page = crate::NewAiPage {
        task: crate::AiTask::Simplify,
        span: crate::PageSpan::Page { page: 1 },
        source: "00ff".into(),
        result: "narwhalresult".into(),
        by: "Ollama".into(),
    };
    lib.add_ai_page(a, page, 1_300).unwrap();
    let mut write = |lib: &mut Library| {
        let dirty = lib.take_dirty();
        store.apply(&lib.changes(&dirty)).unwrap();
    };
    let on_disk = |needle: &str| {
        let files = ["library.sqlite", "library.sqlite-wal"];
        let bytes = files
            .iter()
            .filter_map(|f| std::fs::read(dir.path().join(f)).ok());
        bytes
            .into_iter()
            .any(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
    };
    let secrets = [
        "zebrafinch",
        "quokkanote",
        "okapimeaning",
        "dikdiksynonym",
        "gerenukusage",
        "narwhalresult",
    ];
    write(&mut lib);
    assert!(secrets.iter().all(|s| on_disk(s)), "written first");
    lib.remove_books(&[a], crate::Data::Delete, 2_000);
    write(&mut lib);
    for secret in secrets {
        assert!(!on_disk(secret), "{secret} is still on disk");
    }
}
