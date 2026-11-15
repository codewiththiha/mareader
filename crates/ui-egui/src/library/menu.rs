//! The library's View menu: layout, columns, covers and order.

use egui::{Id, Popup, PopupCloseBehavior, RectAlign, RichText, Ui};
use mareader_library::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, SortKey, View};

use crate::icons::Icon;
use crate::platform::Platform;
use crate::titlebar::LibraryBar;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::library::{Action, ShelfAction};

pub const LIBRARY_VIEW_MENU: &str = "library-view-menu";
/// The sort orders' fold, and its name.
pub const SORT_FOLD: &str = "Sort by";
const WIDTH: f32 = 248.0;
/// A menu row's height, as `widgets::menu_row` draws it.
const ROW_H: f32 = 30.0;

/// The menu's button; `auto` is the grid's automatic column count.
pub fn button(
    ui: &mut Ui,
    library: &LibraryBar<'_>,
    pinned: bool,
    platform: Platform,
    out: &mut Commands,
) {
    let (view, auto) = (library.view, library.auto_columns);
    let id = Id::new(LIBRARY_VIEW_MENU);
    let open = Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::ViewOptions, "View options", open);
    Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| {
            widgets::menu_style(ui, WIDTH);
            // Every change shows at once; the menu stays open to chain them.
            let mut next = *view;
            if widgets::menu_row(ui, Icon::FolderPlus, "New shelf", None, false).clicked() {
                out.push(Sent::Engine(Command::Library(Action::Shelf(
                    ShelfAction::New,
                ))));
                ui.close();
            }
            let organizer = super::menus::Organizer {
                busy: library.organizing,
                paused: library.paused.as_deref(),
            };
            super::menus::organize_rows(ui, organizer, out);
            crate::view_menu::pin_row(ui, pinned, out);
            crate::view_menu::shortcuts_row(ui, platform, out);
            widgets::menu_rule(ui);
            contents(ui, &mut next, auto);
            if next != *view {
                out.push(Sent::Engine(Command::SetLibraryView(next)));
            }
            widgets::close_on_key(ui);
        });
}

fn contents(ui: &mut Ui, view: &mut View, auto: u8) {
    widgets::menu_section(ui, None, "Layout");
    for (layout, icon, label) in [
        (Layout::Grid, Icon::Grid, "Grid"),
        (Layout::List, Icon::List, "List"),
    ] {
        if widgets::menu_row(ui, icon, label, None, view.layout == layout).clicked() {
            view.layout = layout;
        }
    }
    let grid = view.layout == Layout::Grid;
    ui.add_enabled_ui(grid, |ui| columns(ui, view, auto));
    widgets::menu_rule(ui);
    widgets::menu_section(ui, None, "Book covers");
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        for (fit, label) in [(CoverFit::Fit, "Fit"), (CoverFit::Crop, "Crop")] {
            let button = egui::Button::selectable(view.cover == fit, label);
            if ui.add_sized([(WIDTH - 16.0) / 2.0, 28.0], button).clicked() {
                view.cover = fit;
            }
        }
    });
    widgets::menu_rule(ui);
    // Shut, it keeps the order in use and its direction.
    let fold = crate::fold::title(ui, egui::Id::new(SORT_FOLD), SORT_FOLD, None);
    let rows = SortKey::ALL.map(|key| (key, view.sort == key));
    let mut picked = None;
    crate::fold::rows(ui, fold, (ROW_H, 0.0), rows, |ui, key, live| {
        let on = view.sort == key;
        let r = widgets::menu_row(ui, Icon::Sort, key.label(), None, on);
        if live && r.clicked() && !on {
            picked = Some(key);
        }
    });
    if let Some(key) = picked {
        view.set_sort(key);
    }
    // The reader's own order has no other way round.
    if let Some([up, down]) = view.sort.directions() {
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            for (ascending, label) in [(true, up), (false, down)] {
                let button = egui::Button::selectable(view.ascending == ascending, label);
                if ui.add_sized([(WIDTH - 16.0) / 2.0, 28.0], button).clicked() {
                    view.ascending = ascending;
                }
            }
        });
    }
    widgets::menu_rule(ui);
    // Shelved books can stay on the top level too, or only on shelves.
    widgets::menu_section(ui, None, "Library shows");
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        for (home, label) in [
            (Home::Everything, "All books"),
            (Home::Unshelved, "Unshelved"),
        ] {
            let button = egui::Button::selectable(view.home == home, label);
            if ui.add_sized([(WIDTH - 16.0) / 2.0, 28.0], button).clicked() {
                view.home = home;
            }
        }
    });
}

/// Auto, or a pinned count stepped from what Auto shows now.
fn columns(ui: &mut Ui, view: &mut View, auto: u8) {
    ui.horizontal(|ui| {
        ui.add_space(ui.spacing().button_padding.x);
        ui.label("Columns");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let shown = view.columns.unwrap_or(auto);
            let more = ui.add_enabled_ui(shown < COLUMNS_MAX, |ui| {
                widgets::small_icon_button(ui, Icon::Plus, "More columns")
            });
            let count = RichText::new(shown.to_string()).strong();
            ui.add_sized([22.0, 24.0], egui::Label::new(count));
            let fewer = ui.add_enabled_ui(shown > COLUMNS_MIN, |ui| {
                widgets::small_icon_button(ui, Icon::Minus, "Fewer columns")
            });
            let auto_on = view.columns.is_none();
            if ui.add(egui::Button::selectable(auto_on, "Auto")).clicked() {
                view.columns = None;
            }
            if more.inner.clicked() {
                view.columns = Some(shown.saturating_add(1).min(COLUMNS_MAX));
            }
            if fewer.inner.clicked() {
                view.columns = Some(shown.saturating_sub(1).max(COLUMNS_MIN));
            }
        });
    });
}
