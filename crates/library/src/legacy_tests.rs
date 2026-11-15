#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use crate::disk::Folder;
use crate::test_support::opened;
use crate::tests::save;
use crate::{
    Data, DeviceId, FileStamp, FolderId, Format, Kind, Library, Origin, Recovered, Row, ShelfKind,
};

const DEVICE: DeviceId = DeviceId(7);
const OLD: FolderId = FolderId(0xabc);

/// A book read in place under the old folder.
fn found_in_old(lib: &mut Library, path: &str, content: u128) -> crate::BookId {
    let stamp = FileStamp {
        size: 2048,
        mtime_ms: 1,
    };
    let id = lib.record_open(opened(path, content, Format::Pdf, Some(stamp)), 1_000);
    for location in &mut lib.books.get_mut(&id).unwrap().locations {
        location.origin = Origin::Folder(OLD);
    }
    id
}

fn kinds(dir: &Path) -> Vec<Kind> {
    let store = crate::store::Store::open(&dir.join("library.sqlite")).unwrap();
    store.rows().unwrap().into_iter().map(|r| r.kind).collect()
}

#[test]
fn an_old_folder_row_becomes_a_folder_with_its_removals_as_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    let a = found_in_old(&mut lib, "/s/Books/a.pdf", 1);
    let b = found_in_old(&mut lib, "/s/Books/Poems/b.pdf", 2);
    let gone = found_in_old(&mut lib, "/s/Books/c.pdf", 3);
    let top = lib.create_shelf("Books", None, 1_100).unwrap();
    let poems = lib.create_shelf("Poems", Some(top), 1_100).unwrap();
    lib.file(a, top, 1_200);
    lib.file(b, poems, 1_200);
    lib.remove_books(&[gone], Data::Keep, 1_300);
    save(&folder, &mut lib);
    let old = serde_json::json!({
        "id": OLD, "root": "/s/Books", "formats": [Format::Epub], "exclude": true,
        "min_size": 4096, "watch": true, "added": 500, "hidden": [gone],
        "shelving": "each", "shelf": top,
    });
    let row = Row {
        kind: Kind::OldFolder,
        id: OLD.to_string(),
        body: old.to_string(),
    };
    let changes = crate::Changes {
        put: vec![row],
        ..Default::default()
    };
    let mut store = folder.load(DEVICE).store.unwrap();
    store.apply(&changes).unwrap();
    drop(store);

    let mut back = folder.load(DEVICE).library;
    let adopted = back.folder(OLD).unwrap();
    assert_eq!(adopted.root, Path::new("/s/Books"));
    let o = &adopted.opts;
    assert!(o.in_place && o.watch && o.groups && o.include);
    assert!(!o.formats.contains(&Format::Epub) && o.formats.contains(&Format::Pdf));
    assert_eq!(o.min_size, 4096);
    let placed: Vec<u128> = adopted.placed.iter().map(|c| c.0).collect();
    assert_eq!(placed, [1, 2, 3]);
    let map: Vec<(&str, _)> = (adopted.shelf_map.iter())
        .map(|(k, s)| (k.as_str(), *s))
        .collect();
    assert_eq!(map, [("", top), ("Poems", poems)]);
    let rung = |s| back.shelf(s).unwrap().kind.value.clone();
    let poems_rung = ShelfKind::Rung {
        folder: OLD,
        key: "Poems".into(),
    };
    assert_eq!(rung(poems), poems_rung);
    // Its removal is offered back from the folder's menu.
    let back_again: Vec<String> = (back.recoverables(OLD).into_iter())
        .filter_map(|r| match r {
            Recovered::Removed(stone) => Some(stone.label()),
            Recovered::Moved { .. } => None,
        })
        .collect();
    assert_eq!(back_again, ["Book 3"]);
    // The next save writes the folder and retires the old row.
    assert!(back.is_dirty());
    save(&folder, &mut back);
    let saved = kinds(dir.path());
    assert!(saved.contains(&Kind::Folder) && !saved.contains(&Kind::OldFolder));
    let again = folder.load(DEVICE).library;
    assert!(!again.is_dirty());
    assert_eq!(again.folder(OLD).unwrap().shelf_map.len(), 2);
}

#[test]
fn an_old_folder_that_excluded_everything_takes_every_format() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = Library::new(DEVICE);
    found_in_old(&mut lib, "/s/a.pdf", 1);
    save(&folder, &mut lib);
    let every: Vec<Format> = Format::ALL.into();
    let old = serde_json::json!({
        "id": OLD, "root": "/s", "formats": every, "exclude": true, "watch": false,
    });
    let row = Row {
        kind: Kind::OldFolder,
        id: OLD.to_string(),
        body: old.to_string(),
    };
    let changes = crate::Changes {
        put: vec![row],
        ..Default::default()
    };
    folder.load(DEVICE).store.unwrap().apply(&changes).unwrap();
    let back = folder.load(DEVICE).library;
    let adopted = back.folder(OLD).unwrap();
    assert_eq!(adopted.opts.formats.len(), Format::ALL.len());
    assert!(!adopted.opts.groups, "no shelving became one shelf");
    assert!(adopted.shelf_map.is_empty());
}
