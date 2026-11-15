//! `cargo xtask layers`: the dependency rules of `docs/architecture.md`.

use std::process::Command;

use serde_json::Value;

/// Every crate, planned ones included, by name without `mareader-`.
pub const TABLE: &[(&str, Layer)] = &[
    ("appearance", Layer::Core(0)),
    ("animated-icons", Layer::Core(0)),
    ("workspace", Layer::Core(0)),
    ("fetch", Layer::Core(0)),
    ("search", Layer::Core(0)),
    ("audio", Layer::Core(0)),
    ("memory", Layer::Core(0)),
    ("script", Layer::Core(0)),
    ("pdf", Layer::Core(1)),
    ("nn-core", Layer::Core(0)),
    ("kokoro", Layer::Core(0)),
    ("g2p", Layer::Core(0)),
    ("ipa", Layer::Core(0)),
    ("hanzi", Layer::Core(0)),
    ("vits", Layer::Core(0)),
    ("phonon", Layer::Core(0)),
    ("speech", Layer::Core(1)),
    ("reflow", Layer::Core(1)),
    ("library", Layer::Core(1)),
    ("study", Layer::Core(1)),
    ("dict", Layer::Core(1)),
    ("cefr", Layer::Core(1)),
    ("epub", Layer::Core(2)),
    ("settings", Layer::Core(2)),
    ("session", Layer::Core(3)),
    ("assist", Layer::Core(3)),
    ("engine", Layer::Core(5)),
    ("ui-egui", Layer::Front),
    ("mareader", Layer::App),
    ("web", Layer::App),
    ("cli", Layer::App),
    ("xtask", Layer::Exempt),
];

/// Same-layer edges that are allowed; any other must point down.
pub const SIDEWAYS: &[(&str, &str)] = &[
    ("kokoro", "nn-core"),
    ("phonon", "nn-core"),
    ("vits", "nn-core"),
];

/// Toolkit crates, allowed only in `Layer::Front` and exempt crates.
pub const UI_CRATES: &[&str] = &[
    "egui",
    "eframe",
    "egui_extras",
    "egui_kittest",
    "winit",
    "rfd",
];
/// Prefix families with the same rule.
pub const UI_PREFIXES: &[&str] = &["wgpu", "egui"];

/// The one app that must never link a toolkit: it is the proof.
pub const HEADLESS: &str = "cli";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Toolkit-free; may depend only on lower layers.
    Core(u8),
    /// One front end, in egui; the only layer that may use a toolkit.
    Front,
    /// A binary: it may use a toolkit, except the headless client.
    App,
    /// Tooling that builds and checks the tree.
    Exempt,
}

impl Layer {
    pub fn rank(self) -> u8 {
        match self {
            Layer::Core(n) => n,
            Layer::Front => 6,
            Layer::App => 7,
            Layer::Exempt => u8::MAX,
        }
    }
}

pub fn short(name: &str) -> &str {
    name.strip_prefix("mareader-").unwrap_or(name)
}

fn layer_of(name: &str) -> Option<Layer> {
    TABLE
        .iter()
        .find(|(n, _)| *n == short(name))
        .map(|(_, l)| *l)
}

fn is_ui(dep: &str) -> bool {
    UI_CRATES.contains(&dep) || UI_PREFIXES.iter().any(|p| dep.starts_with(p))
}

/// One package and the names of its direct dependencies, dev ones too.
pub struct Package {
    pub name: String,
    pub deps: Vec<String>,
}

/// Every rule violation in `packages`, as readable lines.
pub fn check(packages: &[Package]) -> Vec<String> {
    let members: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
    let mut errors = Vec::new();
    for p in packages {
        let Some(layer) = layer_of(&p.name) else {
            errors.push(format!(
                "{}: not in xtask/src/layers.rs TABLE; give it a layer",
                p.name
            ));
            continue;
        };
        if layer == Layer::Exempt {
            continue;
        }
        let headless = layer == Layer::App && short(&p.name) == HEADLESS;
        for dep in &p.deps {
            if headless && (is_ui(dep) || short(dep) == "ui-egui") {
                errors.push(format!(
                    "{} is the headless client and must not depend on {dep}",
                    p.name
                ));
                continue;
            }
            // Ours either way, but a bare `cefr` is the git crate of that name.
            let ours = members.contains(&dep.as_str()) || dep.starts_with("mareader-");
            if let Some(theirs) = layer_of(dep).filter(|_| ours) {
                let sideways = SIDEWAYS.contains(&(short(&p.name), short(dep)));
                let down =
                    theirs.rank() < layer.rank() || (theirs.rank() == layer.rank() && sideways);
                if !down {
                    errors.push(format!(
                        "{} (L{}) must not depend on {} (L{})",
                        p.name,
                        layer.rank(),
                        dep,
                        theirs.rank()
                    ));
                }
            } else if is_ui(dep) && !matches!(layer, Layer::Front | Layer::App) {
                errors.push(format!(
                    "{} is below the UI layer and must not depend on {dep}",
                    p.name
                ));
            }
        }
    }
    errors
}

