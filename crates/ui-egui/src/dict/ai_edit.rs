//! The AI dictionary's entry editor: every field, a delete, and a question
//! before an edit takes the place of another entry.

use egui::{Align, Context, Layout, Rect, RichText, Sense, Ui, Vec2, WidgetInfo, WidgetType, vec2};
use mareader_dict::{Saved, language, language_code};
use mareader_engine::command::Command;
use mareader_engine::dict::{Action, Edited};
use mareader_settings::LANGUAGES;
use mareader_study::Pos;

use super::{DictUi, entry};
use crate::icons::Icon;
use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;

/// The names screen readers and tests find the editor's parts by.
pub const EDIT: &str = "Edit this AI entry";
pub const TITLE: &str = "Edit AI entry";
pub const SAVE: &str = "Save";
pub const DELETE: &str = "Delete";
pub const DELETE_SURE: &str = "Delete for good";
pub const REPLACE: &str = "Replace it";
pub const CLASS: &str = "Class";
pub const LANGUAGE: &str = "Language";

/// An AI entry open for editing: its old self, and each field as typed.
pub struct Editor {
    old: Saved,
    word: String,
    /// One of `classes`, as stored.
    class: String,
    /// One of `languages`, by code.
    language: String,
    classes: Vec<String>,
    languages: Vec<String>,
    meaning: String,
    synonyms: String,
    usages: String,
    /// Saved or deleted, and waiting on the dictionary.
    sent: bool,
    /// Delete was pressed once; the next press deletes.
    deleting: bool,
}

impl Editor {
    pub fn new(old: Saved) -> Self {
        Self {
            word: old.word.clone(),
            class: old.pos.clone(),
            language: old.lang.clone(),
            classes: classes(&old.pos),
            languages: languages(&old.lang),
            meaning: old.meaning.clone(),
            synonyms: old.synonyms.join(", "),
            usages: old.usages.join("\n"),
            sent: false,
            deleting: false,
            old,
        }
    }

    /// The entry as typed; who wrote it and when stay as they were.
    pub fn edited(&self) -> Saved {
        Saved {
            word: self.word.trim().to_owned(),
            pos: self.class.clone(),
            lang: self.language.clone(),
            meaning: self.meaning.trim().to_owned(),
            synonyms: list(&self.synonyms, ','),
            usages: list(&self.usages, '\n'),
            ..self.old.clone()
        }
    }

    /// The first field left blank, by name, as a save needs them all.
    fn missing(&self) -> Option<&'static str> {
        let fields = [
            (&self.word, "a word"),
            (&self.class, "a class"),
            (&self.language, "a language"),
            (&self.meaning, "a meaning"),
        ];
        let blank = fields.into_iter().find(|(f, _)| f.trim().is_empty());
        blank.map(|(_, name)| name)
    }
}

/// The classes cards name; `old`, when none of them, stays one.
fn classes(old: &str) -> Vec<String> {
    let names = Pos::ALL.map(|p| p.name().to_owned());
    keep(names.into_iter(), old)
}

/// Each language the AI writes in, by stored code, and `old`.
fn languages(old: &str) -> Vec<String> {
    keep(LANGUAGES.iter().map(|(_, name)| language_code(name)), old)
}

/// `all` once each, then `old` when it is none of them.
fn keep(all: impl Iterator<Item = String>, old: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for choice in all.chain(std::iter::once(old.to_owned())) {
        if !choice.is_empty() && !out.contains(&choice) {
            out.push(choice);
        }
    }
    out
}

/// `text`'s parts between `sep`, trimmed, the empty ones dropped.
fn list(text: &str, sep: char) -> Vec<String> {
    let parts = text.split(sep).map(str::trim).filter(|s| !s.is_empty());
    parts.map(str::to_owned).collect()
}

/// The "AI" mark and, beside it, the button that opens the editor.
pub fn mark(ui: &mut Ui, saved: &Saved, t: &Tokens, out: &mut Commands) {
    entry::ai_badge(ui, &saved.model, t);
    pencil(ui, saved, out);
}

/// The mark with the pencil's spot kept; [`pin`] draws it there.
pub fn slot(ui: &mut Ui, saved: &Saved, t: &Tokens) -> Rect {
    entry::ai_badge(ui, &saved.model, t);
    ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover()).0
}

/// The pencil over a card's own click, so a press there is the pencil's.
pub fn pin(ui: &mut Ui, slot: Rect, (saved, at): (&Saved, usize), out: &mut Commands) {
    let place = egui::UiBuilder::new().max_rect(slot);
    let mut pinned = ui.new_child(place.id_salt(("ai-pencil", at)));
    pencil(&mut pinned, saved, out);
}

