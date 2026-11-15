//! `cargo xtask trapped`: no toolkit-free file hides in the front-end crate.
//!
//! A file belongs in the engine when it names no toolkit, and the gate says so
//! outright: nothing may be left (`docs/architecture.md` § Layers). It counted
//! against a committed number while the moves were still ahead; the number is
//! gone with them. What may stay is allowlisted, and each of those is the front
//! end's own: its root and its glyph table.
//!
//! Only `src/` counts. `tests/` and `examples/` are harnesses: a test binary's
//! root names no toolkit and never will, and no engine should hold it.

use std::path::{Path, PathBuf};

/// Files that may name no toolkit without leaving the front end.
///
/// Each is presentation with nowhere to go: the crate's root and its glyph
/// table.
pub const ALLOWED: &[&str] = &["lib.rs", "icons/pack.rs"];

/// Every word that means a toolkit crate is in play.
const TOOLKIT: &[&str] = &[
    "egui",
    "eframe",
    "egui_extras",
    "egui_kittest",
    "wgpu",
    "winit",
    "rfd",
];

/// True when `text` names a toolkit crate anywhere, comments included: a file
/// that explains egui is still a file about pixels.
pub fn names_toolkit(text: &str) -> bool {
    TOOLKIT.iter().any(|word| text.contains(word))
}

/// The `.rs` files under `root` that name no toolkit, by path from `root`.
pub fn free_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            if !names_toolkit(&text)
                && let Ok(rel) = path.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
}

/// The files that are trapped, given what is found and what is allowed.
pub fn trapped(found: &[String], allowed: &[&str]) -> Vec<String> {
    found
        .iter()
        .filter(|rel| !allowed.iter().any(|a| a == rel))
        .cloned()
        .collect()
}

fn source_dir() -> PathBuf {
    crate::root().join("crates/ui-egui/src")
}

pub fn run(_: &[String]) -> Result<(), String> {
    let found = free_files(&source_dir());
    let left = trapped(&found, ALLOWED);
    println!(
        "trapped: {} toolkit-free file(s); none may stay",
        left.len()
    );
    if left.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} file(s) name no toolkit and belong below the front end:\n  {}",
        left.len(),
        left.join("\n  ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    #[test]
    fn a_file_that_explains_egui_is_still_about_pixels() {
        assert!(names_toolkit("//! A view over egui's Ui."));
        assert!(names_toolkit("use egui::Context;"));
        assert!(names_toolkit("let frame = eframe::Frame;"));
        assert!(!names_toolkit("//! Windows and doors."));
        assert!(!names_toolkit("use mareader_session::Session;"));
    }

    #[test]
    fn the_allowlisted_files_are_not_trapped() {
        let found = files(&["lib.rs", "icons/pack.rs", "export.rs", "library/picks.rs"]);
        let left = trapped(&found, ALLOWED);
        assert_eq!(left, files(&["export.rs", "library/picks.rs"]));
    }

    #[test]
    fn nothing_is_trapped_when_the_crate_is_all_views() {
        assert!(trapped(&files(&["lib.rs"]), ALLOWED).is_empty());
        assert!(trapped(&[], ALLOWED).is_empty());
    }

    /// The allowlist is the whole of what may stay, so it cannot grow quietly.
    #[test]
    fn two_files_may_name_no_toolkit() {
        assert_eq!(ALLOWED, &["lib.rs", "icons/pack.rs"]);
        // One more toolkit-free file, and the gate fails on it by name.
        let left = trapped(&files(&["app/options.rs"]), ALLOWED);
        assert_eq!(left, files(&["app/options.rs"]));
    }
}
