//! What a book is called, and what to call the second one.

pub fn stem_of(path: &str) -> String {
    reader_core::filename::file_stem_from_path(path).unwrap_or_else(|| path.to_string())
}

/// The next free duplicate of `base`: `base_1`, `base_2`, and so on.
///
/// `in_use` is every name the library already shows. A trailing `_N` on `base`
/// is stripped before counting, so duplicating a duplicate steps instead of
/// stacking: "Dune_1" becomes "Dune_2", the same reading a file manager gives
/// it, where the counter is not part of the name. Never empty: a blank `base`
/// falls back to a word.
pub fn duplicate_title(base: &str, in_use: &std::collections::HashSet<String>) -> String {
    let base = base.trim();
    let root = if base.is_empty() { "Book" } else { base };
    // The counter rule is the filename policy's, not this crate's: a second
    // spelling of it here would be a second convention the moment either half
    // was edited.
    let root = reader_core::filename::strip_copy_counter(root);
    (1u32..)
        .map(|n| format!("{root}_{n}"))
        .find(|candidate| !in_use.contains(candidate))
        .expect("an unbounded counter always finds a free name")
}
