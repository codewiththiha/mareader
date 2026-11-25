use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::folder::{Mode, Options};
use crate::holding::Holding;
use crate::ledger::Recovered;
use crate::link::LinkTo;
use crate::run::{Asked, Copied, Landed, Plan};
use crate::{
    BookId, ContentId, Data, DeviceId, FileStamp, FolderId, Format, Found, Library, Measure,
    NewMark, Origin, Position, ShelfId, ShelfKind, Spot,
};

pub(crate) const DEVICE: DeviceId = DeviceId(3);

pub(crate) fn found(path: &str, content: u128) -> Found {
    Found {
        path: PathBuf::from(path),
        format: Format::Pdf,
        measure: Measure {
            content: ContentId(content),
            stamp: FileStamp {
                size: 100 + content as u64,
                mtime_ms: 1,
            },
        },
        title: None,
        author: None,
    }
}

pub(crate) fn opts(mode: Mode) -> Options {
    let mut o = Options::default();
    o.set_mode(mode);
    o
}

/// A whole run at once; a copying run's copies land in `/store`.
pub(crate) fn run(
    lib: &mut Library,
    root: &str,
    mode: Mode,
    files: &[Found],
    asked: Asked,
    plan: Plan,
) -> (FolderId, Landed) {
    let id = lib.open_folder(PathBuf::from(root), opts(mode), &plan, 1_000);
    let planned = lib.plan_walk(id, files.to_vec(), asked, plan).unwrap();
    let copies: HashMap<PathBuf, Copied> = (planned.to_copy(mode.copies()).into_iter())
        .map(|f| {
            let name = f.path.file_name().unwrap();
            let copy = Copied {
                path: Path::new("/store").join(name),
                measure: f.measure,
            };
            (f.path.clone(), copy)
        })
        .collect();
    (id, lib.land_walk(planned, &copies, 2_000))
}

pub(crate) fn import(
    lib: &mut Library,
    root: &str,
    mode: Mode,
    files: &[Found],
) -> (FolderId, Landed) {
    run(lib, root, mode, files, Asked::Explicitly, Plan::default())
}

pub(crate) fn rescan(lib: &mut Library, id: FolderId, files: &[Found]) -> Landed {
    let planned = lib
        .plan_walk(id, files.to_vec(), Asked::OnFocus, Plan::default())
        .unwrap();
    lib.land_walk(planned, &HashMap::new(), 3_000)
}

pub(crate) fn shelf_named(lib: &Library, name: &str) -> ShelfId {
    lib.shelves()
        .find(|s| s.name.value == name)
        .map(|s| s.id)
        .unwrap()
}

pub(crate) fn titles_on(lib: &Library, shelf: ShelfId) -> Vec<String> {
    let mut t: Vec<String> = lib
        .shelf_books(shelf)
        .iter()
        .map(|b| b.title.value.clone())
        .collect();
    t.sort();
    t
}

#[test]
fn a_folder_read_in_place_lands_its_books_on_a_shelf_per_subfolder() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/dune.pdf", 1), found("/books/sf/neuro.pdf", 2)];
    let (id, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    assert_eq!(landed.placed.len(), 2);
    let root = shelf_named(&lib, "books");
    assert_eq!(landed.root, Some(root));
    let sf = shelf_named(&lib, "sf");
    assert_eq!(
        lib.shelf(sf).unwrap().parent.value,
        Some(root),
        "rungs nest as the disk"
    );
    assert_eq!(titles_on(&lib, root), ["dune"]);
    assert_eq!(titles_on(&lib, sf), ["neuro"]);
    let kind = &lib.shelf(sf).unwrap().kind.value;
    assert_eq!(kind.rung(), Some((id, "sf")));
    let book = lib.book(landed.placed[0]).unwrap();
    assert_eq!(
        book.locations[0].origin,
        Origin::Folder(id),
        "read where it is"
    );
}

#[test]
fn one_shelf_for_everything_files_every_subfolder_on_the_root() {
    let mut lib = Library::new(DEVICE);
    let mut o = opts(Mode::InPlace);
    o.groups = false;
    let id = lib.open_folder("/books".into(), o, &Plan::default(), 1);
    let files = vec![found("/books/a/x.pdf", 1), found("/books/b/c/y.pdf", 2)];
    let planned = lib
        .plan_walk(id, files, Asked::Explicitly, Plan::default())
        .unwrap();
    lib.land_walk(planned, &HashMap::new(), 2);
    assert_eq!(lib.shelves().count(), 1);
    assert_eq!(titles_on(&lib, shelf_named(&lib, "books")), ["x", "y"]);
}

