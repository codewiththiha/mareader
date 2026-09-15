//! Path-part spelling: the one place a file name, a stem, an extension or a
//! folder label is taken apart.
//!
//! These four jobs used to be done by six functions in three idioms across
//! the services, the shell and the import, and a Windows path answered
//! differently depending on which door it came in. Everything here is pure,
//! host-tested, and shared by the frontend and the shell.

/// The last segment of a path, either separator, no trailing empties: what a
/// shelf shows a file by. Empty for an all-separator path and for a bare drive
/// root (`C:\`) — a root has no last segment to show.
pub fn file_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    // A drive root only looks like a segment once its trailing separator is
    // trimmed: "C:" is a letter wearing its colon, not a name. The letter is
    // checked because a colon is legal in a Unix name — "ab:" is one.
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return String::new();
    }
    trimmed.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// The name without its extension: the title a document wears when it
/// supplies none. A dotfile (`.gitignore`) is all stem.
pub fn file_stem(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_string(),
        _ => name,
    }
}

/// Lower case, no dot: the key the format registry answers by. Empty for a
/// name with no extension (`Makefile`, `.gitignore`).
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// The last segment a folder goes by, falling back to the whole path for a
/// root ("/", "C:\\") that has no segment to show.
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
