//! `cargo xtask panel-commands`: UI click handlers dispatch commands only.

use std::path::{Path, PathBuf};

/// Allowlist of legacy `ui-egui` adapters outside `app/` taking `&mut Session`.
pub const MUT_SESSION_ALLOWED: &[&str] = &[
    "dict/mod.rs",
    "dict/page.rs",
    "documents.rs",
    "library/runs.rs",
    "library/search.rs",
    "library/view.rs",
    "page_ai/mod.rs",
    "space.rs",
    "study_mode/mod.rs",
    "study/acting.rs",
    "study/answers.rs",
    "study/keeping.rs",
    "study/marking.rs",
    "study/mod.rs",
    "word_list/window.rs",
];

const CLICK_TRIGGERS: &[&str] = &[
    ".clicked()",
    ".double_clicked()",
    ".secondary_clicked()",
    ".changed()",
];

/// Scans one Rust source string relative to `crates/ui-egui/src`.
pub fn scan_source(rel: &str, text: &str, allowed_mut_session: &[&str]) -> Vec<String> {
    if rel == "app.rs" || rel.starts_with("app/") {
        return Vec::new();
    }
    let body = strip_cfg_test(text);
    let lines: Vec<&str> = body.lines().collect();
    let mut violations = Vec::new();
    let allow_mut = allowed_mut_session.contains(&rel);

    for (idx, &raw) in lines.iter().enumerate() {
        let line = strip_line_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if CLICK_TRIGGERS.iter().any(|t| line.contains(t)) {
            let block = collect_block(&lines, idx);
            if has_session_call(&block) {
                violations.push(format!(
                    "{rel}:{}: click handler calls `session.*(...)`; dispatch a command",
                    idx + 1
                ));
            }
        }
        if !allow_mut
            && (line.contains("&mut Session")
                || line.contains("&mut mareader_engine::session::Session")
                || line.contains("session.execute(")
                || line.contains("session.execute_json(")
                || line.contains("session.execute_id("))
        {
            violations.push(format!(
                "{rel}:{}: direct `&mut Session` or `session.execute*` outside app/ allowlist",
                idx + 1
            ));
        }
    }
    violations
}

fn strip_cfg_test(text: &str) -> &str {
    match text.find("#[cfg(test)]") {
        Some(pos) => &text[..pos],
        None => text,
    }
}

fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(pos) => &line[..pos],
        None => line,
    }
}

fn collect_block(lines: &[&str], start: usize) -> String {
    let first = strip_line_comment(lines[start]);
    let mut depth = brace_delta(first);
    let mut out = String::from(first);
    let mut idx = start + 1;
    let limit = (start + 32).min(lines.len());
    while depth > 0 && idx < limit {
        let piece = strip_line_comment(lines[idx]);
        out.push('\n');
        out.push_str(piece);
        depth += brace_delta(piece);
        idx += 1;
    }
    out
}

fn brace_delta(line: &str) -> i32 {
    let mut delta = 0_i32;
    for ch in line.chars() {
        if ch == '{' {
            delta += 1;
        } else if ch == '}' {
            delta -= 1;
        }
    }
    delta
}

fn has_session_call(block: &str) -> bool {
    let bytes = block.as_bytes();
    let needle = b"session.";
    let mut pos = 0;
    while let Some(found) = block[pos..].find("session.") {
        let abs = pos + found;
        let boundary_ok =
            abs == 0 || (!bytes[abs - 1].is_ascii_alphanumeric() && bytes[abs - 1] != b'_');
        let after = &block[abs + needle.len()..];
        if boundary_ok && starts_with_method_call(after) {
            return true;
        }
        pos = abs + needle.len();
    }
    false
}

fn starts_with_method_call(after: &str) -> bool {
    let ident_len = after
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
        .count();
    if ident_len == 0 {
        return false;
    }
    after[ident_len..].trim_start().starts_with('(')
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("read {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            walk(root, &path, out)?;
        } else if path.extension().is_some_and(|e| e == "rs") {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("read {}: {e}", path.display()))?;
            out.extend(scan_source(&rel, &text, MUT_SESSION_ALLOWED));
        }
    }
    Ok(())
}

pub fn run(_: &[String]) -> Result<(), String> {
    let root = crate::root().join("crates/ui-egui/src");
    let mut violations = Vec::new();
    walk(&root, &root, &mut violations)?;
    println!(
        "panel-commands: {} violation(s) ({} allowlisted legacy adapters)",
        violations.len(),
        MUT_SESSION_ALLOWED.len()
    );
    if violations.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} panel-commands violation(s):\n  {}",
        violations.len(),
        violations.join("\n  ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_direct_session_mutation_in_click_handler() {
        let bad = r#"
fn draw(ui: &mut Ui, session: &Session) {
    if ui.button("Jump").clicked() {
        session.execute(Command::FirstPage, 0.0);
    }
}
"#;
        let errs = scan_source("sidebar.rs", bad, MUT_SESSION_ALLOWED);
        assert_eq!(errs.len(), 2, "{errs:?}");
    }

    #[test]
    fn allows_command_dispatch_in_click_handler() {
        let good = r#"
fn draw(ui: &mut Ui, out: &mut Commands) {
    if ui.button("Jump").clicked() {
        out.push(Sent::Engine(Command::FirstPage));
    }
}
"#;
        let errs = scan_source("sidebar.rs", good, MUT_SESSION_ALLOWED);
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn workspace_ui_egui_passes_panel_commands_gate() {
        let root = crate::root().join("crates/ui-egui/src");
        let mut violations = Vec::new();
        walk(&root, &root, &mut violations).expect("walk crates/ui-egui/src");
        assert!(violations.is_empty(), "{violations:?}");
    }
}
