//! The looks, the page's geometry and the text's, one key each.
//!
//! Every key here writes a setting by its dotted path, so a new dial needs no
//! new command. Two read the looks as they stand and write what they find:
//! `save_preset` keeps them as a preset, `label` writes three fields at once.

use mareader_appearance::{BaseMode, NoiseMode, TextureMode, builtin_presets};
use mareader_settings::{
    AppearanceMenuStyle, Bar, BookDesign, ColumnAlign, Corners, FloatingLabelStyle, FontChoice,
    PaperArea,
};

use super::{field, flag, named, number};
use crate::command::Command;
use crate::session::Session;

/// The looks' keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str, at: &Session) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "set" => by_path(value),
        "theme" => theme(value),
        "preset" => preset(value),
        "save_preset" => save_preset(value, at),
        "label" => label(value, at),
        "texture" => texture(value),
        "grain" => grain(value),
        "appearance_menu" => menu_style(value),
        "blend_area" => blend_area(value),
        "design" => design(value),
        "column" => column(value),
        "corners" => corners(value),
        "body_font" => field("text.default_font", FontChoice::parse(value)),
        "texture_scale" => scale(value),
        "grain_intensity" => intensity(value),
        "margin" => measure(value, "layout.page_margin", "margin: points"),
        "text_size" => measure(value, "text.font_size", "text_size: px"),
        "indent" => measure(value, "text.text_indent", "indent: ems"),
        "ink" => measure(value, "text.ink_contrast", "ink: %"),
        "paragraph" => measure(value, "text.paragraph_margin", "paragraph: ems"),
        "column_width" => measure(value, "text.column_width_pct", "column_width: %"),
        "pane_gap" => measure(value, "workspace.look.gap", "pane_gap: points"),
        "blend" => field("paper.blend", flag(value)),
        "animations" => field("motion.enabled", flag(value)),
        "float" => field("chrome.sidebar_overlay", flag(value)),
        "glass" => field("chrome.glass", flag(value)),
        "pinned" => pinned(value),
        // `gap` is the space between pages, so the setting is its opposite.
        "gap" => field("layout.no_gap", value == "false"),
        "justify" => field("text.justify", flag(value)),
        "override_font" => field("text.override_font", flag(value)),
        "override_layout" => field("text.override_layout", flag(value)),
        "theme_pictures" => field("text.theme_pictures", flag(value)),
        "theme_emojis" => field("text.theme_emojis", flag(value)),
        "hyphenate" => field("text.hyphenation", flag(value)),
        "book_layout" => field("text.book_layout", flag(value)),
        "shadow" => field("layout.page_shadow", flag(value)),
        _ => return None,
    };
    Some(parsed)
}

/// Any setting by its dotted path: `layout.page_indicator:false`.
fn by_path(value: &str) -> Result<Vec<Command>, String> {
    let (path, json) = value.split_once(':').ok_or("set: path:json")?;
    Ok(vec![Command::SetField {
        path: path.into(),
        value: json.into(),
    }])
}

/// A number written to `path`, with the key's own hint when it is not one.
fn measure(value: &str, path: &str, hint: &str) -> Result<Vec<Command>, String> {
    number::<f32>(value, hint).and_then(|n| field(path, n))
}

fn scale(value: &str) -> Result<Vec<Command>, String> {
    let hint = "texture_scale: 25 to 400";
    number::<u16>(value, hint).and_then(|n| field("appearance.texture_scale", n))
}

fn intensity(value: &str) -> Result<Vec<Command>, String> {
    let hint = "grain_intensity: 0 to 100";
    number::<u8>(value, hint).and_then(|n| field("appearance.noise_intensity", n))
}

fn theme(value: &str) -> Result<Vec<Command>, String> {
    let hint = "theme: light, dark, dim or black";
    let base = BaseMode::ALL
        .into_iter()
        .find(|b| b.label().eq_ignore_ascii_case(value))
        .ok_or(hint)?;
    field("appearance.base", base)
}

