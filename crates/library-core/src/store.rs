//! Where a book's bytes live on disk: one folder per book, keyed by an id
//! that never changes.
//!
//! The folder is the unit a removal sweeps whole, and nothing on disk wears a
//! title: renaming a book moves nothing. Covers and highlights are NOT files
//! here — they persist in the app's own storage — so the only byte this module
//! names is the stored source.

const ITEMS_DIR: &str = "items";

/// The stem a stored book's bytes wear inside its item folder (`source.pdf`,
/// `source.md`): one predictable name, the format carried by the suffix.
const SOURCE_STEM: &str = "source";

/// The item-folder root under a store root: `<store_root>/items`. Keeping
/// everything below one root is what the delete command's containment check
/// guards.
pub fn items_root(store_root: &str) -> String {
    join(trim_sep(store_root), ITEMS_DIR)
}

/// The folder one book owns: `<items_root>/<id>`. The id is sanitised into a
/// single component rather than trusted: a hand-edited blob must not turn a
/// folder name into a traversal.
fn item_dir(items_root: &str, book_id: &str) -> String {
    join(trim_sep(items_root), &component(book_id))
}

/// Where a stored book's bytes live: `<items_root>/<id>/source.<ext>`. An
/// empty extension yields a bare `source`; no admitted format asks for one —
/// the registry refuses an extension-less name.
pub fn source_path(items_root: &str, book_id: &str, ext: &str) -> String {
    // The emptiness check is on the raw extension: `component` maps an empty
    // string to its fallback, which is right for a folder name and wrong for
    // "this source has no suffix".
    let file = if ext.trim().is_empty() {
        SOURCE_STEM.to_string()
    } else {
        format!("{SOURCE_STEM}.{}", component(ext))
    };
    join(&item_dir(items_root, book_id), &file)
}

/// The extension a migrated source keeps in its new name: every supported
/// extension lower-cased. `None` for one the registry does not know, which
/// keeps its old name rather than being renamed into something no reader can
/// open.
pub fn migrated_ext(ext: &str) -> Option<&'static str> {
    match crate::scan::store_dir(ext) {
        "other" => None,
        dir => Some(dir),
    }
}

/// Drop trailing separators so a join never produces `root//child`. Both
/// separators are trimmed: a store root arrives from the host's path API.
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

/// One path component, made safe to write: separators, Windows-reserved
/// characters and control characters become `_`; the result is trimmed of the
/// dots and spaces that would make it relative, capped, and replaced with a
/// fallback when nothing is left.
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
        assert_eq!(
            items_root("C:\\AppData\\Library\\"),
            "C:\\AppData\\Library/items"
        );
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
    fn the_source_lives_inside_the_folder_the_id_names() {
        let dir = format!("/app/Library/items/{ID}");
        let path = source_path("/app/Library/items", ID, "pdf");
        assert!(path.starts_with(&format!("{dir}/")), "{path} escapes {dir}");
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
        assert_eq!(item_dir("/r", "a\u{0}b\u{1f}c"), "/r/a_b_c");
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
    fn a_migrated_address_is_the_item_path_under_its_new_name() {
        let root = "/app/Library";
        let ext = migrated_ext("pdf").expect("a known format");
        assert_eq!(
            source_path(&items_root(root), ID, ext),
            format!("/app/Library/items/{ID}/source.pdf")
        );
    }
}