fn metadata() -> Result<Vec<Package>, String> {
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(crate::root())
        .output()
        .map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let pkgs = v["packages"]
        .as_array()
        .ok_or("cargo metadata: no packages")?;
    Ok(pkgs
        .iter()
        .map(|p| Package {
            name: p["name"].as_str().unwrap_or_default().to_owned(),
            deps: p["dependencies"]
                .as_array()
                .map(|d| {
                    d.iter()
                        .map(|d| d["name"].as_str().unwrap_or_default().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect())
}

/// Checks transitive `cargo tree` output for the headless client.
pub fn check_headless_tree(tree: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut errors = Vec::new();
    for line in tree.lines() {
        let Some(name) = line.split_whitespace().next() else {
            continue;
        };
        if (is_ui(name) || short(name) == "ui-egui") && seen.insert(name) {
            errors.push(format!(
                "mareader-{HEADLESS} transitively links toolkit crate {name}"
            ));
        }
    }
    errors
}

fn headless_tree(packages: &[Package]) -> Result<Vec<String>, String> {
    let Some(cli) = packages.iter().find(|p| short(&p.name) == HEADLESS) else {
        return Ok(Vec::new());
    };
    let out = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args([
            "tree",
            "-q",
            "--locked",
            "--offline",
            "-p",
            &cli.name,
            "-e",
            "normal,build",
            "--prefix",
            "none",
        ])
        .current_dir(crate::root())
        .output()
        .map_err(|e| format!("cargo tree: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    Ok(check_headless_tree(&String::from_utf8_lossy(&out.stdout)))
}

pub fn run(_: &[String]) -> Result<(), String> {
    let packages = metadata()?;
    let mut errors = check(&packages);
    errors.extend(headless_tree(&packages)?);
    if errors.is_empty() {
        println!("layers: {} crates ok", packages.len());
        Ok(())
    } else {
        Err(format!(
            "{} violation(s):\n  {}",
            errors.len(),
            errors.join("\n  ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, deps: &[&str]) -> Package {
        Package {
            name: name.into(),
            deps: deps.iter().map(|d| (*d).into()).collect(),
        }
    }

    #[test]
    fn egui_below_the_front_end_is_refused() {
        let errs = check(&[pkg("mareader-pdf", &["egui"])]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(check(&[pkg("mareader-ui-egui", &["egui", "wgpu"])]).is_empty());
    }

    #[test]
    fn edges_point_down_and_unknown_crates_need_a_layer() {
        let all = [
            pkg("mareader-appearance", &["mareader-ui-egui"]),
            pkg("mareader-ui-egui", &["mareader-appearance"]),
            pkg("mareader-mystery", &[]),
        ];
        let errs = check(&all);
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("mareader-appearance (L0)")));
        assert!(errs.iter().any(|e| e.contains("mareader-mystery")));
    }

    #[test]
    fn the_engine_sits_under_the_front_end_and_above_the_core() {
        let ok = [
            pkg("mareader-engine", &["mareader-session"]),
            pkg("mareader-ui-egui", &["mareader-engine"]),
            pkg("mareader", &["mareader-ui-egui"]),
        ];
        assert!(check(&ok).is_empty(), "{:?}", check(&ok));
        let up = check(&[pkg("mareader-engine", &["mareader-ui-egui"])]);
        assert_eq!(up.len(), 1, "{up:?}");
        assert!(up[0].contains("(L5)"), "{up:?}");
        let pixels = check(&[pkg("mareader-engine", &["egui"])]);
        assert_eq!(pixels.len(), 1, "{pixels:?}");
    }

    #[test]
    fn a_toolkit_smuggled_in_through_a_dev_dependency_is_refused() {
        // The metadata list includes dev-dependencies, so they are checked too.
        let dev = ["egui_kittest", "wgpu-core"];
        let errs = check(&[pkg("mareader-engine", &dev)]);
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(check(&[pkg("mareader-ui-egui", &dev)]).is_empty());
    }

    #[test]
    fn the_headless_client_may_not_link_a_toolkit() {
        let apps = [
            pkg("mareader-cli", &["mareader-engine"]),
            pkg("mareader-cli", &["mareader-ui-egui"]),
            pkg("mareader-cli", &["eframe"]),
            pkg("mareader", &["mareader-ui-egui", "eframe"]),
        ];
        let errs = check(&apps);
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(
            errs.iter().all(|e| e.contains("headless client")),
            "{errs:?}"
        );
        assert!(check(&[pkg("mareader-cli", &["mareader-engine"])]).is_empty());
    }

    #[test]
    fn the_layer_ranks_leave_the_engine_floor() {
        assert_eq!(Layer::Core(5).rank(), 5);
        assert_eq!(Layer::Front.rank(), 6);
        assert_eq!(Layer::App.rank(), 7);
        assert!(Layer::Exempt.rank() > Layer::App.rank());
    }

    #[test]
    fn headless_tree_rejects_transitive_toolkit_crates() {
        let clean = "mareader-cli v0.0.5\nmareader-engine v0.0.5\nserde_json v1.0.0\n";
        assert!(check_headless_tree(clean).is_empty());
        let bad = "mareader-cli v0.0.5\nwgpu-core v24.0.0\negui v0.31.0\negui v0.31.0\n";
        let errs = check_headless_tree(bad);
        assert_eq!(errs.len(), 2, "{errs:?}");
    }

    #[test]
    fn every_row_names_a_crate_the_workspace_has() {
        // Rows for planned crates must not outlive them by much: the tree wins.
        let have = std::fs::read_dir(crate::root().join("crates"))
            .map(|dirs| {
                dirs.filter_map(|d| d.ok())
                    .map(|d| d.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (name, _) in TABLE {
            if matches!(*name, "mareader" | "web" | "cli" | "xtask") {
                continue;
            }
            assert!(have.iter().any(|d| d == name), "{name} has no crate");
        }
    }
}
