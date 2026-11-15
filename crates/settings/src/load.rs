//! Reading stored settings: migrate the JSON, then parse each section alone.

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{AiSettings, Provider, SCHEMA_VERSION, Settings};

/// Loaded settings plus what had to be repaired, for the log.
#[derive(Debug, Default)]
pub struct Loaded {
    pub settings: Settings,
    pub notes: Vec<String>,
    /// Top-level keys nothing read, as a newer build's.
    pub unknown: Vec<String>,
}

/// Never fails: anything unreadable falls back to defaults, section by section.
pub fn load(json: &str) -> Loaded {
    let mut notes = Vec::new();
    let mut root = match serde_json::from_str::<Value>(json) {
        Ok(Value::Object(root)) => root,
        Ok(_) | Err(_) => {
            notes.push("settings unreadable; using defaults".to_owned());
            return Loaded {
                settings: Settings::default(),
                notes,
                unknown: Vec::new(),
            };
        }
    };
    migrate(&mut root, &mut notes);
    let (settings, unknown) = parse_sections(root, &mut notes);
    Loaded {
        settings,
        notes,
        unknown,
    }
}

/// Compact JSON in the current schema.
pub fn to_json(settings: &Settings) -> String {
    serde_json::to_string(settings).unwrap_or_default()
}

fn migrate(root: &mut Map<String, Value>, notes: &mut Vec<String>) {
    let stored = root.get("version").and_then(Value::as_u64).unwrap_or(0);
    if stored > u64::from(SCHEMA_VERSION) {
        notes.push(format!(
            "settings v{stored} are newer; unknown fields dropped"
        ));
    }
    if stored < 1 {
        v0_to_v1(root);
    }
    if stored < 2 {
        v1_to_v2(root);
    }
    if stored < 3 {
        v2_to_v3(root);
    }
    if stored < 4 {
        v3_to_v4(root);
    }
    if stored < 5 {
        v4_to_v5(root);
    }
    if stored < 6 {
        v5_to_v6(root);
    }
    root.insert("version".to_owned(), json!(SCHEMA_VERSION));
}

/// v6 added Read aloud to the selection menu, after Look up if shown.
fn v5_to_v6(root: &mut Map<String, Value>) {
    let bar = root
        .get_mut("study")
        .and_then(|s| s.get_mut("highlights"))
        .and_then(|h| h.get_mut("bar"))
        .and_then(Value::as_array_mut);
    let Some(bar) = bar else {
        return;
    };
    let item = json!("read_aloud");
    if bar.contains(&item) {
        return;
    }
    let near = |name: &str| bar.iter().position(|v| v == name);
    let at = near("look_up")
        .or_else(|| near("explain"))
        .or_else(|| near("copy"))
        .map_or(0, |i| i + 1);
    bar.insert(at, item);
}

