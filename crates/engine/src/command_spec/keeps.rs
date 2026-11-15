//! The forms a script sends for what is kept: choices, presets, an export.

use serde_json::Value;

use crate::appearance_menu::QuickText;
use crate::command::{Command, Independence};
use crate::command_spec::{json, marks, names};
use crate::session::Session;
use crate::settings::Tab;

/// `settings.open`: a tab, or a model's voices, which is a tab of its own.
pub(crate) fn open(_at: &Session, params: &Value) -> Result<Command, String> {
    let model = json::maybe_text(params, "model")?;
    let tab = match model {
        Some(name) => Tab::Model(marks::model(&name)?),
        None => json::one_of(params, "tab", names::TAB)?,
    };
    Ok(Command::OpenSettings(tab))
}

/// `settings.write`: a whole set of choices, as the settings window sends them.
pub(crate) fn write(_at: &Session, params: &Value) -> Result<Command, String> {
    let found = json::object(params, "settings")?;
    let loaded = mareader_settings::load(&found.to_string());
    if let Some(note) = loaded.notes.first() {
        return Err(format!("\"settings\" wants choices that load: {note}"));
    }
    Ok(Command::SetSettings(Box::new(loaded.settings)))
}

/// `settings.set`: one setting by its dotted path, to any JSON value.
///
/// The path is the one an option key names, and the value arrives as the text
/// the option door takes: a JSON string is its own text, and anything else is
/// written out, so `true` and `"true"` both set a flag.
pub(crate) fn set_field(_at: &Session, params: &Value) -> Result<Command, String> {
    let path = json::text(params, "path")?;
    let found = json::held(params, "value")?;
    let value = match found {
        Value::String(text) => text.clone(),
        _ => found.to_string(),
    };
    Ok(Command::SetField { path, value })
}

/// `settings.appearance`: the look, in the shape the popover sends.
pub(crate) fn appearance(_at: &Session, params: &Value) -> Result<Command, String> {
    let look = json::shape(params, "look", LOOK)?;
    Ok(Command::SetAppearance(look))
}

/// What a look holds, for a refusal that says so.
const LOOK: &str = "a look: base, tint, texture, noise";

/// `settings.quick-text`: the popover's dials, each left as it is when absent.
pub(crate) fn quick_text(at: &Session, params: &Value) -> Result<Command, String> {
    let mut dials = QuickText::of(&at.settings().text);
    if let Some(size) = json::maybe_real(params, "size_pct")? {
        dials.size_pct = size;
    }
    if let Some(height) = json::maybe_real(params, "line_height")? {
        dials.line_height = height;
    }
    if let Some(margin) = json::maybe_real(params, "paragraph_margin")? {
        dials.paragraph_margin = margin;
    }
    if let Some(font) = json::maybe_flag(params, "override_font")? {
        dials.override_font = font;
    }
    if let Some(layout) = json::maybe_flag(params, "override_layout")? {
        dials.override_layout = layout;
    }
    Ok(Command::SetQuickText(dials))
}

/// `settings.independence`: whether a split takes its own colour and texture.
pub(crate) fn independence(_at: &Session, params: &Value) -> Result<Command, String> {
    let themes = json::off_unless(params, "themes")?;
    let textures = json::off_unless(params, "textures")?;
    Ok(Command::SetIndependence(Independence { themes, textures }))
}

/// `settings.ink`: how dark a book's text stays, 10 to 100.
pub(crate) fn ink(_at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::SetInkContrast(json::real(params, "ink")?))
}

/// `settings.preset-save`: a name, a section when it has one, and the look.
pub(crate) fn preset_save(_at: &Session, params: &Value) -> Result<Command, String> {
    let name = json::text(params, "name")?;
    let group = json::maybe_text(params, "group")?.unwrap_or_default();
    let look = json::shape(params, "look", LOOK)?;
    Ok(Command::SavePreset { name, group, look })
}

/// `settings.preset-delete`: the name of one of the user's presets.
pub(crate) fn preset_delete(_at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::DeletePreset(json::text(params, "name")?))
}

/// `settings.sidebar-overlay`: floating over the panes, or docked beside them.
pub(crate) fn sidebar(_at: &Session, params: &Value) -> Result<Command, String> {
    let overlay = json::flag(params, "overlay")?;
    Ok(Command::SetSidebarOverlay(overlay))
}

/// `settings.titlebar-pin`: the title bar kept up, or following the pointer.
pub(crate) fn titlebar(_at: &Session, params: &Value) -> Result<Command, String> {
    let pinned = json::flag(params, "pinned")?;
    Ok(Command::SetTitlebarPinned(pinned))
}

/// `library.view`: the library page's layout and order.
pub(crate) fn library_view(_at: &Session, params: &Value) -> Result<Command, String> {
    let what = "a library view: layout, cover, sort, home";
    let view = json::shape(params, "view", what)?;
    Ok(Command::SetLibraryView(view))
}

/// `dict.settings`: the Dictionary panel's choices.
pub(crate) fn dictionary(_at: &Session, params: &Value) -> Result<Command, String> {
    let what = "the dictionary's choices: hover, langs, window";
    let held = json::shape(params, "dictionary", what)?;
    Ok(Command::SetDictionary(Box::new(held)))
}

/// `export.write`: a name, a format and the text that goes in it.
///
/// When `contents` is omitted, renders the active (or named) pane's highlights
/// and notes in `format` and defaults `name` from the document title.
pub(crate) fn export(at: &Session, params: &Value) -> Result<Command, String> {
    let format = json::one_of(params, "format", names::FORMAT)?;
    let (name, contents) = match json::maybe_text(params, "contents")? {
        Some(contents) => (json::text(params, "name")?, contents),
        None => {
            let pane = json::pane(at, params)?;
            let (default_name, rendered) = at.export_notes(pane, format);
            let name = json::maybe_text(params, "name")?.unwrap_or(default_name);
            (name, rendered)
        }
    };
    Ok(Command::Export {
        name,
        format,
        contents,
    })
}

/// `command.batch`: runs a list of `{id, params}` steps in order.
pub(crate) fn batch(_at: &Session, params: &Value) -> Result<Command, String> {
    let steps: Vec<crate::journal::Step> =
        json::shape(params, "commands", "a list of {id, params} steps")?;
    let max = crate::journal::MAX_BATCH_LEN;
    if steps.len() > max {
        return Err(format!("batch exceeds {max} commands"));
    }
    Ok(Command::Batch(steps))
}