fn pencil(ui: &mut Ui, saved: &Saved, out: &mut Commands) {
    if widgets::small_icon_button(ui, Icon::Rename, EDIT).clicked() {
        out.push(Sent::Ui(UiCommand::EditAi(Box::new(saved.clone()))));
    }
}

impl DictUi {
    /// Opens the editor on an AI dictionary entry.
    pub fn edit_ai(&mut self, saved: Saved) {
        self.editor = Some(Editor::new(saved));
    }

    /// Whether the editor is open, for tests.
    pub fn editing_ai(&self) -> bool {
        self.editor.is_some()
    }

    /// The editor over everything while open; `edited` is the engine's word.
    pub fn show_editor(&mut self, ctx: &Context, edited: Option<&Edited>, out: &mut Commands) {
        let Some(editor) = &mut self.editor else {
            return;
        };
        if !show(ctx, editor, edited, out) {
            if edited.is_some_and(|e| *e != Edited::Waiting) {
                send(out, Action::DropEdit);
            }
            self.editor = None;
        }
    }
}

fn send(out: &mut Commands, action: Action) {
    out.push(Sent::Engine(Command::Dict(action)));
}

/// Draws the editor; false once it closes, saved, deleted or let go.
fn show(ctx: &Context, ed: &mut Editor, edited: Option<&Edited>, out: &mut Commands) -> bool {
    if ed.sent && edited.is_none() {
        return false;
    }
    let t = Tokens::get(ctx);
    let mut open = true;
    let shown = Dialog::new("ai-entry-editor").show(ctx, |ui| {
        ui.set_width(440.0);
        modal::title(ui, TITLE, &t);
        ui.add_space(8.0);
        fields(ui, ed, &t);
        ui.add_space(6.0);
        match edited.filter(|_| ed.sent) {
            Some(Edited::Taken(kept)) => taken(ui, ed, kept, &t, out),
            Some(Edited::Failed(why)) => {
                let line = format!("Couldn't save: {why}");
                ui.label(RichText::new(line).color(t.danger));
            }
            Some(Edited::Waiting) => {
                ui.label(RichText::new("Saving…").color(t.muted));
            }
            None => {}
        }
        ui.add_space(8.0);
        open = actions(ui, ed, edited, &t, out);
    });
    open && !shown.should_close()
}

/// Each field, its name above it; lists say how they split.
fn fields(ui: &mut Ui, ed: &mut Editor, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 4.0;
    field(ui, ("Word", ""), &mut ed.word, 1, t);
    ui.horizontal_top(|ui| {
        let half = (ui.available_width() - 8.0) / 2.0;
        let column = Layout::top_down(Align::Min);
        ui.allocate_ui_with_layout(vec2(half, 0.0), column, |ui| {
            let class = (&mut ed.class, ed.classes.as_slice());
            pick(ui, CLASS, class, str::to_owned, t);
        });
        ui.allocate_ui_with_layout(vec2(half, 0.0), column, |ui| {
            let lang = (&mut ed.language, ed.languages.as_slice());
            pick(ui, LANGUAGE, lang, language, t);
        });
    });
    field(ui, ("Meaning", ""), &mut ed.meaning, 2, t);
    let names = ("Synonyms", "Separated by commas");
    field(ui, names, &mut ed.synonyms, 1, t);
    field(ui, ("Usages", "One sentence a line"), &mut ed.usages, 3, t);
}

/// A field picked from `choices`, each shown as `shown` names it.
fn pick(
    ui: &mut Ui,
    name: &str,
    (value, choices): (&mut String, &[String]),
    shown: fn(&str) -> String,
    t: &Tokens,
) {
    ui.label(RichText::new(name).size(t.text_small).color(t.muted));
    let list = widgets::combo(("ai-entry", name))
        .width(ui.available_width())
        .height(280.0)
        .selected_text(shown(value))
        .show_ui(ui, |ui| {
            for choice in choices {
                ui.selectable_value(value, choice.clone(), shown(choice));
            }
        });
    let info = || WidgetInfo::labeled(WidgetType::ComboBox, true, name);
    list.response.widget_info(info);
    ui.add_space(4.0);
}

fn field(ui: &mut Ui, (name, hint): (&str, &str), text: &mut String, rows: usize, t: &Tokens) {
    ui.label(RichText::new(name).size(t.text_small).color(t.muted));
    let edit = match rows {
        1 => widgets::input(text),
        _ => widgets::input_multiline(text).rows(rows),
    };
    edit.hint(hint).label(name).show(ui);
    ui.add_space(4.0);
}

