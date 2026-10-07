//! Getting a raw file to a shape every parser can split on.

/// Normalise a raw file: drop the BOM, fold CRLF, trim trailing space.
pub fn normalize(raw: &str) -> String {
    let stripped = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    // Fold CRLF first, then lone CRs, so every line ending becomes one LF.
    let folded = stripped.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(folded.len());
    for (i, line) in folded.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line.trim_end_matches([' ', '\t']));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize;

    #[test]
    fn line_endings_fold_and_the_bom_goes() {
        assert_eq!(normalize("\u{feff}a\r\nb\rc\nd"), "a\nb\nc\nd");
        assert_eq!(normalize("x  \ny\t"), "x\ny");
        // A hard-break marker looks like an empty line, so trailing spaces go.
        assert_eq!(normalize("a  "), "a");
        // Nothing else is touched: indentation is content.
        assert_eq!(normalize("  indented\n\ttabbed"), "  indented\n\ttabbed");
        assert_eq!(normalize(""), "");
    }
}
