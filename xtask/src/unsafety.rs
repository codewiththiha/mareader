//! `cargo xtask unsafe`: only listed files may allow `unsafe_code`.

use std::fs;
use std::path::Path;

/// Files that may lift the lint, each reviewed by the owner.
const ALLOWED: &[&str] = &[
    "apps/mareader/src/traffic_lights.rs",
    "apps/mareader/src/open_files.rs",
    "crates/speech/src/mac_voices.rs",
    "crates/memory/src/lib.rs",
    "crates/nn-core/src/simd.rs",
];

/// This checker, which names the lifts it looks for.
const CHECKER: &str = "xtask/src/unsafety.rs";

/// Lint levels that would let `unsafe` through.
const LIFTS: &[&str] = &[
    "allow(unsafe_code)",
    "warn(unsafe_code)",
    "expect(unsafe_code)",
];

/// Problems in the workspace manifest, member manifests and Rust files.
pub fn check(
    root: &str,
    members: &[(String, String)],
    sources: &[(String, String)],
) -> Vec<String> {
    let mut out = Vec::new();
    if !root.contains("unsafe_code = \"deny\"") && !root.contains("unsafe_code = \"forbid\"") {
        out.push("Cargo.toml: the workspace must deny unsafe_code".into());
    }
    for (path, text) in members {
        if !text.contains("[lints]\nworkspace = true") {
            out.push(format!("{path}: needs [lints] workspace = true"));
        }
    }
    for (path, text) in sources {
        if ALLOWED.contains(&path.as_str()) || path == CHECKER {
            continue;
        }
        if LIFTS.iter().any(|lift| text.contains(lift)) {
            out.push(format!("{path}: lifts unsafe_code; only {ALLOWED:?} may"));
        }
    }
    out
}

/// Rust files below `dir`, as paths relative to the root.
fn rust_files(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(root, &path, out)?;
            }
        } else if name.ends_with(".rs") {
            let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let rel = path.strip_prefix(root).unwrap_or(&path);
            out.push((rel.to_string_lossy().replace('\\', "/"), text));
        }
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn run(_: &[String]) -> Result<(), String> {
    let root = crate::root();
    let mut members = Vec::new();
    let mut sources = Vec::new();
    for group in ["crates", "apps"] {
        let entries = fs::read_dir(root.join(group)).map_err(|e| format!("{group}: {e}"))?;
        for entry in entries {
            let dir = entry.map_err(|e| e.to_string())?.path();
            let manifest = dir.join("Cargo.toml");
            if manifest.is_file() {
                let rel = manifest.strip_prefix(&root).unwrap_or(&manifest);
                members.push((rel.to_string_lossy().into_owned(), read(&manifest)?));
            }
        }
        rust_files(&root, &root.join(group), &mut sources)?;
    }
    members.push((
        "xtask/Cargo.toml".into(),
        read(&root.join("xtask/Cargo.toml"))?,
    ));
    rust_files(&root, &root.join("xtask"), &mut sources)?;
    let problems = check(&read(&root.join("Cargo.toml"))?, &members, &sources);
    if problems.is_empty() {
        println!(
            "unsafe: {} files, only {} may allow it",
            sources.len(),
            ALLOWED.len()
        );
        Ok(())
    } else {
        Err(problems.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "[workspace.lints.rust]\nunsafe_code = \"deny\"\n";
    const MEMBER: &str = "[lints]\nworkspace = true\n";

    fn one(path: &str, text: &str) -> Vec<(String, String)> {
        vec![(path.into(), text.into())]
    }

    #[test]
    fn the_audited_module_may_allow_it() {
        let src = one(ALLOWED[0], "#[allow(unsafe_code)]\nmod mac {}");
        assert!(check(ROOT, &one("a/Cargo.toml", MEMBER), &src).is_empty());
    }

    #[test]
    fn any_other_file_may_not() {
        let src = one("crates/ui-egui/src/x.rs", "#![allow(unsafe_code)]");
        assert_eq!(check(ROOT, &[], &src).len(), 1);
        let src = one("crates/ui-egui/src/x.rs", "#[expect(unsafe_code)]");
        assert_eq!(check(ROOT, &[], &src).len(), 1);
    }

    #[test]
    fn the_workspace_lint_and_member_opt_in_are_required() {
        let problems = check("", &one("crates/x/Cargo.toml", "[package]\n"), &[]);
        assert_eq!(problems.len(), 2);
    }
}
