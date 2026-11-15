//! `cargo xtask z-order`: floating surfaces in `ui-egui` go through `z_order`.

use std::path::{Path, PathBuf};

/// The one module allowed to name raw `egui::Order` and `Area::new`.
const OWNER: &str = "z_order.rs";
/// `modal.rs` wraps `egui::Modal` and registers it at `Tier::Modal`.
const MODAL_OWNER: &str = "modal.rs";
/// `focus.rs` paints the keyboard ring onto the focused widget's own layer.
const LAYER_PAINTER_ALLOWED: &[&str] = &[OWNER, "focus.rs"];

const RAW_ORDERS: &[&str] = &[
    "Order::Background",
    "Order::Middle",
    "Order::Foreground",
    "Order::Tooltip",
    "Order::Debug",
];

/// Scans one Rust source string relative to `crates/ui-egui/src`.
pub fn scan_source(rel: &str, text: &str) -> Vec<String> {
    if rel == OWNER {
        return Vec::new();
    }
    let body = text.split("#[cfg(test)]").next().unwrap_or(text);
    let mut violations = Vec::new();

    for (idx, raw) in body.lines().enumerate() {
        let line = raw.split("//").next().unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }
        let n = idx + 1;
        if let Some(tok) = RAW_ORDERS.iter().find(|&&t| line.contains(t)) {
            violations.push(format!(
                "{rel}:{n}: raw `{tok}` outside `{OWNER}`; declare a `z_order::Tier`"
            ));
        }
        if has_method_call(line, ".order(") {
            violations.push(format!(
                "{rel}:{n}: raw `.order(...)` outside `{OWNER}`; use `z_order::area`"
            ));
        }
        if has_ctor_call(line, "Area::new(") {
            violations.push(format!(
                "{rel}:{n}: raw `Area::new(...)` outside `{OWNER}`; use `z_order::area`"
            ));
        }
        if has_ctor_call(line, "Window::new(") {
            violations.push(format!(
                "{rel}:{n}: raw `Window::new(...)` outside `{OWNER}`; use `z_order::area`"
            ));
        }
        if rel != MODAL_OWNER && has_ctor_call(line, "Modal::new(") {
            violations.push(format!(
                "{rel}:{n}: raw `Modal::new(...)` outside `{MODAL_OWNER}`; use `modal::Dialog`"
            ));
        }
        if has_ctor_call(line, "LayerId::new(") {
            violations.push(format!(
                "{rel}:{n}: raw `LayerId::new(...)` outside `{OWNER}`; use `z_order::layer`"
            ));
        }
        if !LAYER_PAINTER_ALLOWED.contains(&rel) && has_method_call(line, ".layer_painter(") {
            violations.push(format!(
                "{rel}:{n}: raw `.layer_painter(...)` outside `{OWNER}`; use `z_order::painter`"
            ));
        }
        if has_method_call(line, ".move_to_top(") {
            violations.push(format!(
                "{rel}:{n}: direct `.move_to_top(...)` outside `{OWNER}`; use `z_order::enforce`"
            ));
        }
    }
    violations
}

fn has_method_call(line: &str, needle: &str) -> bool {
    line.contains(needle)
}

fn has_ctor_call(line: &str, needle: &str) -> bool {
    let mut rest = line;
    while let Some(pos) = rest.find(needle) {
        let before = rest[..pos].chars().next_back();
        let boundary = before.is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
        if boundary {
            return true;
        }
        rest = &rest[pos + needle.len()..];
    }
    false
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("{}: {e}", dir.display()))?.path();
        if path.is_dir() {
            collect_rs(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

pub fn run(_: &[String]) -> Result<(), String> {
    let src = crate::root().join("crates/ui-egui/src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files)?;
    files.sort();

    let mut violations = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&src)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        violations.extend(scan_source(&rel, &text));
    }

    if !violations.is_empty() {
        for v in &violations {
            eprintln!("z-order violation: {v}");
        }
        return Err(format!(
            "{} z-order violation(s) found (see docs/ui-layers.md)",
            violations.len()
        ));
    }
    println!("z-order: {} file(s) checked, 0 violations", files.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_raw_order_area_layer_painter_and_move_to_top() {
        let src = r#"
fn draw(ctx: &egui::Context) {
    let l = egui::LayerId::new(Order::Foreground, id);
    let _p = ctx.layer_painter(l);
    egui::Area::new(id).order(Order::Middle).show(ctx, |_| {});
    ctx.move_to_top(l);
}
"#;
        let out = scan_source("page_bar.rs", src);
        assert!(out.iter().any(|m| m.contains("Order::Foreground")));
        assert!(out.iter().any(|m| m.contains("LayerId::new")));
        assert!(out.iter().any(|m| m.contains(".layer_painter")));
        assert!(out.iter().any(|m| m.contains("Area::new")));
        assert!(out.iter().any(|m| m.contains(".order(")));
        assert!(out.iter().any(|m| m.contains(".move_to_top")));
    }

    #[test]
    fn allows_z_order_owner_modal_wrapper_and_focus_ring_painter() {
        assert!(scan_source("z_order.rs", "Area::new(id).order(Order::Middle)").is_empty());
        assert!(scan_source("modal.rs", "Modal::new(self.id).show(ctx, add)").is_empty());
        assert!(scan_source("focus.rs", "ctx.layer_painter(w.layer_id)").is_empty());
    }

    #[test]
    fn rejects_raw_modal_outside_modal_rs_and_ignores_unrelated_enums() {
        let bad = scan_source(
            "settings_window.rs",
            "egui::Modal::new(id).show(ctx, |_| {});",
        );
        assert_eq!(bad.len(), 1);
        let anim = scan_source(
            "icons/anim.rs",
            "if live { Order::Again } else { Order::Start }",
        );
        assert!(anim.is_empty());
    }
}
