//! Adding books: the plus card, its one menu, and what a folder gives back.

use egui::epaint::CornerRadiusF32;
use egui::epaint::tessellator::path::rounded_rectangle;
use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Popup, PopupCloseBehavior, Rect, Response, RichText,
    Sense, Shape, Stroke, Ui, pos2, vec2,
};
use mareader_library::{FolderId, Library, Recovered, ShelfId, dir_of_rung, human_age, human_size};

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, LibraryDialog, Sent, UiCommand};
use mareader_engine::command::Command;
use mareader_engine::library::{Action, ShelfAction};

pub const ADD_MENU: &str = "library-add";
const WIDTH: f32 = 300.0;
const ROW_H: f32 = 46.0;

/// Where the menu adds: the shelf open, if any.
#[derive(Clone, Copy)]
pub struct Here<'a> {
    pub model: &'a Library,
    pub at: Option<ShelfId>,
}

/// The plus card ending the grid: a book-sized slot, a menu behind it.
pub fn card(ui: &mut Ui, cover: Rect, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    let resp = ui.interact(cover, Id::new("library-add-card"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add books"));
    let open = Popup::is_id_open(ui.ctx(), Id::new(ADD_MENU));
    let lit = resp.hovered() || open;
    let p = ui.painter();
    // An empty slot on the shelf: dashed, filling in only when pointed at.
    if lit {
        p.rect_filled(cover, CornerRadius::same(6), t.hover.gamma_multiply(0.7));
    }
    let mut edge = Vec::new();
    rounded_rectangle(&mut edge, cover.shrink(0.5), CornerRadiusF32::same(6.0));
    if let Some(&start) = edge.first() {
        edge.push(start);
    }
    let dash = t.muted.gamma_multiply(if lit { 1.0 } else { 0.55 });
    p.extend(Shape::dashed_line(&edge, Stroke::new(1.25, dash), 6.0, 5.0));
    let d = (cover.width() * 0.24).clamp(36.0, 52.0);
    let middle = cover.center() - vec2(0.0, d * 0.3);
    // Pointed at, the plus fills with the accent: this is the action.
    let (disc, plus) = match lit {
        true => (t.accent, theme::ink_on(t.accent)),
        false => (t.hover, t.muted),
    };
    p.circle_filled(middle, d / 2.0, disc);
    let mark = Rect::from_center_size(middle, vec2(d * 0.42, d * 0.42));
    Icon::Plus.paint(ui, mark, plus);
    let ink = if lit { t.ink } else { t.muted };
    let at = pos2(middle.x, middle.y + d / 2.0 + 10.0);
    let font = theme::medium(13.0);
    p.text(at, Align2::CENTER_TOP, "Add books", font, ink);
    menu(&resp, here, t, out);
}

/// The list's last row: the same menu, in a row's shape.
pub fn row(ui: &mut Ui, rect: Rect, inset: f32, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    let resp = ui.interact(rect, Id::new("library-add-row"), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Add books"));
    if resp.hovered() || Popup::is_id_open(ui.ctx(), Id::new(ADD_MENU)) {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let icon = Rect::from_center_size(
        pos2(rect.left() + inset + 17.0, rect.center().y),
        vec2(18.0, 18.0),
    );
    Icon::Plus.paint(ui, icon, t.muted);
    let at = pos2(icon.right() + 22.0, rect.center().y);
    let font = FontId::proportional(14.0);
    ui.painter()
        .text(at, Align2::LEFT_CENTER, "Add books", font, t.muted);
    menu(&resp, here, t, out);
}

/// The empty library's one button.
pub fn button(ui: &mut Ui, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    let button = crate::widgets::primary_button(ui, Some(Icon::Plus), "Add books", t.accent, t)
        .min_size(vec2(132.0, 34.0));
    let resp = ui.add(button);
    menu(&resp, here, t, out);
}

/// One menu for every way books arrive, anchored under `anchor`.
fn menu(anchor: &Response, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    Popup::from_toggle_button_response(anchor)
        .id(Id::new(ADD_MENU))
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| {
            contents(ui, here, t, out);
            crate::widgets::close_on_key(ui);
        });
}

fn contents(ui: &mut Ui, here: Here<'_>, t: &Tokens, out: &mut Commands) {
    ui.set_width(WIDTH);
    ui.spacing_mut().item_spacing.y = 2.0;
    let onto = match here.at {
        Some(_) => "Copies the files onto this shelf",
        None => "Copies the files into your library",
    };
    if entry(ui, Icon::Import, "Choose files…", onto, t).clicked() {
        out.push(Sent::Ui(UiCommand::LibraryDialog(LibraryDialog::Import)));
    }
    let folder = "Copy it in, or read it where it is";
    if entry(ui, Icon::FolderPlus, "Choose a folder…", folder, t).clicked() {
        out.push(Sent::Ui(UiCommand::LibraryDialog(LibraryDialog::Folder)));
    }
    // A folder's shelf offers its own files, and what left it.
    let shelf = here.at.and_then(|id| here.model.shelf(id));
    let rung = shelf.and_then(|s| s.kind.value.rung());
    let rung = rung.and_then(|(id, key)| Some((here.model.folder(id)?, key)));
    if let Some((folder, key)) = rung {
        let dir = dir_of_rung(&folder.root, key);
        let from = "Choose files from this folder";
        if entry(ui, Icon::Folder, from, &folder.rung_name(key), t).clicked() {
            out.push(Sent::Ui(UiCommand::LibraryDialog(LibraryDialog::From(dir))));
        }
    }
    ui.separator();
    let (label, detail) = match here.at {
        Some(_) => ("New shelf inside", "A shelf on this shelf"),
        None => ("New shelf", "Drag books onto it to file them"),
    };
    if entry(ui, Icon::Shelf, label, detail, t).clicked() {
        out.push(Sent::Engine(Command::Library(Action::Shelf(
            ShelfAction::New,
        ))));
    }
    if let Some((folder, _)) = rung {
        restore(ui, here.model, folder.id, t, out);
    }
}

/// What the folder can give back: removals first, then moves.
fn restore(ui: &mut Ui, model: &Library, folder: FolderId, t: &Tokens, out: &mut Commands) {
    let back = model.recoverables(folder);
    if back.is_empty() {
        return;
    }
    ui.separator();
    ui.label(RichText::new("Restore").size(11.5).color(t.muted));
    let now = mareader_engine::library::wall_ms();
    for item in back {
        match item {
            Recovered::Removed(stone) => {
                let there = stone.last_path.is_file();
                let mut detail = format!("removed {}", human_age(stone.removed_ms, now));
                if stone.size > 0 {
                    detail = format!("{detail} · {}", human_size(stone.size));
                }
                if !there {
                    detail = "not there any more".into();
                }
                let label = stone.label();
                let resp =
                    ui.add_enabled_ui(there, |ui| entry(ui, Icon::Restore, &label, &detail, t));
                let hint = "Add this file back, even if it doesn't match the folder's filters";
                if resp.inner.on_hover_text(hint).clicked() {
                    let content = stone.content;
                    out.push(Sent::Engine(Command::Library(Action::Restore {
                        folder,
                        content,
                    })));
                }
            }
            Recovered::Moved {
                book, title, home, ..
            } => {
                let detail = match home {
                    Some(shelf) => format!("now on “{shelf}”"),
                    None => "in the library, on no shelf".into(),
                };
                let resp = entry(ui, Icon::Restore, &title, &detail, t);
                if resp
                    .on_hover_text("This book moved to another shelf")
                    .clicked()
                {
                    out.push(Sent::Engine(Command::Library(Action::AskMoved {
                        folder,
                        book,
                    })));
                }
            }
        }
    }
}

/// A menu row: an icon, a title and what it does underneath.
fn entry(ui: &mut Ui, icon: Icon, title: &str, detail: &str, t: &Tokens) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let mark = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(17.0, 17.0));
    icon.paint(ui, mark, t.muted);
    let x = mark.right() + 12.0;
    let room = rect.right() - x - 8.0;
    let line = |text: &str, font: FontId, color: Color32, top: f32| {
        let text = egui::WidgetText::from(RichText::new(text).font(font).color(color));
        let wrap = Some(egui::TextWrapMode::Truncate);
        let galley = text.into_galley(ui, wrap, room.max(16.0), egui::FontSelection::Default);
        ui.painter()
            .galley(pos2(x, rect.top() + top), galley, color);
    };
    line(title, theme::medium(13.5), t.ink, 6.0);
    line(detail, FontId::proportional(12.0), t.muted, 25.0);
    resp
}
