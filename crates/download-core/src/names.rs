//! The names a download may be given, and the ones it may not.

/// A file name that cannot leave the directory it is joined to.
pub fn safe_file_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return None;
    }
    if trimmed
        .chars()
        .any(|c| matches!(c, '/' | '\\' | '\0' | ':'))
    {
        return None;
    }
    Some(trimmed.to_string())
}

/// The file name a URL's path ends in, when it names one.
pub fn file_name_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    safe_file_name(path.rsplit('/').next()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_name_survives_and_is_trimmed() {
        assert_eq!(
            safe_file_name("cefr.parquet"),
            Some("cefr.parquet".to_string())
        );
        assert_eq!(safe_file_name("  a.bin "), Some("a.bin".to_string()));
    }

    #[test]
    fn nothing_that_leaves_its_directory_survives() {
        for name in [
            "",
            "   ",
            ".",
            "..",
            "../secret",
            "..\\secret",
            "a/b",
            "a\\b",
            "C:evil",
            "/abs",
        ] {
            assert_eq!(safe_file_name(name), None, "{name} must be refused");
        }
        assert_eq!(safe_file_name("a\0b"), None);
    }

    #[test]
    fn a_url_answers_its_last_path_segment() {
        assert_eq!(
            file_name_from_url("https://host/data/cefr.zstd.parquet"),
            Some("cefr.zstd.parquet".to_string())
        );
        assert_eq!(
            file_name_from_url("https://host/a/b.bin?token=1#frag"),
            Some("b.bin".to_string())
        );
        // A directory URL names nothing.
        assert_eq!(file_name_from_url("https://host/data/"), None);
        // A traversal in a URL still yields a bare name, never a path.
        assert_eq!(
            file_name_from_url("https://host/../etc/passwd"),
            Some("passwd".to_string())
        );
    }
}
