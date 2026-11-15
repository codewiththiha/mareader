//! `cargo xtask ui-commands [--check]`: syncs `UI_COMMANDS` into `crates/mcp`.

use std::path::PathBuf;

const SOURCE_REL: &str = "crates/ui-egui/src/ui_command.rs";
const TARGET_REL: &str = "crates/mcp/src/ui_catalog.rs";
const OPEN: char = '\u{7b}';
const CLOSE: char = '\u{7d}';

/// One parsed `UiCommandMeta` entry from `crates/ui-egui/src/ui_command.rs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    pub title: String,
    pub label: String,
    pub menu: Option<String>,
    pub shortcut: Option<String>,
}

/// Parses the `UI_COMMANDS` array from `ui_command.rs`.
pub fn parse_source(text: &str) -> Result<Vec<Entry>, String> {
    let marker = "pub const UI_COMMANDS: &[UiCommandMeta] = &[";
    let start = text
        .find(marker)
        .ok_or_else(|| format!("missing `{marker}` in {SOURCE_REL}"))?;
    let body = &text[start + marker.len()..];
    let end = body
        .find("];")
        .ok_or_else(|| format!("unterminated `UI_COMMANDS` in {SOURCE_REL}"))?;
    let section = &body[..end];

    let head = format!("UiCommandMeta {OPEN}");
    let mut entries = Vec::new();
    let mut rest = section;
    while let Some(open) = rest.find(&head) {
        let after = &rest[open + head.len()..];
        let close = after
            .find(CLOSE)
            .ok_or_else(|| "unterminated `UiCommandMeta` block".to_owned())?;
        entries.push(parse_entry(&after[..close])?);
        rest = &after[close + 1..];
    }
    if entries.is_empty() {
        return Err(format!("found zero UI_COMMANDS entries in {SOURCE_REL}"));
    }
    Ok(entries)
}

fn parse_entry(block: &str) -> Result<Entry, String> {
    let id = required_str(block, "id")?;
    let title = required_str(block, "title")?;
    let label = required_str(block, "label")?;
    let menu = optional_str(block, "menu")?;
    let shortcut = optional_str(block, "shortcut")?;
    Ok(Entry {
        id,
        title,
        label,
        menu,
        shortcut,
    })
}

fn field_value<'a>(block: &'a str, name: &str) -> Result<&'a str, String> {
    let prefix = format!("{name}:");
    for line in block.lines() {
        let trimmed = line.trim();
        if let Some(after) = trimmed.strip_prefix(&prefix) {
            return Ok(after.trim().trim_end_matches(','));
        }
    }
    Err(format!("missing field `{name}` in UiCommandMeta"))
}

fn required_str(block: &str, name: &str) -> Result<String, String> {
    let raw = field_value(block, name)?;
    unquote(raw).ok_or_else(|| format!("field `{name}` expected quoted string, got {raw:?}"))
}

fn optional_str(block: &str, name: &str) -> Result<Option<String>, String> {
    let raw = field_value(block, name)?;
    if raw == "None" {
        return Ok(None);
    }
    let inner = raw
        .strip_prefix("Some(")
        .and_then(|s| s.strip_suffix(')'))
        .ok_or_else(|| format!("field `{name}` expected None or Some(\"...\"), got {raw:?}"))?;
    let val = unquote(inner.trim())
        .ok_or_else(|| format!("field `{name}` expected quoted string inside Some(...)"))?;
    Ok(Some(val))
}

fn unquote(raw: &str) -> Option<String> {
    let inner = raw.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.replace("\\\"", "\"").replace("\\\\", "\\"))
}

fn quote(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Renders the exact Rust source for `crates/mcp/src/ui_catalog.rs`.
pub fn render_catalog(entries: &[Entry]) -> String {
    let mut out = String::new();
    out.push_str("//! Generated mirror of `crates/ui-egui/src/ui_command.rs::UI_COMMANDS`.\n");
    out.push_str("//!\n");
    out.push_str("//! Do not edit by hand; regenerate with `cargo xtask ui-commands`.\n\n");
    out.push_str("/// Static metadata for a UI-only command, mirrored without linking `egui`.\n");
    out.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
    out.push_str(&format!("pub struct UiCommandInfo {OPEN}\n"));
    out.push_str("    pub id: &'static str,\n");
    out.push_str("    pub title: &'static str,\n");
    out.push_str("    pub label: &'static str,\n");
    out.push_str("    pub menu: Option<&'static str>,\n");
    out.push_str("    pub shortcut: Option<&'static str>,\n");
    out.push_str(&format!("{CLOSE}\n\n"));
    out.push_str("/// UI-only commands exposed by `mareader-ui-egui`.\n");
    out.push_str("pub const UI_COMMANDS: &[UiCommandInfo] = &[\n");
    for entry in entries {
        out.push_str(&format!("    UiCommandInfo {OPEN}\n"));
        out.push_str(&format!("        id: {},\n", quote(&entry.id)));
        out.push_str(&format!("        title: {},\n", quote(&entry.title)));
        out.push_str(&format!("        label: {},\n", quote(&entry.label)));
        match &entry.menu {
            Some(m) => out.push_str(&format!("        menu: Some({}),\n", quote(m))),
            None => out.push_str("        menu: None,\n"),
        }
        match &entry.shortcut {
            Some(s) => out.push_str(&format!("        shortcut: Some({}),\n", quote(s))),
            None => out.push_str("        shortcut: None,\n"),
        }
        out.push_str(&format!("    {CLOSE},\n"));
    }
    out.push_str("];\n");
    out
}

fn paths() -> (PathBuf, PathBuf) {
    let root = crate::root();
    (root.join(SOURCE_REL), root.join(TARGET_REL))
}

pub fn run(args: &[String]) -> Result<(), String> {
    let check_only = match args {
        [] => false,
        [flag] if flag == "--check" => true,
        _ => return Err("usage: cargo xtask ui-commands [--check]".to_owned()),
    };
    let (source_path, target_path) = paths();
    let source = std::fs::read_to_string(&source_path)
        .map_err(|e| format!("read {}: {e}", source_path.display()))?;
    let entries = parse_source(&source)?;
    let expected = render_catalog(&entries);
    let current = std::fs::read_to_string(&target_path).unwrap_or_default();

    if current == expected {
        println!("ui-commands: {} command(s) in sync", entries.len());
        return Ok(());
    }
    if check_only {
        return Err(format!(
            "{TARGET_REL} is out of sync with {SOURCE_REL}; run `cargo xtask ui-commands`"
        ));
    }
    std::fs::write(&target_path, expected)
        .map_err(|e| format!("write {}: {e}", target_path.display()))?;
    println!(
        "ui-commands: wrote {} command(s) to {TARGET_REL}",
        entries.len()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_ui_catalog_matches_ui_commands_source() {
        let (source_path, target_path) = paths();
        let source = std::fs::read_to_string(&source_path).expect("read ui_command.rs");
        let entries = parse_source(&source).expect("parse UI_COMMANDS");
        assert!(!entries.is_empty());
        let expected = render_catalog(&entries);
        let actual = std::fs::read_to_string(&target_path).expect("read ui_catalog.rs");
        assert_eq!(actual, expected, "run `cargo xtask ui-commands`");
    }
}
