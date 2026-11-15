//! `cargo xtask parity`: validates `parity/mareader-features.toml` and
//! reports how much of mareader the rewrite reproduces.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const FILE: &str = "parity/mareader-features.toml";
const SPECS_FILE: &str = "crates/engine/src/command_spec/rows.rs";

pub const AREAS: &[(&str, &str)] = &[
    ("A", "Window and chrome"),
    ("B", "Workspace and panes"),
    ("C", "Library"),
    ("D", "PDF viewing"),
    ("E", "EPUB reading"),
    ("F", "Appearance"),
    ("G", "Typography"),
    ("H", "Navigation and search"),
    ("I", "Study tools"),
    ("J", "Settings and persistence"),
    ("K", "Motion, keyboard and accessibility"),
    ("L", "Performance and memory"),
];
const TIERS: &[&str] = &["P0", "P1", "P2"];
const STATUSES: &[&str] = &["planned", "partial", "shipped", "dropped"];

#[derive(Debug, Deserialize)]
struct Doc {
    #[serde(rename = "feature", default)]
    features: Vec<Feature>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    pub id: String,
    pub title: String,
    pub area: String,
    pub tier: String,
    pub milestone: String,
    pub status: String,
    /// `path::test_fn` that proves it; required once shipped.
    #[serde(default)]
    pub evidence: Vec<String>,
    /// Engine command id in `SPECS`, when driven by a registered command.
    #[serde(default)]
    pub command: Option<String>,
    /// True for shipped features that belong purely to the UI layer.
    #[serde(default)]
    pub ui_only: bool,
    /// Where mareader implements it, relative to the mareader checkout.
    #[serde(default)]
    pub reference: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

/// Extracts every registered command id from `command_spec/rows.rs`.
pub fn parse_spec_ids(src: &str) -> HashSet<String> {
    let mut ids = HashSet::new();
    let mut rest = src;
    while let Some((before, after)) = rest.split_once("row(") {
        let boundary = before
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        rest = after;
        if !boundary {
            continue;
        }
        if let Some(after_quote) = rest.trim_start().strip_prefix('"')
            && let Some((id, tail)) = after_quote.split_once('"')
        {
            ids.insert(id.to_owned());
            rest = tail;
        }
    }
    ids
}

fn load_spec_ids(repo: &Path) -> Result<HashSet<String>, String> {
    let text =
        std::fs::read_to_string(repo.join(SPECS_FILE)).map_err(|e| format!("{SPECS_FILE}: {e}"))?;
    let ids = parse_spec_ids(&text);
    if ids.is_empty() {
        return Err(format!("{SPECS_FILE}: no registered commands found"));
    }
    Ok(ids)
}

fn valid_id(id: &str) -> bool {
    let Some((area, name)) = id.split_once('.') else {
        return false;
    };
    let kebab = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    kebab(area) && kebab(name)
}

fn valid_milestone(m: &str) -> bool {
    m.strip_prefix('M')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| n <= 8)
}

/// True when `path::name` names a file under `repo` that defines `fn name`.
fn evidence_exists(repo: &Path, cite: &str) -> bool {
    let Some((path, name)) = cite.rsplit_once("::") else {
        return false;
    };
    let Ok(src) = std::fs::read_to_string(repo.join(path)) else {
        return false;
    };
    src.contains(&format!("fn {name}("))
}

/// The mareader checkout, if one sits beside this repository.
fn reference_root(repo: &Path) -> Option<PathBuf> {
    let dir = std::env::var_os("MAREADER_DIR")
        .map(PathBuf::from)
        .or_else(|| repo.parent().map(|p| p.join("mareader")))?;
    dir.join("AGENTS.md").is_file().then_some(dir)
}

pub fn validate(
    features: &[Feature],
    repo: &Path,
    reference: Option<&Path>,
    specs: &HashSet<String>,
) -> Vec<String> {
    let mut errors = Vec::new();
    let mut ids = HashSet::new();
    for f in features {
        let mut e = |m: String| errors.push(format!("{}: {m}", f.id));
        if !ids.insert(f.id.as_str()) {
            e("duplicate id".into());
        }
        if !valid_id(&f.id) {
            e("id must look like `area.kebab-name`".into());
        }
        if f.title.trim().is_empty() {
            e("empty title".into());
        }
        if !AREAS.iter().any(|(a, _)| *a == f.area) {
            e(format!("unknown area {:?}; use A to L", f.area));
        }
        if !TIERS.contains(&f.tier.as_str()) {
            e(format!("tier {:?} must be one of {TIERS:?}", f.tier));
        }
        if !STATUSES.contains(&f.status.as_str()) {
            e(format!("status {:?} must be one of {STATUSES:?}", f.status));
        }
        if !valid_milestone(&f.milestone) {
            e(format!("milestone {:?} must be M0 to M8", f.milestone));
        }
        if f.command.is_some() && f.ui_only {
            e("cannot set both `command` and `ui_only`".into());
        }
        if let Some(cmd) = f.command.as_deref()
            && !specs.contains(cmd)
        {
            e(format!("command {cmd:?} is not in engine SPECS"));
        }
        if f.status == "shipped" {
            if f.evidence.is_empty() {
                e("shipped needs at least one `path::test_fn` in evidence".into());
            }
            if f.command.is_none() && !f.ui_only {
                e("shipped needs `command = \"<id>\"` or `ui_only = true`".into());
            }
        }
        if matches!(f.status.as_str(), "partial" | "dropped") && f.notes.trim().is_empty() {
            e(format!(
                "{} needs notes saying what is missing or why",
                f.status
            ));
        }
        for cite in f.evidence.iter().filter(|c| !evidence_exists(repo, c)) {
            e(format!("evidence {cite:?} names no such file or fn"));
        }
        if let Some(root) = reference {
            for r in f.reference.iter().filter(|r| !root.join(r).exists()) {
                e(format!("reference {r:?} is not in the mareader checkout"));
            }
        }
    }
    errors
}

