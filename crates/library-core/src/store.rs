//! Where a book's bytes live on disk: one folder per book, keyed by an id that
//! never changes.
//!
//! Every file a book owns — its source, its cover, its highlights — sits in the
//! one folder its id names, so a merge or a delete is one directory. Names are
//! not title-derived: a book can be renamed without anything on disk moving.

pub const ITEMS_DIR: &str = "items";

/// The file name a stored book's bytes wear inside its item folder: a PDF is
/// `source.pdf`, a markdown file `source.md`. One predictable stem, the format
/// carried by the suffix.
const SOURCE_STEM: &str = "source";

pub const COVER_FILE: &str = "cover.webp";

pub const MARKS_FILE: &str = "marks.json";

pub const META_FILE: &str = "meta.json";

/// The item-folder root under a store root: `<store_root>/items`. Everything
/// below lives here, which is what keeps a book's bytes, cover and marks inside
/// the store the delete command's containment check already guards.
pub fn items_root(store_root: &str) -> String {
    join(trim_sep(store_root), ITEMS_DIR)
}

/// The folder one book owns end to end: `<items_root>/<id>`. The id is
/// sanitised into a single component rather than trusted: a hand-edited blob
/// must not be able to turn a folder name into a traversal.
pub fn item_dir(items_root: &str, book_id: &str) -> String {
    join(trim_sep(items_root), &component(book_id))
}

/// Where a stored book's bytes live: `<items_root>/<id>/source.<ext>`. An empty
/// extension yields a bare `source` with no suffix, which no admitted format
/// asks for — the registry refuses an extension-less name.
pub fn source_path(items_root: &str, book_id: &str, ext: &str) -> String {
    // The emptiness check is on the RAW extension: `component` maps an empty
    // string to its `item` fallback, which is right for a folder name and wrong for
    // "this source has no suffix".
    let file = if ext.trim().is_empty() {
        SOURCE_STEM.to_string()
    } else {
        format!("{SOURCE_STEM}.{}", component(ext))
    };
    join(&item_dir(items_root, book_id), &file)
}

pub fn cover_path(items_root: &str, book_id: &str) -> String {
    join(&item_dir(items_root, book_id), COVER_FILE)
}

pub fn marks_path(items_root: &str, book_id: &str) -> String {
    join(&item_dir(items_root, book_id), MARKS_FILE)
}

pub fn meta_path(items_root: &str, book_id: &str) -> String {
    join(&item_dir(items_root, book_id), META_FILE)
}

/// The stem a migrated source keeps in its new name, which is every supported
/// extension lower-cased. `None` for an extension the registry does not know,
/// which keeps its old name rather than being renamed into something no reader
/// can open.
pub fn migrated_ext(ext: &str) -> Option<&'static str> {
    match crate::scan::store_dir(ext) {
        "other" => None,
        dir => Some(dir),
    }
}

/// The id a copy in the OLD flat store was named with: the token after its last
/// underscore. The old name was `<stem>_<id>.<ext>`, and an id never carries an
/// underscore. `None` for a name with no seam, which is a file this app did not
/// write.
pub fn flat_store_id(file_name: &str) -> Option<String> {
    let stem = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);
    stem.rsplit_once('_')
        .map(|(_, id)| id.to_string())
        .filter(|id| !id.is_empty())
}

/// Whether a recorded store address still sits in the OLD flat bucket, and so
/// is a candidate for the one-time migration: directly inside one of the three
/// format directories under the store root, as `<root>/pdf/dune_ab12.pdf`.
pub fn is_flat_store_path(store_root: &str, path: &str) -> bool {
    // A directory edge rather than a string prefix, so `/Library-old/x` is not
    // under `/Library` — the rule `crate::folder::rel_under` gives a watched
    // folder, applied to the one directory the app owns.
    let Some(rest) = crate::folder::rel_under(path, store_root) else {
        return false;
    };
    let mut parts = rest.split('/');
    let (Some(dir), Some(_file), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    matches!(dir, "pdf" | "text" | "markdown")
}

/// Drop a trailing separator so a join never produces `root//child`. Both
/// separators are trimmed: a store root arrives from the host's own path API.
fn trim_sep(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
}

fn join(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_string()
    } else {
        format!("{parent}/{child}")
    }
}

