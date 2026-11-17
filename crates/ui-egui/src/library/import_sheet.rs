//! The folder import sheet: a short form, each answer on one line.

use std::path::Path;

use egui::{
    Align, CursorIcon, Frame, Id, Key, Label, Layout, Margin, Response, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, Ui, WidgetInfo, WidgetType,
};
use mareader_library::{Format, Library, MIN_SIZE_MAX, Options};

use crate::icons::Icon;
use crate::modal::{buttons, primary, title};
use crate::theme::Tokens;
use crate::ui_command::{LibraryDialog, Sent, UiCommand};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::library::{Action, State as LibraryState};

/// Wider than the other sheets, as forms read better so.
const WIDTH: f32 = 460.0;
/// A nested choice sits under its parent's words.
const NEST: i8 = 22;

/// Enter imports, Escape or Cancel keeps the library as it was.
pub(super) fn show(ui: &mut Ui, library: &LibraryState, root: &Path, t: &Tokens) -> Option<Sent> {
    let model = library.model();
    let width = WIDTH.min(ui.ctx().content_rect().width() - 80.0);
    ui.set_width(width.max(280.0));
    title(ui, "Import a folder", t);
    let key = Id::new(("import-folder", root));
    // The last answers start it; a tree already read here seeds its own.
    let mut opts = ui.data_mut(|d| {
        d.get_temp_mut_or_insert_with(key, || seed(model, root, library.last_options()))
            .clone()
    });
    let inside = (model.folders())
        .any(|f| f.mode().in_place() && root.starts_with(&f.root) && root != f.root);
    let mut act = None;
    ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.7)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add_space(6.0);
            heading(ui, "Folder", t);
            if folder_row(ui, root, t).clicked() {
                act = Some(Sent::Ui(UiCommand::LibraryDialog(LibraryDialog::Folder)));
            }
            ui.add_space(8.0);
            heading(ui, "File formats", t);
            formats(ui, &mut opts, t);
            ui.add_space(8.0);
            min_size(ui, &mut opts, t);
            ui.add_space(8.0);
            held(ui, &mut opts, inside, t);
            ui.add_space(8.0);
            heading(ui, "Folder structure", t);
            let shapes = [
                (
                    true,
                    "A shelf for each folder",
                    "Each subfolder gets its own shelf.",
                ),
                (
                    false,
                    "One shelf for everything",
                    "Every book goes on one shelf.",
                ),
            ];
            for (groups, name, hint) in shapes {
                let text = RichText::new(name).color(t.ink);
                hinted(
                    ui,
                    0,
                    |ui| ui.radio_value(&mut opts.groups, groups, text),
                    hint,
                    t,
                );
            }
            if inside {
                let says =
                    "Inside a folder already read: this answer covers it and what's under it.";
                small(ui, says, t);
            }
        });
    let admits = Format::ALL.into_iter().any(|f| admitted(&opts, f));
    let free = ui.ctx().memory(|m| m.focused().is_none());
    let enter = free && ui.input(|i| i.key_pressed(Key::Enter));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        buttons(ui, |ui| {
            let go = ui.add_enabled_ui(admits, |ui| primary(ui, "Import", t.accent, t));
            if go.inner.clicked() || (enter && admits) {
                let import = Action::ImportFolder {
                    root: root.to_path_buf(),
                    opts: opts.clone(),
                };
                act = Some(Sent::Engine(Command::Library(import)));
            }
            if ui.button("Cancel").clicked() {
                act = Some(Sent::Engine(Command::Library(Action::CloseSheet)));
            }
            if !admits {
                small(ui, "Choose a format.", t);
            }
        });
    });
    let keep = matches!(act, Some(Sent::Ui(UiCommand::LibraryDialog(_))) | None);
    keep_options(ui, key, opts, keep);
    act
}

fn keep_options(ui: &Ui, key: egui::Id, opts: Options, keep: bool) {
    ui.data_mut(|d| {
        if keep {
            d.insert_temp(key, opts);
        } else {
            d.remove::<Options>(key);
        }
    });
}

/// The sheet's start: `last`, with the watch and shape of a tree here.
fn seed(model: &Library, root: &Path, last: Options) -> Options {
    let standing = model.folder_at(root, true).and_then(|id| model.folder(id));
    match standing.filter(|f| f.mode().in_place()) {
        Some(tree) => Options {
            in_place: true,
            watch: tree.opts.watch,
            groups: tree.opts.groups,
            ..last
        },
        None => last,
    }
}