/// Another entry holds the new key: replace it, or keep editing.
fn taken(ui: &mut Ui, ed: &mut Editor, kept: &Saved, t: &Tokens, out: &mut Commands) {
    let new = ed.edited();
    let what = format!("{} ({}, {})", new.word, new.pos, language(&new.lang));
    let line = format!("The AI dictionary already has {what}: “{}”.", kept.meaning);
    ui.add(egui::Label::new(RichText::new(line).color(t.ink)).wrap());
    ui.horizontal(|ui| {
        if modal::primary(ui, REPLACE, t.danger, t).clicked() {
            send(out, edit(ed, true));
        }
        if modal::quiet(ui, "Keep editing", t).clicked() {
            send(out, Action::DropEdit);
            ed.sent = false;
        }
    });
}

/// The edit `ed` sends, over another entry when `replace`.
fn edit(ed: &Editor, replace: bool) -> Action {
    Action::Edit {
        old: Box::new(ed.old.clone()),
        new: Box::new(ed.edited()),
        replace,
    }
}

/// Save and Cancel right, Delete left; false once Cancel closes it.
fn actions(
    ui: &mut Ui,
    ed: &mut Editor,
    edited: Option<&Edited>,
    t: &Tokens,
    out: &mut Commands,
) -> bool {
    // Saving, or asking about a taken key: only Cancel or the question.
    let busy = matches!(edited, Some(Edited::Waiting | Edited::Taken(_)));
    let waiting = ed.sent && busy;
    let mut open = true;
    ui.horizontal(|ui| {
        let delete = if ed.deleting { DELETE_SURE } else { DELETE };
        let button = egui::Button::new(delete);
        let pressed = ui.add_enabled(!waiting, button).clicked();
        if pressed && ed.deleting {
            send(out, Action::Forget(Box::new(ed.old.clone())));
            ed.sent = true;
        }
        ed.deleting |= pressed;
        modal::buttons(ui, |ui| {
            let missing = ed.missing();
            let can = missing.is_none() && !waiting;
            let save = ui.add_enabled_ui(can, |ui| modal::primary(ui, SAVE, t.accent, t));
            let why = missing.map(|name| format!("The entry needs {name}."));
            let clicked = match why {
                Some(why) => save.inner.on_disabled_hover_text(why).clicked(),
                None => save.inner.clicked(),
            };
            if clicked && ed.edited() == ed.old {
                open = false;
            } else if clicked {
                send(out, edit(ed, false));
                ed.sent = true;
            }
            if modal::quiet(ui, "Cancel", t).clicked() {
                open = false;
            }
        });
    });
    open
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_edit_reads_each_field_and_keeps_who_wrote_it() {
        let old = Saved {
            word: "quick".into(),
            pos: "adjective".into(),
            lang: "my".into(),
            meaning: "fast".into(),
            synonyms: vec!["swift".into()],
            usages: vec!["A quick fox.".into()],
            model: "Model".into(),
            added: 7,
        };
        let mut ed = Editor::new(old.clone());
        assert_eq!(ed.edited(), old, "untouched, unchanged");
        ed.word = " Quick ".into();
        ed.class = "adverb".into();
        ed.language = "russian".into();
        ed.synonyms = "swift, , rapid".into();
        ed.usages = "Run quick.\n\nCome quick.".into();
        let new = ed.edited();
        assert_eq!((new.word.as_str(), new.pos.as_str()), ("Quick", "adverb"));
        assert_eq!(new.lang, "russian");
        assert_eq!(new.synonyms, ["swift", "rapid"]);
        assert_eq!(new.usages, ["Run quick.", "Come quick."]);
        assert_eq!((new.model.as_str(), new.added), ("Model", 7));
        ed.meaning = " ".into();
        assert_eq!(ed.missing(), Some("a meaning"));
    }

    #[test]
    fn class_and_language_are_picked_from_known_ones() {
        let langs = languages("my");
        let count = |code: &str| langs.iter().filter(|l| *l == code).count();
        assert_eq!((count("my"), count("zh"), count("jp")), (1, 1, 1));
        assert_eq!(count("russian"), 1, "a language no pack joins, by name");
        let odd = languages("klingon");
        assert_eq!(odd.last().map(String::as_str), Some("klingon"));
        assert_eq!(classes("noun").len(), Pos::ALL.len());
        assert_eq!(classes("phrase").last().map(String::as_str), Some("phrase"));
        assert!(classes("").iter().all(|c| !c.is_empty()));
    }
}
