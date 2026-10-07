//! Path-part spelling: name, extension and folder label, once.

/// The last segment of a path, either separator.
pub fn file_name(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    // A drive root is not a segment.
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return String::new();
    }
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_string()
}

/// Lower case, no dot: the registry's key.
pub fn extension(path: &str) -> String {
    let name = file_name(path);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// The last segment a folder goes by.
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
    fn an_extension_is_lower_case_and_has_no_dot() {
        assert_eq!(extension("/books/Dune.PDF"), "pdf");
        assert_eq!(extension("/books/notes.markdown"), "markdown");
        assert_eq!(extension("/books/Makefile"), "");
        assert_eq!(extension("/books/.gitignore"), "");
        assert_eq!(extension("C:\\books\\Report.Docx"), "docx");
    }
}
