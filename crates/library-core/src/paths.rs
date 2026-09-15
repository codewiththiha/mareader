//! Path-part spelling: the one place a file name, a stem, an extension or a folder label is
//! taken apart.
//!
//! Six functions in three idioms used to do these four jobs — `rsplit('/')` in the services,
//! `Path::extension` in the shell, `rsplit_once('.')` in the import — and a Windows path
//! answered differently depending on which door it came in. Everything here is pure and
//! host-tested, and both the frontend and the shell depend on this crate already, so the
//! spelling is shared rather than mirrored.

/// The last segment of a path, either separator, no trailing empties: what a shelf shows a
/// file by. Empty for a path that is all separator.
pub fn file_name(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// The name without its extension: the title a document wears when it supplies none.
/// A dotfile (`.gitignore`) is all stem — the format registry refuses it either way.
pub fn file_stem(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name,
    }
}

/// Lower case, no dot: the key the format registry answers by. Empty for a name Rust reads
/// as having no extension (`Makefile`, `.gitignore`).
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// The last segment a folder goes by, falling back to the whole path for a root ("/",
/// "C:\\") that has no last segment to show.
pub fn dir_label(path: &str) -> String {
    let name = file_name(path);
    if name.is_empty() {
        path.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_named_by_its_last_segment_either_separator() {
        assert_eq!(file_name("/Users/me/Dune.pdf"), "Dune.pdf");
        assert_eq!(file_name("/Users/me/Books/"), "Books");
        assert_eq!(file_name("C:\\Users\\me\\Books"), "Books");
        assert_eq!(file_name("Dune.pdf"), "Dune.pdf");
    }

    #[test]
    fn a_root_has_no_name_but_a_label() {
        assert_eq!(file_name("/"), "");
        assert_eq!(dir_label("/"), "/");
        assert_eq!(dir_label("C:\\"), "C:\\");
        assert_eq!(dir_label("/Users/me/Books"), "Books");
        assert_eq!(dir_label("/Users/me/Books/"), "Books");
    }

    #[test]
    fn a_stem_is_the_name_without_its_extension() {
        assert_eq!(file_stem("/books/notes.markdown"), "notes");
        assert_eq!(file_stem("/books/.gitignore"), ".gitignore");
        assert_eq!(file_stem("/books/Makefile"), "Makefile");
    }

    #[test]
    fn an_extension_is_lower_case_and_has_no_dot() {
        assert_eq!(extension("/books/Dune.PDF"), "pdf");
        assert_eq!(extension("/books/notes.markdown"), "markdown");
        assert_eq!(extension("/books/Makefile"), "");
        assert_eq!(extension("/books/.gitignore"), "");
        assert_eq!(extension("C:\\books\\Report.Docx"), "docx");
    }
}