/// One path component, made safe to write: separators, the characters Windows
/// reserves, and control characters become `_`; the result is trimmed of the
/// dots and spaces that would make it a relative path, capped, and replaced
/// with a fallback when nothing is left.
fn component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let capped: String = cleaned.trim_matches(['.', ' ']).chars().take(64).collect();
    match capped.as_str() {
        "" | "." | ".." => "item".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "b018c4f9e2a00001";

    #[test]
    fn the_items_root_is_one_directory_under_the_store() {
        assert_eq!(items_root("/app/Library"), "/app/Library/items");
        assert_eq!(items_root("/app/Library/"), "/app/Library/items");
        assert_eq!(items_root("C:\\AppData\\Library\\"), "C:\\AppData\\Library/items");
    }

    #[test]
    fn a_book_owns_one_folder_named_by_its_id() {
        assert_eq!(
            item_dir("/app/Library/items", ID),
            format!("/app/Library/items/{ID}")
        );
        assert_eq!(
            item_dir("/app/Library/items/", ID),
            format!("/app/Library/items/{ID}")
        );
    }

    #[test]
    fn a_stored_book_s_bytes_are_source_under_its_extension() {
        assert_eq!(
            source_path("/app/Library/items", ID, "pdf"),
            format!("/app/Library/items/{ID}/source.pdf")
        );
        assert_eq!(
            source_path("/app/Library/items", ID, "md"),
            format!("/app/Library/items/{ID}/source.md")
        );
        assert_eq!(
            source_path("/app/Library/items", ID, ""),
            format!("/app/Library/items/{ID}/source")
        );
    }

    #[test]
    fn the_cover_marks_and_meta_live_beside_the_source() {
        let dir = format!("/app/Library/items/{ID}");
        assert_eq!(cover_path("/app/Library/items", ID), format!("{dir}/{COVER_FILE}"));
        assert_eq!(marks_path("/app/Library/items", ID), format!("{dir}/{MARKS_FILE}"));
        assert_eq!(meta_path("/app/Library/items", ID), format!("{dir}/{META_FILE}"));
        // Every file a book owns is inside the one folder its id names, which is the
        // whole of the colocation: a merge or a delete is one directory.
        for path in [
            source_path("/app/Library/items", ID, "pdf"),
            cover_path("/app/Library/items", ID),
            marks_path("/app/Library/items", ID),
            meta_path("/app/Library/items", ID),
        ] {
            assert!(path.starts_with(&format!("{dir}/")), "{path} escapes {dir}");
        }
    }

    #[test]
    fn a_linked_and_a_stored_book_share_one_folder_shape() {
        // The point of giving a linked book an item folder too: its cover and marks
        // land where a stored book's do, so "where does this book's stuff live" does
        // not branch on the origin. Only `source.*` is the stored book's alone.
        let stored = item_dir("/app/Library/items", ID);
        assert_eq!(cover_path("/app/Library/items", ID), format!("{stored}/{COVER_FILE}"));
        assert_eq!(marks_path("/app/Library/items", ID), format!("{stored}/{MARKS_FILE}"));
    }

    #[test]
    fn an_id_cannot_escape_its_folder() {
        // Defence in depth: the crate mints an id as an alphanumeric token, but a
        // hand-edited blob must not turn a folder name into a traversal.
        let root = "/app/Library/items";
        assert_eq!(item_dir(root, "../../etc"), format!("{root}/_.._etc"));
        assert_eq!(item_dir(root, "a/b\\c:d"), format!("{root}/a_b_c_d"));
        assert_eq!(item_dir(root, "../../x"), format!("{root}/_.._x"));
        assert_eq!(item_dir(root, "..."), format!("{root}/item"));
        assert_eq!(item_dir(root, ""), format!("{root}/item"));
        assert_eq!(item_dir(root, "  "), format!("{root}/item"));
    }

    #[test]
    fn a_control_character_never_reaches_a_folder_name() {
        assert_eq!(
            item_dir("/r", "a\u{0}b\u{1f}c"),
            "/r/a_b_c"
        );
    }

    #[test]
    fn a_long_id_is_capped_not_truncated_into_nothing() {
        let long = "a".repeat(400);
        assert_eq!(component(&long).chars().count(), 64);
        assert_eq!(item_dir("/r", &long), format!("/r/{}", "a".repeat(64)));
    }

    #[test]
    fn an_extension_is_sanitised_like_any_other_component() {
        assert_eq!(
            source_path("/r", ID, "pdf/../../x"),
            format!("/r/{ID}/source.pdf_.._.._x")
        );
    }


    #[test]
    fn a_migrated_copy_is_named_after_its_pipeline_not_its_source() {
        assert_eq!(migrated_ext("pdf"), Some("pdf"));
        assert_eq!(migrated_ext("TXT"), Some("text"));
        assert_eq!(migrated_ext("markdown"), Some("markdown"));
        assert_eq!(migrated_ext("mdown"), Some("markdown"));
        assert_eq!(migrated_ext("epub"), None);
        assert_eq!(migrated_ext(""), None);
    }

    #[test]
    fn the_id_an_old_copy_was_named_with_is_the_token_after_its_last_underscore() {
        assert_eq!(flat_store_id("dune_ab12cd.pdf").as_deref(), Some("ab12cd"));
        assert_eq!(
            flat_store_id("my_big_book_b018c4f9e2a0.pdf").as_deref(),
            Some("b018c4f9e2a0")
        );
        assert_eq!(flat_store_id("dune.pdf"), None);
        assert_eq!(flat_store_id("dune_.pdf"), None, "an empty id is no id");
        assert_eq!(flat_store_id(""), None);
        assert_eq!(flat_store_id("dune_ab12").as_deref(), Some("ab12"));
    }

    #[test]
    fn only_the_three_format_buckets_are_the_old_layout() {
        let root = "/app/Library";
        assert!(is_flat_store_path(root, "/app/Library/pdf/dune_ab12.pdf"));
        assert!(is_flat_store_path(root, "/app/Library/text/notes_ab12.txt"));
        assert!(is_flat_store_path(root, "/app/Library/markdown/a_ab12.md"));
        assert!(!is_flat_store_path(root, &source_path("/app/Library/items", ID, "pdf")));
        assert!(!is_flat_store_path(root, "/app/Library/items/x/y.pdf"));
        assert!(!is_flat_store_path(root, "/app/Library/other/a_ab12.epub"));
        assert!(!is_flat_store_path(root, "/books/dune.pdf"));
        assert!(!is_flat_store_path(root, "/app/Library-old/pdf/dune_ab12.pdf"));
        assert!(is_flat_store_path("/app/Library/", "/app/Library/pdf/dune_ab12.pdf"));
        assert!(!is_flat_store_path(root, "/app/Library/pdf/2024/dune_ab12.pdf"));
    }

    #[test]
    fn a_migrated_address_is_the_item_path_under_its_new_name() {
        let root = "/app/Library";
        let old = format!("/app/Library/pdf/my_big_book_{ID}.pdf");
        assert!(is_flat_store_path(root, &old));
        let id = flat_store_id(&format!("my_big_book_{ID}.pdf")).expect("a seam");
        assert_eq!(id, ID, "the seam is the id the copy was named with");
        let ext = migrated_ext("pdf").expect("a known format");
        assert_eq!(
            source_path(&items_root(root), &id, ext),
            format!("/app/Library/items/{ID}/source.pdf")
        );
    }
}
