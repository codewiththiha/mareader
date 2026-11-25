//! `cargo xtask assets`: the asset policy in AGENTS.md section Assets.

use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const LICENSES: &[&str] = &[
    "MIT",
    "ISC",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "OFL-1.1",
    "CC0-1.0",
    "CC-BY-4.0",
];

#[derive(Debug, Deserialize)]
struct Doc {
    #[serde(rename = "asset", default)]
    assets: Vec<Asset>,
    #[serde(rename = "bundled", default)]
    bundled: Vec<Bundled>,
}

/// A file committed under `assets/`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    path: String,
    title: String,
    author: String,
    source: String,
    license: String,
    license_file: String,
    sha256: String,
    usage: String,
}

/// Art that arrives inside a Cargo dependency rather than as a file here.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundled {
    name: String,
    via: String,
    author: String,
    source: String,
    license: String,
    usage: String,
}

fn sha256(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

fn files_under(dir: &Path, out: &mut Vec<String>, root: &Path) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let p = entry.path();
        if p.is_dir() {
            files_under(&p, out, root);
        } else if let Ok(rel) = p.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

pub fn run(_: &[String]) -> Result<(), String> {
    let root = crate::root();
    let text = std::fs::read_to_string(root.join("ATTRIBUTION.toml"))
        .map_err(|e| format!("ATTRIBUTION.toml: {e}"))?;
    let doc: Doc = toml::from_str(&text).map_err(|e| format!("ATTRIBUTION.toml: {e}"))?;
    let mut errors = Vec::new();
    let mut files = Vec::new();
    files_under(&root.join("assets"), &mut files, &root);
    let licence_files: Vec<&str> = doc.assets.iter().map(|a| a.license_file.as_str()).collect();
    for f in files
        .iter()
        .filter(|f| !licence_files.contains(&f.as_str()))
    {
        if !doc.assets.iter().any(|a| &a.path == f) {
            errors.push(format!("{f}: no [[asset]] entry"));
        }
    }
    for a in &doc.assets {
        let blank = [&a.title, &a.author, &a.source, &a.usage]
            .iter()
            .any(|s| s.trim().is_empty());
        if blank {
            errors.push(format!(
                "{}: title, author, source and usage are required",
                a.path
            ));
        }
        if !LICENSES.contains(&a.license.as_str()) {
            errors.push(format!(
                "{}: licence {} is not on the allowlist",
                a.path, a.license
            ));
        }
        if !root.join(&a.license_file).is_file() {
            errors.push(format!(
                "{}: licence file {} is missing",
                a.path, a.license_file
            ));
        }
        match sha256(&root.join(&a.path)) {
            Ok(h) if h == a.sha256 => {}
            Ok(h) => errors.push(format!(
                "{}: sha256 is {h}, entry says {}",
                a.path, a.sha256
            )),
            Err(e) => errors.push(e),
        }
    }
    for b in &doc.bundled {
        let ok = b
            .license
            .split(" AND ")
            .all(|part| LICENSES.contains(&part));
        let blank = [&b.via, &b.author, &b.source, &b.usage]
            .iter()
            .any(|s| s.trim().is_empty());
        if !ok || blank {
            errors.push(format!(
                "{}: needs via, author, source, usage and allowed licences",
                b.name
            ));
        }
    }
    if errors.is_empty() {
        println!(
            "assets: {} files, {} bundled sets ok",
            doc.assets.len(),
            doc.bundled.len()
        );
        Ok(())
    } else {
        Err(format!(
            "{} problem(s):\n  {}",
            errors.len(),
            errors.join("\n  ")
        ))
    }
}
