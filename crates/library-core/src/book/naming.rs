//! What a book is called, and what to call the second one.

pub fn stem_of(path: &str) -> String {
    document_core::filename::file_stem_from_path(path).unwrap_or_else(|| path.to_string())
}

/// The next free duplicate of `base`: `base_1`, `base_2`, and so on.
///
/// A trailing `_N` is stripped before counting, so duplicating a duplicate
/// steps instead of stacking ("Dune_1" becomes "Dune_2"), matching file
/// managers. Never empty: a blank `base` falls back to "Book".
pub fn duplicate_title(base: &str, in_use: &std::collections::HashSet<String>) -> String {
    let base = base.trim();
    let root = if base.is_empty() { "Book" } else { base };
    // The counter rule belongs to the filename policy; a second spelling
    // here would be a second convention the moment either half was edited.
    let root = document_core::filename::strip_copy_counter(root);
    (1u32..)
        .map(|n| format!("{root}_{n}"))
        .find(|candidate| !in_use.contains(candidate))
        .expect("an unbounded counter always finds a free name")
}