#[test]
fn a_rescan_of_an_unchanged_folder_lands_nothing() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (id, _) = import(&mut lib, "/books", Mode::Watched, &files);
    assert_eq!(rescan(&mut lib, id, &files).total(), 0);
}

#[test]
fn a_rescan_adds_only_under_a_watched_rung() {
    let mut lib = Library::new(DEVICE);
    let (id, _) = import(
        &mut lib,
        "/books",
        Mode::InPlace,
        &[found("/books/a.pdf", 1)],
    );
    let new = [found("/books/a.pdf", 1), found("/books/b.pdf", 2)];
    assert_eq!(
        rescan(&mut lib, id, &new).total(),
        0,
        "nobody watches the tree"
    );
    let (id, _) = import(
        &mut lib,
        "/books",
        Mode::Watched,
        &[found("/books/a.pdf", 1)],
    );
    assert_eq!(rescan(&mut lib, id, &new).placed.len(), 1);
}

#[test]
fn a_removed_book_stays_out_until_the_reader_imports_it_again() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (id, landed) = import(&mut lib, "/books", Mode::Watched, &files);
    lib.remove_books(&landed.placed, Data::Keep, 2_500);
    assert!(
        lib.folder(id).unwrap().is_ignored(ContentId(1)),
        "a tombstone"
    );
    assert_eq!(
        rescan(&mut lib, id, &files).total(),
        0,
        "a rescan stays quiet"
    );
    let (_, again) = import(&mut lib, "/books", Mode::Watched, &files);
    assert_eq!(again.placed.len(), 1, "an import the reader asked for");
    assert!(
        !lib.folder(id).unwrap().is_ignored(ContentId(1)),
        "the stone is spent"
    );
}

#[test]
fn a_moved_file_is_followed_not_added_twice() {
    let mut lib = Library::new(DEVICE);
    let (id, landed) = import(
        &mut lib,
        "/books",
        Mode::Watched,
        &[found("/books/a.pdf", 1)],
    );
    lib.check_present(|_| false);
    let moved = rescan(&mut lib, id, &[found("/books/sub/a.pdf", 1)]);
    assert_eq!((moved.relinked, moved.placed.len()), (1, 0));
    let book = lib.book(landed.placed[0]).unwrap();
    assert_eq!(book.path(), Some(Path::new("/books/sub/a.pdf")));
}

#[test]
fn a_copying_folder_lands_store_copies_that_remember_their_source() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (id, landed) = import(&mut lib, "/books", Mode::Copy, &files);
    let book = lib.book(landed.placed[0]).unwrap();
    let home = &book.locations[0];
    assert_eq!(
        (home.origin, home.path.as_path()),
        (Origin::Store, Path::new("/store/a.pdf"))
    );
    assert_eq!(home.from.as_deref(), Some(Path::new("/books/a.pdf")));
    assert!(
        !lib.folder(id).unwrap().owes_walk(),
        "copied once, never walked"
    );
    let (_, again) = import(&mut lib, "/books", Mode::Copy, &files);
    assert!(again.placed.is_empty(), "its own source is no second copy");
    assert_eq!(again.returned, 1, "the copy that is there is the one filed");
    assert_eq!(lib.len(), 1);
}

#[test]
fn a_copy_moved_at_its_source_moves_only_its_source() {
    let mut lib = Library::new(DEVICE);
    let (_, landed) = import(&mut lib, "/books", Mode::Copy, &[found("/books/a.pdf", 1)]);
    let planned = {
        let id = lib.open_folder("/books".into(), opts(Mode::Watched), &Plan::default(), 5);
        lib.plan_walk(
            id,
            vec![found("/books/b/a.pdf", 1)],
            Asked::Explicitly,
            Plan::default(),
        )
    };
    lib.land_walk(planned.unwrap(), &HashMap::new(), 6);
    let book = lib.book(landed.placed[0]).unwrap();
    assert_eq!(
        book.locations[0].path,
        Path::new("/store/a.pdf"),
        "the bytes stay"
    );
    assert_eq!(
        book.locations[0].from.as_deref(),
        Some(Path::new("/books/b/a.pdf"))
    );
}

