//! A book's cover art and names for the library, read off the frame.
use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Read as _;

use mareader_epub::{Format, MAX_PICTURE_FILE};
use mareader_pdf::Raster;

/// A PDF's first page or a book's picture, at most `side` long.
pub fn cover(path: &Path, side: u32) -> Option<Raster> {
    let name = path.file_name()?.to_string_lossy();
    if let Some(format) = Format::of_name(&name) {
        let bytes = std::fs::read(path).ok()?;
        let folder = path.parent();
        let read = |p: &str| folder.and_then(|f| read_picture(f, p));
        let epub = mareader_epub::convert(&bytes, format, Some(&name), &read).ok()?;
        let (width, height, pixels) = mareader_epub::cover(&Arc::new(epub))?.thumbnail(side)?;
        return Some(Raster {
            width,
            height,
            pixels,
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    if is_epub_file(path)? {
        let (width, height, pixels) = mareader_epub::cover_file(path)?.thumbnail(side)?;
        return Some(Raster {
            width,
            height,
            pixels,
        });
    }
    let bytes = Arc::new(std::fs::read(path).ok()?);
    #[cfg(target_arch = "wasm32")]
    if mareader_epub::sniff(&bytes) {
        let (width, height, pixels) = mareader_epub::cover(&bytes)?.thumbnail(side)?;
        return Some(Raster {
            width,
            height,
            pixels,
        });
    }
    mareader_pdf::first_page(bytes, side)
}

/// A file's own title and author; text converts without its pictures.
pub fn describe(path: &Path) -> (Option<String>, Option<String>) {
    let read = || {
        let name = path.file_name()?.to_string_lossy();
        if let Some(format) = Format::of_name(&name) {
            let bytes = std::fs::read(path).ok()?;
            let epub = mareader_epub::convert(&bytes, format, Some(&name), &|_| None).ok()?;
            return mareader_epub::describe(&Arc::new(epub));
        }
        #[cfg(not(target_arch = "wasm32"))]
        if is_epub_file(path)? {
            return mareader_epub::describe_file(path);
        }
        let bytes = Arc::new(std::fs::read(path).ok()?);
        #[cfg(target_arch = "wasm32")]
        if mareader_epub::sniff(&bytes) {
            return mareader_epub::describe(&bytes);
        }
        mareader_pdf::describe(bytes)
    };
    read().unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn is_epub_file(path: &Path) -> Option<bool> {
    let mut file = File::open(path).ok()?;
    let mut head = [0u8; 4];
    let n = file.read(&mut head).ok()?;
    Some(mareader_epub::sniff(head.get(..n)?))
}

/// A Markdown file's pictures, each with where a copy of it keeps them.
pub fn pictures(path: &Path) -> Vec<(PathBuf, PathBuf)> {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let seen = RefCell::new(Vec::new());
    let read = |p: &str| {
        let file = folder.join(p);
        let bytes = read_picture(folder, p)?;
        if file.is_file()
            && let Some(kept) = stand_in(p)
        {
            seen.borrow_mut().push((file, kept));
        }
        Some(bytes)
    };
    let name = name.to_string_lossy();
    let _ = mareader_epub::convert(&bytes, Format::Markdown, Some(&name), &read);
    let mut seen = seen.into_inner();
    seen.sort();
    seen.dedup();
    seen
}

/// A small enough picture a note names, else a copy's stand-in for it.
pub(crate) fn read_picture(folder: &Path, path: &str) -> Option<Vec<u8>> {
    let read = |path: PathBuf| {
        let len = std::fs::metadata(&path).ok()?.len();
        let fits = usize::try_from(len).is_ok_and(|len| len <= MAX_PICTURE_FILE);
        fits.then(|| std::fs::read(&path).ok()).flatten()
    };
    read(folder.join(path)).or_else(|| read(folder.join(stand_in(path)?)))
}

/// Where a copied note keeps `path`: `../a.png` as `^/a.png`.
fn stand_in(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(n) => out.push(n),
            Component::CurDir => {}
            Component::ParentDir => out.push("^"),
            Component::RootDir => out.push("^root"),
            Component::Prefix(p) => {
                let drive = p.as_os_str().to_string_lossy();
                let safe: String = drive.chars().filter(char::is_ascii_alphanumeric).collect();
                out.push(format!("^{safe}"));
            }
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).ok();
        path
    }

    fn size(r: Option<Raster>) -> Option<(u32, u32)> {
        r.map(|r| (r.width, r.height))
    }

    #[test]
    fn a_file_names_its_title_and_author_without_being_laid_out() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let book = write(dir, "b.epub", &mareader_epub::sample::epub(2, "Bee"));
        assert_eq!(describe(&book).0.as_deref(), Some("Bee"));
        let note = write(
            dir,
            "n.md",
            b"---\ntitle: Notes\nauthor: Me\n---\n\n# One\n\nText.\n",
        );
        let names = describe(&note);
        assert_eq!(
            (names.0.as_deref(), names.1.as_deref()),
            (Some("Notes"), Some("Me"))
        );
        assert_eq!(describe(&dir.join("gone.pdf")), (None, None));
    }

    #[test]
    fn a_note_lists_its_pictures_and_where_a_copy_keeps_them() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        std::fs::create_dir_all(dir.join("notes/img")).ok();
        write(dir, "notes/img/a.png", b"a");
        write(dir, "notes/c.png", b"c");
        write(dir, "up.png", b"up");
        let far = write(dir, "far.png", b"far");
        let text = format!(
            "![a](img/a.png) ![c](./c.png) ![u](../up.png) ![f]({}) \
             ![w](https://x.org/w.png) ![g](gone.png) ![again](img/a.png) ![d](img)",
            far.display()
        );
        let note = write(dir, "notes/n.md", text.as_bytes());
        let mut kept: Vec<PathBuf> = pictures(&note).into_iter().map(|(_, k)| k).collect();
        kept.sort();
        let roots = kept.iter().filter(|k| k.starts_with("^root")).count();
        kept.retain(|k| !k.starts_with("^root"));
        assert_eq!(kept, ["^/up.png", "c.png", "img/a.png"].map(PathBuf::from));
        assert_eq!(roots, 1, "the absolute one");
        assert!(pictures(&dir.join("missing.md")).is_empty());
        // A copy finds an upward picture where it keeps it.
        std::fs::create_dir_all(dir.join("store/^")).ok();
        write(dir, "store/^/kept.png", b"kept");
        let found = read_picture(&dir.join("store"), "../kept.png");
        assert_eq!(found.as_deref(), Some(&b"kept"[..]));
    }

    #[test]
    fn a_book_shows_its_cover_shrunk_never_enlarged() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let book = write(dir, "b.epub", &mareader_epub::sample::epub(2, "B"));
        assert_eq!(size(cover(&book, 512)), Some((341, 512)));
        assert_eq!(size(cover(&book, 2048)), Some((600, 900)));
    }

    #[test]
    fn a_pdf_shows_its_first_page() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let pdf = write(dir, "p.pdf", &mareader_pdf::sample::pdf(3, "P"));
        let got = size(cover(&pdf, 512));
        assert!(
            got.is_some_and(|(w, h)| h.abs_diff(512) <= 1 && w < h),
            "{got:?}"
        );
    }

    #[test]
    fn markdown_shows_its_opening_picture_and_text_has_none() {
        let dir = tempfile::tempdir().ok();
        let dir = dir.as_ref().map_or(Path::new("."), |d| d.path());
        let png = mareader_epub::sample::cover_png().unwrap_or_default();
        write(dir, "c.png", &png);
        let md = write(dir, "a.md", b"![Cover](c.png)\n\n# One\n\nWords.\n");
        assert_eq!(size(cover(&md, 300)), Some((200, 300)));
        let plain = write(dir, "a.txt", b"Chapter 1\n\nJust words.\n");
        assert!(cover(&plain, 300).is_none());
        let deep = write(dir, "d.md", b"# One\n\nA\n\nB\n\nC\n\nD\n\n![x](c.png)\n");
        assert!(
            cover(&deep, 300).is_none(),
            "a picture deep in isn't a cover"
        );
        assert!(cover(&dir.join("gone.pdf"), 300).is_none());
        let junk = write(dir, "j.epub", b"not a book");
        assert!(cover(&junk, 300).is_none());
    }

    #[test]
    fn a_books_author_is_its_first_creator() {
        let bytes = Arc::new(mareader_epub::sample::epub(1, "B"));
        let typo = crate::Typography::default();
        let book = crate::Document::open(bytes, &typo).ok();
        let author = match &book {
            Some(crate::Document::Epub { book, .. }) => book.author(),
            _ => None,
        };
        assert_eq!(author, Some("mareader"));
    }
}
