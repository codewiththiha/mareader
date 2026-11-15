//! Optional `--root` directory confinement for MCP file read and write paths.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// Confines file read/write paths under a canonical root directory when set.
#[derive(Clone, Debug, Default)]
pub struct RootJail {
    root: Option<PathBuf>,
}

impl RootJail {
    /// Canonicalizes an optional `--root` directory.
    pub fn new(root: Option<PathBuf>) -> Result<Self, String> {
        let root = match root {
            None => None,
            Some(dir) => Some(
                std::fs::canonicalize(&dir)
                    .map_err(|e| format!("invalid --root {}: {e}", dir.display()))?,
            ),
        };
        Ok(Self { root })
    }

    /// Checks standard path-bearing command parameters against `--root`.
    pub fn check_command_paths(&self, id: &str, params: &Value) -> Result<(), String> {
        if self.root.is_none() {
            return Ok(());
        }
        let Some(obj) = params.as_object() else {
            return Ok(());
        };
        for key in ["path", "folder", "dir", "file", "root"] {
            if let Some(raw) = obj.get(key).and_then(Value::as_str) {
                self.check_read(Path::new(raw))
                    .map_err(|why| format!("{id}: {why}"))?;
            }
        }
        Ok(())
    }

    /// Confirms an existing readable path stays inside `--root`.
    pub fn check_read(&self, path: &Path) -> Result<PathBuf, String> {
        let Some(root) = self.root.as_deref() else {
            return Ok(path.to_path_buf());
        };
        let resolved = std::fs::canonicalize(path)
            .map_err(|e| format!("cannot resolve path {}: {e}", path.display()))?;
        if !resolved.starts_with(root) {
            return Err(format!(
                "path {} escapes --root {}",
                resolved.display(),
                root.display()
            ));
        }
        Ok(resolved)
    }

    /// Confirms an output path's parent directory stays inside `--root`.
    pub fn check_write(&self, path: &Path) -> Result<PathBuf, String> {
        let Some(root) = self.root.as_deref() else {
            return Ok(path.to_path_buf());
        };
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let canon_parent = std::fs::canonicalize(parent)
            .map_err(|e| format!("cannot resolve parent {}: {e}", parent.display()))?;
        if !canon_parent.starts_with(root) {
            return Err(format!(
                "output path {} escapes --root {}",
                path.display(),
                root.display()
            ));
        }
        Ok(canon_parent)
    }
}