#[test]
fn copying_over_a_tree_read_in_place_makes_books_of_their_own() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (_, first) = import(&mut lib, "/books", Mode::InPlace, &files);
    let (_, copies) = run(
        &mut lib,
        "/books",
        Mode::Copy,
        &files,
        Asked::Explicitly,
        Plan::default(),
    );
    assert_eq!(copies.placed.len(), 1);
    let copy = lib.book(copies.placed[0]).unwrap();
    assert!(copy.independent && copy.id != first.placed[0]);
    assert_eq!(
        lib.registry()[&ContentId(1)].id,
        first.placed[0],
        "lookups pass it by"
    );
}

fn mark(lib: &mut Library, book: BookId) {
    let spot = Spot::Page {
        page: 1,
        rect: [0.0, 0.0, 1.0, 1.0],
    };
    let new = NewMark::explain("spice".into(), "the spice".into(), spot);
    lib.add_mark(book, new, 2_100);
}

#[test]
fn kept_data_comes_back_with_the_file_and_deleted_data_does_not() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (_, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let id = landed.placed[0];
    lib.rename_book(id, "Dune", 2_050);
    lib.record_position(
        id,
        Position {
            page: 9,
            pages: 20,
            place: None,
            spine: None,
        },
        2_100,
    );
    mark(&mut lib, id);
    let receipt = lib.receipt(&[id]);
    assert!(receipt.has_data() && receipt.marks == 1);
    lib.remove_books(&[id], Data::Keep, 2_200);
    let (_, back) = import(&mut lib, "/books", Mode::InPlace, &files);
    let book = lib.book(back.placed[0]).unwrap();
    assert_eq!(book.title.value, "Dune");
    assert_eq!(lib.position(book.id).map(|p| p.page), Some(9));
    assert_eq!(lib.receipt(&[book.id]).marks, 1);
    lib.remove_books(&[book.id], Data::Delete, 2_300);
    let (_, fresh) = import(&mut lib, "/books", Mode::InPlace, &files);
    let book = lib.book(fresh.placed[0]).unwrap();
    assert_eq!(book.title.value, "a", "a book never seen");
    assert_eq!(
        lib.position(book.id)
            .map(|p| (p.page, p.pages, p.place, p.spine)),
        Some((0, 0, None, None)),
        "the position and its old page count are erased"
    );
    assert_eq!(lib.receipt(&[book.id]).marks, 0);
}

fn answer(meaning: &str) -> crate::Gloss {
    crate::Gloss {
        pos: "noun".into(),
        meaning: meaning.into(),
        usages: vec!["The spice must flow.".into()],
        by: "Gemini · flash".into(),
        ..crate::Gloss::default()
    }
}

fn simplified(page: usize) -> crate::NewAiPage {
    crate::NewAiPage {
        task: crate::AiTask::Simplify,
        span: crate::PageSpan::Page { page },
        source: "00ff".into(),
        result: "plain words".into(),
        by: "Ollama · gemma".into(),
    }
}

#[test]
fn ai_pages_come_back_with_kept_data_and_go_with_erased_data() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (_, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let id = landed.placed[0];
    lib.add_ai_page(id, simplified(2), 2_100).unwrap();
    let receipt = lib.receipt(&[id]);
    assert!(receipt.has_data(), "AI pages alone offer to erase");
    assert_eq!(receipt.ai_pages, 1);
    lib.remove_books(&[id], Data::Keep, 2_200);
    let (_, back) = import(&mut lib, "/books", Mode::InPlace, &files);
    let id = back.placed[0];
    assert_eq!(lib.ai_pages(id).len(), 1, "kept: back with the book");
    lib.remove_books(&[id], Data::Delete, 2_300);
    let (_, fresh) = import(&mut lib, "/books", Mode::InPlace, &files);
    assert_eq!(fresh.placed[0], id, "the same file is the same book");
    assert!(lib.ai_pages(id).is_empty(), "erased: they stay gone");
    assert_eq!(lib.receipt(&[id]).ai_pages, 0);
}

