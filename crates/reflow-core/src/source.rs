//! Getting a raw file to a shape every parser can split on.
//!
//! Normalisation is not a plain-text concern: the block splitter counts blank
//! lines to find boundaries, so a BOM, a CRLF or a line of trailing spaces is
//! the difference between one paragraph and two, in every format. Both
//! `txt-core` and `md-core` normalise through here before they parse, and the
//! Markdown renderer still sees the hard-break markers it cares about because
//! only *trailing whitespace that renders as nothing* is removed.

/// Normalise a raw file for parsing: drop the UTF-8 BOM, fold CRLF/CR to
/// LF, and trim trailing whitespace off every line (a trailing double space
/// is a Markdown hard break the renderer still gets from the line's own
/// content — the trim only removes the invisible kind that makes empty
/// lines look non-empty).
pub fn normalize(raw: &str) -> String {
    let stripped = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    // Fold CRLF first, then lone CRs (old macOS): every line ending becomes
    // exactly one LF, so the split never yields phantom blank lines.
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
        // A hard-break marker (two trailing spaces) would look like an empty
        // line to a blank-line splitter, so trailing spaces are trimmed even
        // though the renderer's own content keeps the break.
        assert_eq!(normalize("a  "), "a");
        // Nothing else is touched: indentation is content.
        assert_eq!(normalize("  indented\n\ttabbed"), "  indented\n\ttabbed");
        assert_eq!(normalize(""), "");
    }
}