/// A quiet label over a group of answers.
fn heading(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

fn small(ui: &mut Ui, text: &str, t: &Tokens) {
    ui.add(Label::new(RichText::new(text).size(t.text_small).color(t.muted)).wrap());
}

/// The folder as a field: the whole row picks another.
fn folder_row(ui: &mut Ui, root: &Path, t: &Tokens) -> Response {
    let full = root.display().to_string();
    let shown = Frame::new()
        .fill(t.line.gamma_multiply(0.35))
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(Icon::Folder.image(ui, 16.0).tint(t.muted));
                let path = RichText::new(&full).color(t.ink);
                ui.add(Label::new(path).truncate().selectable(false));
            });
        });
    let rect = shown.response.rect;
    let resp = ui.interact(rect, ui.id().with("import-folder-row"), Sense::click());
    let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
    let resp = resp.on_hover_text(full);
    let label = "Choose another folder";
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    if resp.hovered() || resp.has_focus() {
        let ring = Stroke::new(1.0, t.muted);
        (ui.painter()).rect_stroke(rect, t.radius, ring, StrokeKind::Inside);
    }
    resp
}

/// Whether a file of `format` counts, whichever way the set is kept.
fn admitted(opts: &Options, format: Format) -> bool {
    opts.include == opts.formats.contains(&format)
}

/// A box per format; ticked ones count. Untick to leave one out.
fn formats(ui: &mut Ui, opts: &mut Options, t: &Tokens) {
    let mut taken: Vec<Format> = (Format::ALL.into_iter())
        .filter(|f| admitted(opts, *f))
        .collect();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        for format in Format::ALL {
            let mut on = taken.contains(&format);
            let text = RichText::new(format.label()).color(t.ink);
            if widgets::checkbox(ui, &mut on, text).changed() {
                match on {
                    true => taken.push(format),
                    false => taken.retain(|f| *f != format),
                }
                changed = true;
            }
        }
    });
    if changed {
        opts.include = true;
        opts.formats = taken.into_iter().collect();
    }
}

/// "File size larger than" and its dial, on one line.
fn min_size(ui: &mut Ui, opts: &mut Options, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("File size larger than").color(t.ink));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let more = ui.add_enabled_ui(opts.min_size < MIN_SIZE_MAX, |ui| {
                widgets::small_icon_button(ui, Icon::Plus, "Only larger files")
            });
            let value = RichText::new(opts.min_size_label()).color(t.ink);
            ui.add_sized([56.0, 24.0], Label::new(value));
            let less = ui.add_enabled_ui(opts.min_size > 0, |ui| {
                widgets::small_icon_button(ui, Icon::Minus, "Smaller files count too")
            });
            if more.inner.clicked() {
                opts.step_min_size(1);
            }
            if less.inner.clicked() {
                opts.step_min_size(-1);
            }
        });
    });
}

/// Read in place or copied, and, read in place, watched or not.
fn held(ui: &mut Ui, opts: &mut Options, inside: bool, t: &Tokens) {
    let text = RichText::new("Read books in place").color(t.ink);
    let hint = "Read from the folder instead of copying into the library.";
    hinted(
        ui,
        0,
        |ui| widgets::checkbox(ui, &mut opts.in_place, text),
        hint,
        t,
    );
    if !opts.in_place {
        opts.watch = false;
        return;
    }
    let text = RichText::new("Check for new books").color(t.ink);
    let hint = match inside {
        true => "Only this subfolder is looked through again.",
        false => "Looked through again when the app opens or comes back.",
    };
    hinted(
        ui,
        NEST,
        |ui| widgets::checkbox(ui, &mut opts.watch, text),
        hint,
        t,
    );
}

/// A checkbox or radio with a small line under its words.
fn hinted(ui: &mut Ui, indent: i8, add: impl FnOnce(&mut Ui) -> Response, hint: &str, t: &Tokens) {
    let nest = Margin {
        left: indent,
        ..Margin::ZERO
    };
    Frame::new().inner_margin(nest).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        add(ui);
        let under = Margin {
            left: widgets::CHECKBOX_INDENT.round() as i8,
            ..Margin::ZERO
        };
        Frame::new()
            .inner_margin(under)
            .show(ui, |ui| small(ui, hint, t));
    });
}
