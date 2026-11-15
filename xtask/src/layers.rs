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
    ("ui", Layer::Front),
    ("mareader", Layer::Exempt),
    ("web", Layer::Exempt),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// Toolkit-free; may depend only on lower layers.
    Core(u8),
    /// The egui front end; the only layer that may use a toolkit.
    Front,
    /// Binaries and tooling.
    Exempt,
}

impl Layer {
    fn rank(self) -> u8 {
        match self {
            Layer::Core(n) => n,
            Layer::Front => 7,
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
        for dep in &p.deps {
            if members.contains(&dep.as_str()) {
                let Some(theirs) = layer_of(dep) else {
                    continue;
                };
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
            } else if is_ui(dep) && layer != Layer::Front {
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

pub fn run(_: &[String]) -> Result<(), String> {
    let packages = metadata()?;
    let errors = check(&packages);
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
        assert!(check(&[pkg("mareader-ui", &["egui", "wgpu"])]).is_empty());
    }

    #[test]
    fn edges_point_down_and_unknown_crates_need_a_layer() {
        let all = [
            pkg("mareader-appearance", &["mareader-ui"]),
            pkg("mareader-ui", &["mareader-appearance"]),
            pkg("mareader-mystery", &[]),
        ];
        let errs = check(&all);
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(errs.iter().any(|e| e.contains("mareader-appearance (L0)")));
        assert!(errs.iter().any(|e| e.contains("mareader-mystery")));
    }
}
