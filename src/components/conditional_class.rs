//! Compile-time-adjacent check: each `class=("…", cond)` token must be a
//! single class name. A space-separated value throws a swallowed SyntaxError.

#[cfg(test)]
mod tests {
    use std::path::Path;

    fn walk_rs(dir: &Path, out: &mut Vec<(String, usize, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk_rs(&path, out);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in src.lines().enumerate() {
                let Some(rest) = line.split("class=(\"").nth(1) else {
                    continue;
                };
                let Some(token) = rest.split('"').next() else {
                    continue;
                };
                if token.contains(' ') {
                    out.push((path.display().to_string(), i + 1, token.to_string()));
                }
            }
        }
    }

    #[test]
    fn no_multi_token_conditional_classes() {
        let mut hits = Vec::new();
        walk_rs(Path::new("src"), &mut hits);
        assert!(
            hits.is_empty(),
            "conditional class tokens must be a single name:\n{}",
            hits.iter()
                .map(|(p, n, t)| format!("{p}:{n}: {t:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
