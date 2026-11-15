use std::path::{Path, PathBuf};

use crate::disk::{self, FoundAgain, Limits, Sought};
use crate::test_support::opened;
use crate::{ContentId, DeviceId, FileStamp, Format, Library, Measure, RelinkError};

const DEVICE: DeviceId = DeviceId(7);

fn measure(content: u128, size: u64) -> Measure {
    Measure {
        content: ContentId(content),
        stamp: FileStamp { size, mtime_ms: 1 },
    }
}

fn path_of(lib: &Library, id: crate::BookId) -> Option<PathBuf> {
    lib.book(id).and_then(|b| b.path()).map(Path::to_path_buf)
}

#[test]
fn relinking_heals_the_same_content_and_asks_about_other() {
    let mut lib = Library::new(DEVICE);
    let id = lib.record_open(
        opened("/old/a.pdf", 1, Format::Pdf, Some(measure(1, 100).stamp)),
        1_000,
    );
    let other = lib.record_open(
        opened("/b.pdf", 2, Format::Pdf, Some(measure(2, 100).stamp)),
        1_000,
    );
    lib.check_present(|p| p != Path::new("/old/a.pdf"));
    assert!(lib.book(id).unwrap().missing());
    let to = Path::new("/new/renamed.pdf");
    assert_eq!(
        lib.relink(id, to, Format::Pdf, measure(1, 100), false),
        Ok(())
    );
    assert_eq!(path_of(&lib, id), Some(to.into()));
    assert_eq!(
        lib.book(id).unwrap().locations.len(),
        1,
        "the dead path goes"
    );
    // Another book's file, another format, other content: refused or asked.
    let b = lib.relink(id, Path::new("/x.pdf"), Format::Pdf, measure(2, 1), false);
    assert_eq!(b, Err(RelinkError::OtherBook("Book 2".into())));
    let epub = lib.relink(id, Path::new("/x.epub"), Format::Epub, measure(9, 1), true);
    let wrong = RelinkError::Format {
        file: Format::Epub,
        book: Format::Pdf,
    };
    assert_eq!(epub, Err(wrong));
    let edition = Path::new("/x.pdf");
    let asked = lib.relink(id, edition, Format::Pdf, measure(9, 1), false);
    assert_eq!(asked, Err(RelinkError::Differs));
    assert_eq!(
        lib.relink(id, edition, Format::Pdf, measure(9, 1), true),
        Ok(())
    );
    assert_eq!(lib.book(id).unwrap().content, ContentId(9));
    assert!(lib.book(other).is_some());
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Old enough not to be settling.
fn limits() -> Limits {
    Limits {
        settle_ms: 0,
        ..Limits::new(u64::MAX)
    }
}

#[test]
fn a_walk_keeps_books_and_skips_hidden_litter_and_deep_folders() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("a.pdf"), b"%PDF a");
    write(&root.join("sub/b.EPUB"), b"b");
    write(&root.join("sub/notes.docx"), b"no");
    write(&root.join(".hidden/c.pdf"), b"c");
    write(&root.join(".d.pdf"), b"d");
    write(&root.join("~$lock.pdf"), b"e");
    write(&root.join("node_modules/f.pdf"), b"f");
    write(&root.join("1/2/3/deep.pdf"), b"g");
    let shallow = Limits {
        depth: 2,
        ..limits()
    };
    let walked = disk::walk(root, |_, _| true, shallow, &[]);
    let names: Vec<_> = walked
        .files
        .iter()
        .map(|f| f.0.strip_prefix(root).unwrap())
        .collect();
    assert_eq!(names, [Path::new("a.pdf"), Path::new("sub/b.EPUB")]);
    assert!(walked.truncated, "too deep to see everything");
    assert!(!walked.offline);
    // A file being copied is left for a later look.
    let now = Limits::new(u64::MAX / 2);
    let fresh = Limits {
        now_ms: walked.files[0].2.mtime_ms,
        ..now
    };
    assert_eq!(disk::walk(root, |_, _| true, fresh, &[]).settling, 3);
    assert!(disk::walk(&root.join("gone"), |_, _| true, limits(), &[]).offline);
}

#[cfg(unix)]
#[test]
fn a_walk_never_follows_links() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("books");
    write(&root.join("a.pdf"), b"a");
    write(&dir.path().join("outside/b.pdf"), b"b");
    std::os::unix::fs::symlink(dir.path().join("outside"), root.join("link")).unwrap();
    std::os::unix::fs::symlink(root.join("a.pdf"), root.join("again.pdf")).unwrap();
    let walked = disk::walk(&root, |_, _| true, limits(), &[]);
    assert_eq!(walked.files.len(), 1);
}

#[test]
fn a_lost_file_is_found_again_by_content_under_any_name() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let old = root.join("book.pdf");
    write(&old, b"%PDF the very book");
    let content = disk::measure(&old, Format::Pdf).unwrap();
    std::fs::remove_file(&old).unwrap();
    write(&root.join("x/renamed.pdf"), b"%PDF the very book");
    write(&root.join("y/book.pdf"), b"%PDF a later edition");
    let sought = Sought {
        content: content.content,
        format: Format::Pdf,
        name: Some("book.pdf".into()),
        size: Some(content.stamp.size),
    };
    let FoundAgain::Same(path, _) = disk::find_again(root, &sought, limits()) else {
        panic!("not found by content");
    };
    assert_eq!(path, root.join("x/renamed.pdf"));
    std::fs::remove_file(&path).unwrap();
    let named = disk::find_again(root, &sought, limits());
    assert!(matches!(named, FoundAgain::Named(p, _) if p == root.join("y/book.pdf")));
    std::fs::remove_file(root.join("y/book.pdf")).unwrap();
    assert_eq!(
        disk::find_again(root, &sought, limits()),
        FoundAgain::Nothing
    );
}