/// v5 made the one chosen provider a named model that Explain uses.
fn v4_to_v5(root: &mut Map<String, Value>) {
    let Some(Value::Object(ai)) = root.get_mut("ai") else {
        return;
    };
    let chosen = ai.remove("provider").unwrap_or_else(|| json!("auto"));
    let mut moved = AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    };
    let id = match serde_json::from_value::<Provider>(chosen).ok() {
        // Automatic meant Apple Intelligence where a Mac has it.
        None => cfg!(target_os = "macos").then(|| moved.add_preset(Provider::Apple, "")),
        Some(p) => {
            let model = (ai.get(p.scope()))
                .and_then(|e| e.get("model"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            Some(moved.add_preset(p, model))
        }
    };
    moved.tasks.explain.preset = id;
    let presets = serde_json::to_value(&moved.presets).unwrap_or_else(|_| json!([]));
    let tasks = serde_json::to_value(&moved.tasks).unwrap_or_else(|_| json!({}));
    ai.insert("presets".to_owned(), presets);
    ai.insert("tasks".to_owned(), tasks);
}

/// v4 added Note to the selection menu, after Look up if shown.
fn v3_to_v4(root: &mut Map<String, Value>) {
    let bar = root
        .get_mut("study")
        .and_then(|s| s.get_mut("highlights"))
        .and_then(|h| h.get_mut("bar"))
        .and_then(Value::as_array_mut);
    let Some(bar) = bar else {
        return;
    };
    let item = json!("note");
    if bar.contains(&item) {
        return;
    }
    let near = |name: &str| bar.iter().position(|v| v == name);
    let at = near("look_up")
        .or_else(|| near("explain"))
        .or_else(|| near("copy"))
        .map_or(0, |i| i + 1);
    bar.insert(at, item);
}

/// v3 added Look up to the selection menu, after Explain if shown.
fn v2_to_v3(root: &mut Map<String, Value>) {
    let bar = root
        .get_mut("study")
        .and_then(|s| s.get_mut("highlights"))
        .and_then(|h| h.get_mut("bar"))
        .and_then(Value::as_array_mut);
    let Some(bar) = bar else {
        return;
    };
    let item = json!("look_up");
    if bar.contains(&item) {
        return;
    }
    let near = |name: &str| bar.iter().position(|v| v == name);
    let at = near("explain")
        .or_else(|| near("copy"))
        .map_or(0, |i| i + 1);
    bar.insert(at, item);
}

/// v2 moved Column Width to the text, so each pane may have its own.
fn v1_to_v2(root: &mut Map<String, Value>) {
    let layout = root.get_mut("layout").and_then(Value::as_object_mut);
    if let Some(v) = layout.and_then(|l| l.remove("column_width_pct")) {
        put(root, "text", "column_width_pct", v);
    }
}

/// v0 is the flat blob the shell saved before this crate existed.
fn v0_to_v1(root: &mut Map<String, Value>) {
    let moves = [
        ("titlebar_pinned", "chrome", "titlebar_pinned"),
        ("sidebar_overlay", "chrome", "sidebar_overlay"),
        ("animations", "motion", "enabled"),
        ("sidebar_slide", "motion", "sidebar_slide"),
    ];
    for (from, section, field) in moves {
        if let Some(v) = root.remove(from) {
            put(root, section, field, v);
        }
    }
    let Some(Value::Object(mut look)) = root.remove("workspace") else {
        return;
    };
    if let Some(Value::String(c)) = look.get("outline_color") {
        let c = c.to_lowercase();
        look.insert("outline_color".to_owned(), json!(c));
    }
    if let Some(Value::Array(rgb)) = look.remove("outline_custom") {
        let byte = |i: usize| rgb.get(i).and_then(Value::as_u64).unwrap_or(0).min(255);
        let hex = format!("#{:02x}{:02x}{:02x}", byte(0), byte(1), byte(2));
        look.insert("outline_custom".to_owned(), json!(hex));
    }
    if let Some(Value::Bool(rounded)) = look.remove("rounded") {
        let corners = if rounded { "rounded" } else { "square" };
        look.insert("corners".to_owned(), json!(corners));
    }
    put(root, "workspace", "look", Value::Object(look));
}

/// Sets `root[section][field]`, creating the section when missing.
fn put(root: &mut Map<String, Value>, section: &str, field: &str, v: Value) {
    let entry = root.entry(section).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    if let Value::Object(map) = entry {
        map.insert(field.to_owned(), v);
    }
}

/// One bad section resets only itself, never its neighbours.
fn parse_sections(
    mut root: Map<String, Value>,
    notes: &mut Vec<String>,
) -> (Settings, Vec<String>) {
    let mut s = Settings::default();
    macro_rules! sections {
        ($($field:ident),+) => {$(
            if let Some(v) = root.remove(stringify!($field)) {
                section(&mut s.$field, v, stringify!($field), notes);
            }
        )+};
    }
    sections!(
        appearance,
        user_presets,
        chrome,
        layout,
        paper,
        motion,
        text,
        study,
        dictionary,
        ai,
        workspace,
        library,
        read_aloud
    );
    // What's left was never read: a section missing above resets each launch.
    root.remove("version");
    s.sanitize();
    (s, root.into_iter().map(|(k, _)| k).collect())
}

fn section<T: DeserializeOwned>(slot: &mut T, v: Value, key: &str, notes: &mut Vec<String>) {
    match serde_json::from_value(v) {
        Ok(parsed) => *slot = parsed,
        Err(e) => notes.push(format!("settings.{key} reset to defaults: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Corners, Swatch};

    #[test]
    fn every_saved_section_is_read_back() {
        let mut saved = Settings::default();
        let id = saved.ai.add_preset(Provider::Ollama, "llama3.2:latest");
        saved.ai.tasks.explain.preset = Some(id);
        saved.ai.tasks.explain.language = Some("my".to_owned());
        saved.motion.icon_animations = false;
        let got = load(&to_json(&saved));
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        assert!(got.unknown.is_empty(), "never read: {:?}", got.unknown);
        assert_eq!(got.settings.ai, saved.ai);
        assert_eq!(got.settings.motion, saved.motion);
    }

    #[test]
    fn v4_s_chosen_provider_becomes_the_model_explain_uses() {
        let v4 = r#"{"version":4,"ai":{"provider":"ollama",
            "ollama":{"url":"http://box:11434/v1","model":"qwen3:4b"},
            "openai":{"model":"gpt-x"}}}"#;
        let got = load(v4);
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        let ai = got.settings.ai;
        let p = ai.explain_preset().cloned().unwrap_or_default();
        assert_eq!(
            (p.provider, p.model.as_str()),
            (Provider::Ollama, "qwen3:4b")
        );
        assert_eq!(p.name, "Ollama · qwen3:4b");
        assert_eq!(ai.presets.len(), 1, "only the chosen one moves");
        assert_eq!(ai.url(Provider::Ollama), "http://box:11434/v1");
        // A blank model keeps meaning the provider's default.
        let v4 = r#"{"version":4,"ai":{"provider":"openai"}}"#;
        let ai = load(v4).settings.ai;
        let model = ai.explain_preset().map(|p| p.model().to_owned());
        assert_eq!(model.as_deref(), Some("gpt-4.1-mini"));
    }

    #[test]
    fn v4_s_automatic_is_apple_on_a_mac_and_nothing_elsewhere() {
        let ai = load(r#"{"version":4,"ai":{"provider":"auto"}}"#)
            .settings
            .ai;
        let apple = ai.explain_preset().map(|p| p.provider);
        match cfg!(target_os = "macos") {
            true => assert_eq!(apple, Some(Provider::Apple)),
            false => assert!(apple.is_none() && ai.presets.is_empty()),
        }
    }

    #[test]
    fn an_empty_object_is_all_defaults() {
        let got = load("{}");
        assert_eq!(got.settings, Settings::default());
        assert!(got.notes.is_empty());
    }

    #[test]
    fn garbage_falls_back_with_a_note() {
        let got = load("not json");
        assert_eq!(got.settings, Settings::default());
        assert_eq!(got.notes.len(), 1);
    }

    #[test]
    fn a_bad_section_resets_only_itself() {
        let json = r#"{"version":1,"motion":{"zoom":"yes"},"chrome":{"titlebar_pinned":true}}"#;
        let got = load(json);
        assert!(
            got.settings.chrome.titlebar_pinned,
            "the good section survives"
        );
        assert_eq!(got.settings.motion, Default::default());
        assert!(got.notes.iter().any(|n| n.contains("settings.motion")));
    }

    #[test]
    fn the_v0_shell_blob_migrates() {
        let v0 = r#"{
            "appearance": {"base": "dim"},
            "titlebar_pinned": true,
            "animations": false,
            "sidebar_overlay": true,
            "sidebar_slide": false,
            "workspace": {"gap": 6.0, "outline_width": 3.0, "outline_color": "Custom",
                          "outline_custom": [255, 0, 16], "rounded": true, "shadow": true}
        }"#;
        let got = load(v0);
        let s = got.settings;
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        assert!(s.chrome.titlebar_pinned && s.chrome.sidebar_overlay);
        assert!(!s.motion.enabled && !s.motion.sidebar_slide);
        let look = s.workspace.look;
        assert_eq!((look.gap, look.outline_width), (6.0, 3.0));
        assert_eq!(look.outline_color, Swatch::Custom);
        assert_eq!(look.outline_custom.to_hex(), "#ff0010");
        assert_eq!(look.corners, Corners::Rounded);
        assert!(look.shadow);
    }

    #[test]
    fn v1_column_width_moves_to_the_text() {
        let v1 = r#"{"version":1,"layout":{"column_width_pct":80,"page_margin":8}}"#;
        let got = load(v1);
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        assert_eq!(got.settings.text.column_width_pct, 80.0);
        assert_eq!(got.settings.layout.page_margin, 8.0);
        assert_eq!(got.settings.version, SCHEMA_VERSION);
    }

    #[test]
    fn v2_menus_gain_look_up_after_explain() {
        use crate::{BarItem, Pen};
        let v2 = r#"{"version":2,"study":{"highlights":{"bar":["explain",{"highlight":"full"},"copy"]}}}"#;
        let bar = load(v2).settings.study.highlights.bar;
        let want = [
            BarItem::Explain,
            BarItem::LookUp,
            BarItem::ReadAloud,
            BarItem::Note,
            BarItem::Highlight(Pen::Full),
            BarItem::Copy,
        ];
        assert_eq!(bar, want);
        let bare = r#"{"version":2,"study":{"highlights":{"bar":[]}}}"#;
        let bar = load(bare).settings.study.highlights.bar;
        assert_eq!(bar, [BarItem::LookUp, BarItem::ReadAloud, BarItem::Note]);
        // Note came in v4: a v4 menu without it stays so.
        let v4 = r#"{"version":4,"study":{"highlights":{"bar":["copy"]}}}"#;
        let bar = load(v4).settings.study.highlights.bar;
        assert_eq!(bar, [BarItem::Copy, BarItem::ReadAloud]);
    }

    #[test]
    fn v5_menus_gain_read_aloud_after_look_up() {
        use crate::{BarItem, Pen};
        let v5 = r#"{"version":5,"study":{"highlights":{"bar":["copy","look_up",{"highlight":"full"}]}}}"#;
        let bar = load(v5).settings.study.highlights.bar;
        let want = [
            BarItem::Copy,
            BarItem::LookUp,
            BarItem::ReadAloud,
            BarItem::Highlight(Pen::Full),
        ];
        assert_eq!(bar, want);
        let v6 = r#"{"version":6,"study":{"highlights":{"bar":["copy"]}}}"#;
        assert_eq!(load(v6).settings.study.highlights.bar, [BarItem::Copy]);
    }

    #[test]
    fn v6_motion_gains_icon_animations_on() {
        let v6 = r#"{"version":6,"motion":{"enabled":false,"sidebar_slide":false}}"#;
        let motion = load(v6).settings.motion;
        assert!(!motion.enabled && !motion.sidebar_slide);
        assert!(motion.icon_animations, "existing behaviour stays on");
    }

    #[test]
    fn v3_menus_gain_note_after_look_up() {
        use crate::{BarItem, Pen};
        let v3 = r#"{"version":3,"study":{"highlights":{"bar":["look_up",{"highlight":"full"}]}}}"#;
        let bar = load(v3).settings.study.highlights.bar;
        let want = [
            BarItem::LookUp,
            BarItem::ReadAloud,
            BarItem::Note,
            BarItem::Highlight(Pen::Full),
        ];
        assert_eq!(bar, want);
    }

    #[test]
    fn loading_clamps_out_of_range_values() {
        let json = r#"{"version":1,"text":{"font_size":400,"line_height":0.2}}"#;
        let text = load(json).settings.text;
        assert_eq!(text.font_size, 34.0);
        assert_eq!(text.line_height, 1.0);
    }
}