#[test]
fn erasing_a_book_blanks_every_record_even_ones_removed_before() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (_, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let id = landed.placed[0];
    mark(&mut lib, id);
    let mark_id = lib.marks(id)[0].id;
    assert!(lib.set_note(mark_id, Some("my thoughts".into()), 2_110));
    assert!(lib.set_gloss(mark_id, Some(answer("a fine dust")), 2_115));
    let old = lib.add_ai_page(id, simplified(1), 2_120).unwrap();
    let new = lib.add_ai_page(id, simplified(1), 2_130).unwrap();
    assert_ne!(old, new, "the old one is replaced, its words kept");
    // A highlight removed earlier, kept only so Undo can bring it back.
    lib.set_marks_alive(&[mark_id], false, 2_140);
    lib.remove_books(&[id], Data::Delete, 2_200);
    for m in lib.marks.values().filter(|m| m.book.value == id) {
        assert!(!m.alive.value && m.word.is_empty() && m.context.is_empty());
        assert!(m.gloss.value.is_none() && m.note.value.is_none());
    }
    let pages: Vec<_> = (lib.ai_pages.values())
        .filter(|p| p.book.value == id)
        .collect();
    assert_eq!(pages.len(), 2);
    assert!(pages.iter().all(|p| !p.alive.value && p.result.is_empty()));
    import(&mut lib, "/books", Mode::InPlace, &files);
    assert!(lib.book(id).is_some(), "back as a fresh book");
    assert_eq!(lib.set_marks_alive(&[mark_id], true, 2_400), 0, "no Undo");
    assert_eq!(lib.set_ai_pages_alive(&[old, new], true, 2_400), 0);
    // An answer that was still on its way lands nowhere.
    assert!(!lib.set_gloss(mark_id, Some(answer("late")), 2_500));
    assert!(!lib.set_note(mark_id, Some("late".into()), 2_500));
    assert!(lib.marks(id).is_empty() && lib.ai_pages(id).is_empty());
}

#[test]
fn a_restore_lands_on_the_shelf_it_left_else_the_folders_root() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    let (id, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    lib.remove_books(&landed.placed, Data::Keep, 2_500);
    let removed: Vec<String> = (lib.recoverables(id).into_iter())
        .filter_map(|r| match r {
            Recovered::Removed(stone) => Some(stone.label()),
            Recovered::Moved { .. } => None,
        })
        .collect();
    assert_eq!(removed, ["a", "b"]);
    let sf = shelf_named(&lib, "sf");
    let back = lib
        .restore_removed(id, ContentId(2), &files[1], None, 3_000)
        .unwrap();
    assert_eq!(lib.shelves_of(back).into_iter().collect::<Vec<_>>(), [sf]);
    lib.remove_shelf(sf, 3_100);
    let back = lib
        .restore_removed(id, ContentId(1), &files[0], None, 3_200)
        .unwrap();
    let root = shelf_named(&lib, "books");
    assert!(lib.shelves_of(back).contains(&root));
    assert!(lib.recoverables(id).is_empty());
}

#[test]
fn a_book_moved_off_the_tree_is_offered_back_with_where_it_went() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    let (id, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let root = shelf_named(&lib, "books");
    let fiction = lib.create_shelf("Fiction", None, 2_100).unwrap();
    lib.unfile(landed.placed[0], root, 2_200);
    lib.file(landed.placed[0], fiction, 2_200);
    let got = lib.recoverables(id);
    assert!(matches!(&got[..], [Recovered::Moved { home: Some(h), .. }] if h == "Fiction"));
}

#[test]
fn a_merge_asks_about_a_name_the_shelf_already_holds() {
    let mut lib = Library::new(DEVICE);
    let shelf = lib.create_shelf("Mine", None, 1).unwrap();
    let (_, held) = import(
        &mut lib,
        "/old",
        Mode::InPlace,
        &[found("/old/dune.pdf", 1)],
    );
    lib.file(held.placed[0], shelf, 2);
    let plan = Plan {
        into: Some(shelf),
        ..Plan::default()
    };
    let files = vec![found("/books/dune.pdf", 2), found("/books/other.pdf", 3)];
    let id = lib.open_folder("/books".into(), opts(Mode::InPlace), &plan, 3);
    let planned = lib.plan_walk(id, files, Asked::Explicitly, plan).unwrap();
    assert_eq!(planned.asks.len(), 1);
    assert_eq!(planned.asks[0].arrival.name, "dune");
    let landed = lib.land_walk(planned, &HashMap::new(), 4);
    assert_eq!(
        landed.root,
        Some(shelf),
        "files into the shelf that is here"
    );
    assert_eq!(titles_on(&lib, shelf), ["dune", "other"]);
}

