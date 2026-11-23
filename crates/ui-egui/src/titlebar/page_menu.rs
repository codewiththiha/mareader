//! The pages beside the library: Space's button, and their shared menu.

use egui::Id;
use mareader_engine::command::Command;
use mareader_engine::route::Route;
use mareader_engine::space::Room;

use super::{BarContext, BarPage, DICTIONARY_MENU};
use crate::icons::Icon;
use crate::ui_command::{Commands, Sent};
use crate::view_menu;
use crate::widgets;

/// The library bar's way into Space, and Space's pressed way back.
pub(super) const SPACE: &str = "Space";
pub(super) const BACK: &str = "Back to the library";
const MENU_W: f32 = 248.0;

/// Space's button: a click shows `to`, which `label` names; Space opens at
/// its hub.
pub(super) fn space(ui: &mut egui::Ui, label: &str, on: bool, to: Route) -> Option<Sent> {
    let r = widgets::icon_button(ui, Icon::Levels, label, on);
    let go = match to {
        Route::Space => Command::ShowSpace(Room::Hub),
        other => Command::ShowRoute(other),
    };
    r.clicked().then_some(Sent::Engine(go))
}

/// The page's menu: the pin and the shortcuts; its choices sit on the page.
pub(super) fn show(ui: &mut egui::Ui, bar: &BarContext<'_>, out: &mut Commands) {
    let id = Id::new(DICTIONARY_MENU);
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    let label = match bar.page {
        BarPage::Space => "Space options",
        _ => "Dictionary options",
    };
    let r = widgets::icon_button(ui, Icon::Menu, label, open);
    egui::Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(MENU_W)
        .show(|ui| {
            ui.set_width(MENU_W);
            ui.spacing_mut().item_spacing.y = 2.0;
            view_menu::pin_row(ui, bar.pinned, out);
            view_menu::shortcuts_row(ui, bar.platform, out);
        });
}