fn load(repo: &Path) -> Result<Vec<Feature>, String> {
    let text = std::fs::read_to_string(repo.join(FILE)).map_err(|e| format!("{FILE}: {e}"))?;
    toml::from_str::<Doc>(&text)
        .map(|d| d.features)
        .map_err(|e| format!("{FILE}: {e}"))
}

fn report(features: &[Feature]) {
    let score = |fs: &[&Feature]| {
        let live: Vec<_> = fs.iter().filter(|f| f.status != "dropped").collect();
        let got: f32 = live
            .iter()
            .map(|f| {
                if f.status == "shipped" {
                    1.0
                } else if f.status == "partial" {
                    0.5
                } else {
                    0.0
                }
            })
            .sum();
        (got, live.len())
    };
    let mut by_area: BTreeMap<&str, Vec<&Feature>> = BTreeMap::new();
    for f in features {
        by_area.entry(f.area.as_str()).or_default().push(f);
    }
    println!("{:<4} {:<36} {:>8} {:>6}", "", "area", "weighted", "total");
    for (code, name) in AREAS {
        let fs = by_area.get(code).cloned().unwrap_or_default();
        let (got, n) = score(&fs);
        println!("{code:<4} {name:<36} {got:>8.1} {n:>6}");
    }
    let all: Vec<&Feature> = features.iter().collect();
    let (got, n) = score(&all);
    let pct = if n == 0 { 0.0 } else { 100.0 * got / n as f32 };
    println!("\nparity: {got:.1} of {n} features ({pct:.1} %), partial counts half");
    for tier in TIERS {
        let fs: Vec<&Feature> = features.iter().filter(|f| f.tier == *tier).collect();
        let (got, n) = score(&fs);
        println!("  {tier}: {got:.1} of {n}");
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let repo = crate::root();
    let features = load(&repo)?;
    let specs = load_spec_ids(&repo)?;
    let reference = reference_root(&repo);
    if reference.is_none() {
        eprintln!("parity: no ../mareader (or MAREADER_DIR); reference paths not checked");
    }
    let errors = validate(&features, &repo, reference.as_deref(), &specs);
    if !errors.is_empty() {
        return Err(format!(
            "{} error(s):\n  {}",
            errors.len(),
            errors.join("\n  ")
        ));
    }
    match args {
        [flag, status] if flag == "--list" => {
            for f in features.iter().filter(|f| &f.status == status) {
                println!("{:<40} {:<4} {:<3} {}", f.id, f.tier, f.milestone, f.title);
            }
        }
        _ => report(&features),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(status: &str, evidence: &[&str]) -> Feature {
        Feature {
            id: "shell.example".into(),
            title: "Example".into(),
            area: "A".into(),
            tier: "P0".into(),
            milestone: "M0".into(),
            status: status.into(),
            evidence: evidence.iter().map(|s| (*s).into()).collect(),
            command: None,
            ui_only: true,
            reference: Vec::new(),
            notes: String::new(),
        }
    }

    #[test]
    fn shipped_needs_evidence_that_exists() {
        let repo = crate::root();
        let specs = HashSet::new();
        let empty = feature("shipped", &[]);
        assert_eq!(validate(&[empty], &repo, None, &specs).len(), 1);
        let fake = feature("shipped", &["xtask/src/parity.rs::no_such_fn"]);
        assert_eq!(validate(&[fake], &repo, None, &specs).len(), 1);
        let real = feature(
            "shipped",
            &["xtask/src/parity.rs::shipped_needs_evidence_that_exists"],
        );
        assert!(validate(&[real], &repo, None, &specs).is_empty());
    }

    #[test]
    fn shipped_requires_valid_command_or_ui_only() {
        let repo = crate::root();
        let specs = load_spec_ids(&repo).expect("load specs");
        assert!(specs.contains("reader.open"));
        assert!(specs.contains("pane.split"));

        let ev = &["xtask/src/parity.rs::shipped_requires_valid_command_or_ui_only"];
        let mut missing = feature("shipped", ev);
        missing.ui_only = false;
        assert_eq!(validate(&[missing], &repo, None, &specs).len(), 1);

        let mut bogus = feature("shipped", ev);
        bogus.ui_only = false;
        bogus.command = Some("no.such-command".into());
        assert_eq!(validate(&[bogus], &repo, None, &specs).len(), 1);

        let mut both = feature("shipped", ev);
        both.command = Some("reader.open".into());
        assert_eq!(validate(&[both], &repo, None, &specs).len(), 1);

        let mut ok = feature("shipped", ev);
        ok.ui_only = false;
        ok.command = Some("reader.open".into());
        assert!(validate(&[ok], &repo, None, &specs).is_empty());
    }

    #[test]
    fn ids_and_milestones_are_checked() {
        assert!(valid_id("shell.title-bar"));
        assert!(!valid_id("Shell.title_bar"));
        assert!(valid_milestone("M8") && !valid_milestone("M9") && !valid_milestone("none"));
    }
}