#[test]
fn add_as_new_mints_a_tree_of_its_own_under_the_new_name() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1)];
    import(&mut lib, "/books", Mode::Copy, &files);
    let plan = Plan {
        rename: Some("books_1".into()),
        ..Plan::default()
    };
    let (_, landed) = run(
        &mut lib,
        "/books",
        Mode::Copy,
        &files,
        Asked::Explicitly,
        plan,
    );
    let new = shelf_named(&lib, "books_1");
    assert_eq!(landed.root, Some(new));
    assert_eq!(
        titles_on(&lib, new),
        ["a"],
        "the held book files on the new tree"
    );
}

#[test]
fn rungs_hang_as_the_disk_does_unless_a_hand_moved_them() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/sf/a.pdf", 1)];
    let (id, _) = import(&mut lib, "/books", Mode::Watched, &files);
    let (root, sf) = (shelf_named(&lib, "books"), shelf_named(&lib, "sf"));
    let other = lib.create_shelf("Other", None, 5).unwrap();
    lib.shelves.get_mut(&sf).unwrap().parent.value = Some(other);
    rescan(&mut lib, id, &files);
    assert_eq!(
        lib.shelf(sf).unwrap().parent.value,
        Some(root),
        "back on its seat"
    );
    lib.shelves.get_mut(&sf).unwrap().parent.value = Some(other);
    lib.shelves.get_mut(&sf).unwrap().moved.value = true;
    rescan(&mut lib, id, &files);
    assert_eq!(
        lib.shelf(sf).unwrap().parent.value,
        Some(other),
        "the reader's place wins"
    );
}

#[test]
fn taking_a_shelf_apart_lifts_its_books_and_shelves_a_level() {
    let mut lib = Library::new(DEVICE);
    let files = [
        found("/books/sf/a.pdf", 1),
        found("/books/sf/deep/b.pdf", 2),
    ];
    import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, sf, deep) = (
        shelf_named(&lib, "books"),
        shelf_named(&lib, "sf"),
        shelf_named(&lib, "deep"),
    );
    let link = lib.link_to(LinkTo::Shelf(sf), None, 5).unwrap();
    lib.remove_shelf(sf, 6);
    assert_eq!(titles_on(&lib, root), ["a"]);
    assert_eq!(lib.shelf(deep).unwrap().parent.value, Some(root));
    assert!(lib.link(link).is_none(), "a link goes with its shelf");
}

#[test]
fn removing_a_shelf_with_everything_inside_takes_its_books_too() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    let (id, _) = import(&mut lib, "/books", Mode::InPlace, &files);
    let root = shelf_named(&lib, "books");
    lib.remove_shelves(&[root], true, Data::Keep, 7);
    assert!(lib.is_empty() && lib.shelves().count() == 0);
    assert_eq!(lib.folder(id).unwrap().ignored.len(), 2);
}

#[test]
fn a_removed_book_takes_its_links_with_it() {
    let mut lib = Library::new(DEVICE);
    let (_, landed) = import(
        &mut lib,
        "/books",
        Mode::InPlace,
        &[found("/books/a.pdf", 1)],
    );
    let link = lib
        .link_to(LinkTo::Book(landed.placed[0]), None, 5)
        .unwrap();
    assert_eq!(lib.link(link).unwrap().name.value, "a");
    lib.remove_books(&landed.placed, Data::Keep, 6);
    assert!(lib.link(link).is_none());
}

fn copy(path: &str, content: u128) -> Copied {
    Copied {
        path: PathBuf::from(path),
        measure: found(path, content).measure,
    }
}

#[test]
fn a_duplicate_is_the_librarys_own_copy_marks_and_all_under_the_next_name() {
    let mut lib = Library::new(DEVICE);
    let (_, landed) = import(
        &mut lib,
        "/books",
        Mode::InPlace,
        &[found("/books/Dune.pdf", 1)],
    );
    let id = landed.placed[0];
    mark(&mut lib, id);
    let dup = lib.duplicate_book(id, &copy("/store/d.pdf", 1), 5).unwrap();
    let book = lib.book(dup).unwrap();
    assert_eq!(book.title.value, "Dune_1");
    assert!(book.independent && book.locations[0].origin == Origin::Store);
    assert_eq!(lib.receipt(&[dup]).marks, 1);
    let root = shelf_named(&lib, "books");
    assert_eq!(titles_on(&lib, root), ["Dune", "Dune_1"], "filed beside it");
}

