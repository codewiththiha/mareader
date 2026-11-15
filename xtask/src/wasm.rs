//! `cargo xtask wasm`: L0–L5, the front end and the web app check for wasm32.
//!
//! Rule 3 of `docs/architecture.md` said a gate would arrive with the first L1
//! crate; this is it. The crate list comes from [`crate::layers::TABLE`], so a
//! crate that gains a row is checked the same commit, and a core crate listed
//! here that is not is a gate failure rather than a silent gap.

use std::process::Command;

use crate::layers::{Layer, TABLE, short};

/// What may not be checked for wasm32, and why. Each entry is a decision the
/// owner made, not an oversight: these crates are documented desktop-only.
pub const EXEMPT: &[(&str, &str)] = &[(
    "assist",
    "AI needs threads, a keychain and TLS; docs/architecture.md rule 3 exempts it",
)];

/// Crates the browser links, asked of cargo rather than read off the layer
/// table: a crate is worth a wasm check when the web app pulls it in, and
/// nothing else is. `speech` and `ui-egui` declare their model and script
/// crates desktop-only, so those are in no wasm tree.
fn reachable() -> Result<Vec<String>, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "tree",
            "-q",
            "--locked",
            "-p",
            "mareader-web",
            "--target",
            "wasm32-unknown-unknown",
            "-e",
            "normal",
            "--prefix",
            "none",
        ])
        .current_dir(crate::root())
        .output()
        .map_err(|e| format!("cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let mut names: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|n| n.starts_with("mareader-"))
        .map(|n| n.to_owned())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// The targets the gate builds, in layer order; the front end and web app last.
pub fn targets(reachable: &[String]) -> Vec<String> {
    let mut names: Vec<(u8, String)> = TABLE
        .iter()
        .filter_map(|(name, layer)| match layer {
            Layer::Core(n) if *n <= 5 => Some((*n, (*name).to_owned())),
            Layer::Front => Some((6, (*name).to_owned())),
            _ => None,
        })
        .filter(|(_, name)| !EXEMPT.iter().any(|(skip, _)| short(skip) == name))
        .filter(|(_, name)| reachable.iter().any(|r| r == &full_name(name)))
        .collect();
    names.sort();
    let mut out: Vec<String> = names
        .into_iter()
        .map(|(_, name)| full_name(&name))
        .collect();
    out.push("mareader-web".to_owned());
    out
}

/// A row of the layer table as the package it names.
fn full_name(name: &str) -> String {
    if name == "ui-egui" {
        "mareader-ui-egui".to_owned()
    } else {
        format!("mareader-{name}")
    }
}

fn check(package: &str) -> Result<(), String> {
    println!("== wasm: {package}");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "check",
            "-q",
            "--locked",
            "-p",
            package,
            "--target",
            "wasm32-unknown-unknown",
        ])
        .current_dir(crate::root())
        .output()
        .map_err(|e| format!("cargo check {package}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(12).collect();
        let tail: Vec<&&str> = tail.iter().rev().collect();
        Err(format!(
            "{package} does not build for wasm32:\n  {}",
            tail.iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n  ")
        ))
    }
}

/// Every target, stopping at the first that fails: one answer, not a wall.
pub fn run(_: &[String]) -> Result<(), String> {
    let reachable = reachable()?;
    let list = targets(&reachable);
    println!("wasm: {} crates for wasm32-unknown-unknown", list.len());
    for package in &list {
        check(package)?;
    }
    println!("wasm: {} crates ok", list.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the browser links, as `cargo tree` reports it for the real app.
    fn linked() -> Vec<String> {
        [
            "engine",
            "appearance",
            "session",
            "settings",
            "library",
            "ui-egui",
        ]
        .iter()
        .map(|n| full_name(n))
        .collect()
    }

    #[test]
    fn the_core_and_the_front_end_are_every_target_below_the_apps() {
        let list = targets(&linked());
        for name in ["mareader-appearance", "mareader-session", "mareader-engine"] {
            assert!(list.iter().any(|t| t == name), "{name} missing");
        }
        assert!(list.iter().any(|t| t == "mareader-ui-egui"));
        assert_eq!(list.last().map(String::as_str), Some("mareader-web"));
        assert!(
            !list.iter().any(|t| t == "mareader"),
            "an app is not a crate here"
        );
    }

    #[test]
    fn a_crate_the_browser_never_links_is_not_checked_alone() {
        // Desktop-only by `speech`'s own declaration, so in no wasm tree.
        let list = targets(&linked());
        for gone in ["mareader-kokoro", "mareader-vits", "mareader-phonon"] {
            assert!(
                !list.iter().any(|t| t == gone),
                "{gone} should not be built"
            );
        }
    }

    #[test]
    fn a_desktop_only_crate_is_exempt_by_name_not_by_layer() {
        let list = targets(&linked());
        assert!(!list.iter().any(|t| t == "mareader-assist"));
        assert!(
            EXEMPT
                .iter()
                .all(|(name, why)| !why.is_empty() && short(name) == *name)
        );
    }

    #[test]
    fn the_list_is_unique_and_ordered_by_layer() {
        let list = targets(&linked());
        // Layer order, which is what the function promises; the web app last.
        let mut seen: u8 = 0;
        for (at, target) in list.iter().enumerate() {
            let rank = TABLE
                .iter()
                .find(|(name, _)| full_name(name) == *target)
                .map(|(_, layer)| layer.rank());
            let Some(r) = rank else {
                panic!("{list:?}: {target} has no layer row");
            };
            assert!(r >= seen, "{list:?}: out of layer order at L{r}");
            assert_eq!(at == list.len() - 1, r == 7, "{list:?}: the web app last");
            seen = r;
        }
        let unique: std::collections::BTreeSet<&String> = list.iter().collect();
        assert_eq!(unique.len(), list.len());
    }
}