fn preset(value: &str) -> Result<Vec<Command>, String> {
    let hint = "preset: sepia, green, night, parchment, cinema";
    let found = builtin_presets()
        .into_iter()
        .find(|p| p.id == value)
        .ok_or(hint)?;
    field("appearance", found.appearance)
}

fn texture(value: &str) -> Result<Vec<Command>, String> {
    let hint = "texture: none, paper, lined, grid, dotted, cross";
    let mode = TextureMode::ALL
        .into_iter()
        .find(|m| format!("{m:?}").eq_ignore_ascii_case(value))
        .ok_or(hint)?;
    field("appearance.texture", mode)
}

fn grain(value: &str) -> Result<Vec<Command>, String> {
    let hint = "grain: off, static or animated";
    named(
        value,
        &[
            ("off", NoiseMode::Off),
            ("static", NoiseMode::Static),
            ("animated", NoiseMode::Animated),
        ],
        hint,
    )
    .and_then(|m| field("appearance.noise", m))
}

fn menu_style(value: &str) -> Result<Vec<Command>, String> {
    let hint = "appearance_menu: classic or dial_deck";
    named(
        value,
        &[
            ("classic", AppearanceMenuStyle::Classic),
            ("dial_deck", AppearanceMenuStyle::DialDeck),
            ("dial-deck", AppearanceMenuStyle::DialDeck),
        ],
        hint,
    )
    .and_then(|s| field("chrome.appearance_menu", s))
}

fn blend_area(value: &str) -> Result<Vec<Command>, String> {
    let hint = "blend_area: whole or edges";
    named(
        value,
        &[("whole", PaperArea::WholePage), ("edges", PaperArea::Edges)],
        hint,
    )
    .and_then(|a| field("paper.area", a))
}

fn design(value: &str) -> Result<Vec<Command>, String> {
    let hint = "design: auto, on or off";
    named(
        value,
        &[
            ("auto", BookDesign::Auto),
            ("on", BookDesign::On),
            ("off", BookDesign::Off),
        ],
        hint,
    )
    .and_then(|d| field("text.book_design", d))
}

fn column(value: &str) -> Result<Vec<Command>, String> {
    let hint = "column: left, center or right";
    named(
        value,
        &[
            ("left", ColumnAlign::Left),
            ("center", ColumnAlign::Center),
            ("right", ColumnAlign::Right),
        ],
        hint,
    )
    .and_then(|a| field("text.column_align", a))
}

/// Anything not `rounded` is square, as the front end read it.
fn corners(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "rounded" => Corners::Rounded,
        _ => Corners::Square,
    };
    field("workspace.look.corners", some)
}

/// Every bar's pin at once: a shot asks for one look, not for three.
fn pinned(value: &str) -> Result<Vec<Command>, String> {
    let on = flag(value);
    let mut all = Vec::new();
    for bar in Bar::ALL {
        all.extend(field(bar_path(bar), on)?);
    }
    Ok(all)
}

/// The setting a bar's pin lives in.
fn bar_path(bar: Bar) -> &'static str {
    match bar {
        Bar::Reader => "chrome.titlebar_pinned",
        Bar::Library => "chrome.library_titlebar_pinned",
        Bar::Dictionary => "chrome.dictionary_titlebar_pinned",
    }
}

/// `Name` or `Name/Section`, keeping the look the session holds.
fn save_preset(value: &str, at: &Session) -> Result<Vec<Command>, String> {
    let (name, group) = value.split_once('/').unwrap_or((value, ""));
    Ok(vec![Command::SavePreset {
        name: name.to_owned(),
        group: group.to_owned(),
        look: at.settings().appearance,
    }])
}

/// The floating label on, off, or kept by chapter: three fields at once.
fn label(value: &str, at: &Session) -> Result<Vec<Command>, String> {
    let mut layout = at.settings().layout.clone();
    layout.floating_label = value != "false";
    layout.floating_label_persist = value == "persist";
    if value == "chapter" {
        layout.floating_label_style = FloatingLabelStyle::Chapter;
    }
    field("layout", layout)
}