#[test]
fn a_duplicated_shelf_copies_its_tree_and_points_its_links_at_the_copies() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    let (_, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, sf) = (shelf_named(&lib, "books"), shelf_named(&lib, "sf"));
    lib.link_to(LinkTo::Book(landed.placed[0]), Some(sf), 4)
        .unwrap();
    let wants = lib.duplicate_wants(root);
    assert_eq!(wants.len(), 2);
    let copies: HashMap<BookId, Copied> = (wants.iter().enumerate())
        .map(|(n, b)| (*b, copy(&format!("/store/{n}.pdf"), n as u128 + 1)))
        .collect();
    let done = lib.duplicate_shelf(root, &copies, 5);
    assert_eq!((done.shelves, done.books, done.links), (1, 2, 1));
    let new = done.shelf.unwrap();
    assert_eq!(lib.shelf(new).unwrap().name.value, "books_1");
    assert_eq!(lib.shelf(new).unwrap().kind.value, ShelfKind::Own);
    let link = lib.links().find(|l| l.level.value != Some(sf)).unwrap();
    let LinkTo::Book(target) = link.target else {
        panic!("a book link")
    };
    assert!(target != landed.placed[0] && lib.book(target).unwrap().independent);
}

#[test]
fn a_book_links_duplicate_copies_its_book_and_a_shelf_links_stays_a_link() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/Dune.pdf", 1), found("/books/sf/b.pdf", 2)];
    let (_, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let sf = shelf_named(&lib, "sf");
    let at_book = lib.link_to(LinkTo::Book(landed.placed[0]), Some(sf), 4);
    let at_shelf = lib.link_to(LinkTo::Shelf(sf), None, 4).unwrap();
    let dup = lib.duplicate_book_on(landed.placed[0], &copy("/store/d.pdf", 1), Some(sf), 5);
    assert!(at_book.is_some() && dup.is_some());
    assert_eq!(
        titles_on(&lib, sf),
        ["Dune_1", "b"],
        "filed where the link is"
    );
    let again = lib.duplicate_link(at_shelf, 6).unwrap();
    let link = lib.link(again).unwrap();
    assert_eq!(
        (link.name.value.as_str(), link.target),
        ("sf_1", LinkTo::Shelf(sf))
    );
}

#[test]
fn a_shelfs_watch_writes_its_own_rung_and_the_root_writes_the_tree() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/a.pdf", 1), found("/books/sf/b.pdf", 2)];
    import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, sf) = (shelf_named(&lib, "books"), shelf_named(&lib, "sf"));
    lib.set_shelf_watch(root, false);
    let watch = lib.shelf_watch(sf).unwrap();
    assert_eq!(
        (watch.on, watch.folder.as_str(), watch.rung.as_deref()),
        (false, "books", Some("sf"))
    );
    assert!(lib.set_shelf_watch(sf, true));
    assert!(!lib.set_shelf_watch(sf, true), "already watched");
    assert!(lib.shelf_tracked(sf) && !lib.shelf_tracked(root));
    assert!(lib.set_shelf_watch(root, true) && lib.set_shelf_watch(root, false));
    assert!(
        !lib.shelf_tracked(sf),
        "the root's answer clears the tree's"
    );
    assert_eq!(lib.shelf_dir(sf), Some(PathBuf::from("/books/sf")));
}

#[test]
fn a_shelfs_badge_says_where_its_books_live_down_its_tree() {
    let mut lib = Library::new(DEVICE);
    let files = [found("/books/sf/a.pdf", 1)];
    let (_, landed) = import(&mut lib, "/books", Mode::InPlace, &files);
    let (root, sf) = (shelf_named(&lib, "books"), shelf_named(&lib, "sf"));
    assert_eq!(lib.holding(sf), Some(Holding::OnDisk));
    assert_eq!(lib.holding(root), Some(Holding::OnDisk), "read from below");
    let dup = lib.duplicate_book(landed.placed[0], &copy("/store/a.pdf", 1), 5);
    assert!(dup.is_some());
    assert_eq!(lib.holding(sf), Some(Holding::Mixed));
    let own = lib.create_shelf("Mine", None, 6).unwrap();
    assert_eq!(lib.holding(own), None, "an empty shelf of the reader's own");
    lib.link_to(LinkTo::Book(dup.unwrap()), Some(own), 7);
    assert_eq!(
        lib.holding(own),
        Some(Holding::Copied),
        "a link counts as its book"
    );
}
