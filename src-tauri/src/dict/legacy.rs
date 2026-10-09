//! Older cache paths and one-time import markers.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

const OLD_IDENTIFIER: &str = "com.codewiththiha.pdfreader";

/// Whether this pack's old store has already been handled or removed.
pub fn is_marked(dir: &Path, pack_id: &str) -> bool {
    marker(dir, pack_id).is_file()
}

/// Remember that this pack must not be resurrected from an old store.
pub fn mark(dir: &Path, pack_id: &str) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(marker(dir, pack_id), b"handled");
}

/// Allow a future download to take precedence over old cache files.
pub fn unmark(dir: &Path, pack_id: &str) {
    let _ = std::fs::remove_file(marker(dir, pack_id));
}

fn marker(dir: &Path, pack_id: &str) -> PathBuf {
    dir.join(format!(".{pack_id}.legacy-imported"))
}

fn add_roots(roots: &mut Vec<PathBuf>, base: &Path) {
    roots.extend([
        base.to_path_buf(),
        base.join("dict"),
        base.join("cefr"),
        base.join("datasets"),
        base.join("datasets").join("dict"),
        base.join("dataset_cache"),
        base.join("dataset_cache").join("dict"),
        base.join("dataset-cache"),
        base.join("dataset-cache").join("dict"),
        base.join("cache").join("dict"),
    ]);
}

/// SQLite files in the current and retired app-data locations.
pub fn candidates(app: &AppHandle, dict_dir: &Path, pack_id: &str, pair: &str) -> Vec<PathBuf> {
    let mut roots = vec![dict_dir.to_path_buf()];
    if let Some(data_dir) = dict_dir.parent() {
        add_roots(&mut roots, data_dir);
        if let Some(parent) = data_dir.parent() {
            let retired = parent.join(OLD_IDENTIFIER);
            add_roots(&mut roots, &retired);
        }
    }
    if let Ok(cache_dir) = app.path().app_cache_dir() {
        add_roots(&mut roots, &cache_dir);
        if let Some(parent) = cache_dir.parent() {
            let retired = parent.join(OLD_IDENTIFIER);
            add_roots(&mut roots, &retired);
        }
    }

    let mut seen_roots = HashSet::new();
    let mut paths = Vec::new();
    for root in roots {
        if !seen_roots.insert(root.clone()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_sqlite_name(&path) {
                continue;
            }
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
    }

    paths.sort_by_key(|path| candidate_rank(path, pack_id, pair));
    paths
}

fn candidate_rank(path: &Path, pack_id: &str, pair: &str) -> u8 {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let stem = normalize(stem);
    if stem == normalize(pack_id) {
        0
    } else if stem == normalize(pair) {
        1
    } else {
        2
    }
}

fn is_sqlite_name(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("db" | "sqlite" | "sqlite3")
    )
}

/// A per-pack filename proves rows without a `pack` column belong here.
pub fn filename_matches(path: &Path, pack_id: &str, pair: &str) -> bool {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let stem = normalize(stem);
    stem == normalize(pack_id) || stem == normalize(pair)
}

fn normalize(value: &str) -> String {
    value.to_ascii_lowercase().replace('_', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_and_extensions_are_normalized() {
        assert!(filename_matches(
            Path::new("/old/en_my.sqlite"),
            "mcfnlp-en-my",
            "en-my"
        ));
        assert!(filename_matches(
            Path::new("/old/mcfnlp-en-my.db"),
            "mcfnlp-en-my",
            "en-my"
        ));
        assert!(!filename_matches(
            Path::new("/old/dictionary.db"),
            "mcfnlp-en-my",
            "en-my"
        ));
        assert!(is_sqlite_name(Path::new("old.sqlite3")));
        assert!(!is_sqlite_name(Path::new("body.parquet")));
    }
}
