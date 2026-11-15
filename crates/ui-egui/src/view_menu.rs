//! The title bar's menu: crowded-out tools, full screen, the shortcut list.

use egui::{
    Align, Id, Layout, Popup, PopupCloseBehavior, Rect, RectAlign, RichText, Sense, Ui, pos2, vec2,
};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::keymap;
use crate::platform::Platform;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand, WindowCommand};
use crate::widgets;
use crate::zoom::ZoomState;
use mareader_engine::command::Command;
use mareader_engine::pane::ViewMode;
use mareader_engine::pane::{Fit, Step, ZoomCommand};

pub const VIEW_MENU: &str = "view-menu";
const WIDTH: f32 = 272.0;

/// A title bar tool; folds into the menu when the bar runs out of room.
#[derive(Clone, Debug, PartialEq)]
pub struct Tool {
    pub icon: Icon,
    pub label: &'static str,
    pub command: Sent,
}

pub struct MenuContext<'a> {
    /// Tools the bar had no room for, in bar order.
    pub folded: &'a [Tool],
    pub fullscreen: bool,
    pub platform: Platform,
    /// The active document's zoom and mode; no rows without one.
    pub zoom: Option<ZoomState>,
    pub mode: Option<ViewMode>,
    pub auto_scroll: Option<bool>,
    /// Whether a book reads aloud; no row without a book.
    pub aloud: Option<bool>,
    /// The title bar stays shown instead of waiting for a hover.
    pub pinned: bool,
}

/// The menu button and its popup.
pub fn button(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    let id = Id::new(VIEW_MENU);
    let open = Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::Menu, "View & tools", open);
    Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(WIDTH)
        .show(|ui| contents(ui, cx, out));
}

fn contents(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    // Menus animate on hover, before a selection can close them.
    let _act = anim::announce(ui, Act::Hover);
    widgets::menu_style(ui, WIDTH);
    // Layout, zoom and reading each under a name of their own.
    if let Some(mode) = cx.mode {
        widgets::menu_section(ui, None, "Layout");
        view_row(ui, mode, cx.platform, out);
    }
    if let Some(zoom) = cx.zoom {
        widgets::menu_section(ui, None, "Zoom");
        zoom_row(ui, zoom, cx.platform, out);
    }
    widgets::menu_section(ui, None, "Reading");
    if let (Some(on), Some(mode)) = (cx.auto_scroll, cx.mode) {
        auto_scroll_row(ui, on, !mode.paged(), cx.platform, out);
    }
    if let Some(on) = cx.aloud {
        read_aloud_row(ui, on, cx.platform, out);
    }
    let full = Sent::Ui(UiCommand::Window(WindowCommand::ToggleFullscreen));
    let (icon, label) = if cx.fullscreen {
        (Icon::ExitFullscreen, "Exit full screen")
    } else {
        (Icon::Fullscreen, "Full screen")
    };
    let keys = keymap::hint(cx.platform, &full);
    if widgets::menu_row(ui, icon, label, keys.as_deref(), false).clicked() {
        out.push(full);
        ui.close();
    }
    // The bar it sits on hides too, so it only ever turns it on.
    let keys = keymap::hint(cx.platform, &Sent::Ui(UiCommand::Chromeless));
    let label = "Distraction-free";
    let mut chromeless = false;
    if widgets::menu_switch_keyed(
        ui,
        Icon::Chromeless,
        label,
        &mut chromeless,
        keys.as_deref(),
    )
    .changed()
    {
        out.push(Sent::Ui(UiCommand::Chromeless));
        ui.close();
    }
    pin_row(ui, cx.pinned, out);
    widgets::menu_rule(ui);
    for tool in cx.folded {
        let keys = keymap::hint(cx.platform, &tool.command);
        if widgets::menu_row(ui, tool.icon, tool.label, keys.as_deref(), false).clicked() {
            out.push(tool.command.clone());
            ui.close();
        }
    }
    footer_actions(ui, cx.platform, out);
    widgets::menu_rule(ui);
    let version = format!("Mareader {}", env!("CARGO_PKG_VERSION"));
    widgets::menu_note(ui, &version);
}

/// Keeps the title bar shown; every route's menu offers it.
pub fn pin_row(ui: &mut Ui, pinned: bool, out: &mut Commands) {
    let label = "Keep title bar shown";
    let mut on = pinned;
    if widgets::menu_switch(ui, Icon::Pin, label, &mut on).changed() {
        out.push(Sent::Engine(Command::SetTitlebarPinned(on)));
    }
}

/// Frequent actions share a compact footer; shortcuts remain hints.
fn footer_actions(ui: &mut Ui, platform: Platform, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let rest = t
        .paper
        .lerp_to_gamma(t.ink, if t.dark { 0.055 } else { 0.04 });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let width = (ui.available_width() - 5.0) / 2.0;
        for (icon, text, name, command) in [
            (
                Icon::Settings,
                "Settings",
                "Settings",
                Sent::Ui(UiCommand::ShowSettings),
            ),
            (
                Icon::Keyboard,
                "Shortcuts",
                "Keyboard shortcuts",
                Sent::Ui(UiCommand::ShowShortcuts),
            ),
        ] {
            let keys = keymap::hint(platform, &command);
            let label = RichText::new(text).size(12.0);
            let response = ui
                .push_id(("view-menu-footer", name), |ui| {
                    let button =
                        egui::Button::image_and_text(anim::glyph(ui, icon, 15.0, name), label)
                            .right_text(
                                RichText::new(keys.as_deref().unwrap_or_default())
                                    .size(10.0)
                                    .color(t.muted),
                            )
                            .image_tint_follows_text_color(true)
                            .fill(rest)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(10)
                            .min_size(vec2(width, 34.0));
                    let response = ui.add(button);
                    let mark = footer_icon_rect(response.rect, ui.spacing().button_padding.x, 15.0);
                    anim::play_at(ui, &response, icon, name, mark);
                    response
                })
                .inner;
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
            if response.clicked() {
                out.push(command);
                ui.close();
            }
        }
    });
}

/// The leading image slot of a text button.
fn footer_icon_rect(button: Rect, pad: f32, size: f32) -> Rect {
    Rect::from_min_size(
        pos2(button.left() + pad, button.center().y - size * 0.5),
        egui::Vec2::splat(size),
    )
}

/// Auto-scroll's switch, dimmed in the paged modes, as in mareader.
fn auto_scroll_row(ui: &mut Ui, on: bool, can: bool, platform: Platform, out: &mut Commands) {
    let keys = keymap::hint(platform, &Sent::Engine(Command::AutoScroll));
    ui.add_enabled_ui(can, |ui| {
        let row = widgets::menu_row(ui, Icon::AutoScroll, "Auto-scroll", keys.as_deref(), on);
        if row.clicked() {
            out.push(Sent::Engine(Command::AutoScroll));
            ui.close();
        }
    });
}

/// Starts reading the book aloud, or stops it.
fn read_aloud_row(ui: &mut Ui, on: bool, platform: Platform, out: &mut Commands) {
    let keys = keymap::hint(platform, &Sent::Engine(Command::ToggleReadAloud));
    let (label, command) = match on {
        true => ("Stop reading aloud", Sent::Engine(Command::StopReadAloud)),
        false => ("Read aloud", Sent::Engine(Command::ToggleReadAloud)),
    };
    if widgets::menu_row(ui, Icon::ReadAloud, label, keys.as_deref(), on).clicked() {
        out.push(command);
        ui.close();
    }
}

/// The four view modes, each named under its icon; the menu stays open.
fn view_row(ui: &mut Ui, current: ViewMode, platform: Platform, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    // This menu stays open, so selection is the visible event.
    let _act = anim::announce(ui, Act::Click);
    let size = vec2(ui.available_width(), 46.0);
    let (band, _) = ui.allocate_exact_size(size, Sense::hover());
    let band = band.shrink2(vec2(4.0, 0.0));
    let slot = band.width() / ViewMode::ALL.len() as f32;
    for (i, mode) in ViewMode::ALL.into_iter().enumerate() {
        let icon = match mode {
            ViewMode::Single => Icon::SinglePage,
            ViewMode::Spread => Icon::Spread,
            ViewMode::Vertical => Icon::ScrollVertical,
            ViewMode::Horizontal => Icon::ScrollHorizontal,
        };
        let cell = Rect::from_min_size(
            pos2(band.left() + slot * i as f32, band.top()),
            vec2(slot, band.height()),
        )
        .shrink(1.0);
        let on = mode == current;
        let label = mode.label();
        let r = ui.interact(cell, ui.id().with(("view-mode", label)), Sense::click());
        let mark = Rect::from_center_size(cell.center() - vec2(0.0, 8.0), egui::Vec2::splat(16.0));
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
        let command = Sent::Engine(Command::View(mode));
        let keys = keymap::hint(platform, &command);
        widgets::tip(&r, label, keys.as_deref());
        let fill = if on {
            t.selected
        } else if r.hovered() {
            t.hover
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(cell, t.radius, fill);
        let ink = if on { t.accent } else { t.ink };
        anim::paint(ui, mark, icon, ink, label, &r);
        let at = cell.center() + vec2(0.0, 11.0);
        let font = egui::FontId::proportional(11.0);
        ui.painter()
            .text(at, egui::Align2::CENTER_CENTER, mode.short(), font, ink);
        if r.clicked() {
            out.push(command);
        }
    }
}

/// Zoom steps, then the fits; the menu stays open so presses chain.
fn zoom_row(ui: &mut Ui, zoom: ZoomState, platform: Platform, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    // Zoom stays open, so its controls answer clicks rather than menu reaches.
    let _act = anim::announce(ui, Act::Click);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.add_space(4.0);
        let steps = [
            (Step::Out, Icon::ZoomOut, "Zoom out"),
            (Step::In, Icon::ZoomIn, "Zoom in"),
        ];
        for (i, (step, icon, label)) in steps.into_iter().enumerate() {
            if i == 1 {
                let percent = RichText::new(zoom.percent()).size(t.text_small);
                ui.add_sized([52.0, 28.0], egui::Label::new(percent));
            }
            let command = Sent::Engine(Command::Zoom(ZoomCommand::Step(step)));
            let keys = keymap::hint(platform, &command);
            let r = ui.add_enabled_ui(zoom.can_step(step), |ui| {
                widgets::icon_button_keyed(ui, icon, label, false, keys.as_deref())
            });
            if r.inner.clicked() {
                out.push(command);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(4.0);
            let fits = [
                (Fit::Page, Icon::FitPage, "Fit page"),
                (Fit::Width, Icon::FitWidth, "Fit width"),
            ];
            for (fit, icon, label) in fits {
                let command = Sent::Engine(Command::Zoom(ZoomCommand::Fit(fit)));
                let keys = keymap::hint(platform, &command);
                let on = zoom.fit == Some(fit);
                if widgets::icon_button_keyed(ui, icon, label, on, keys.as_deref()).clicked() {
                    out.push(command);
                }
            }
        });
    });
}

/// Opens the cheat sheet in the middle of the window; both menus offer it.
pub fn shortcuts_row(ui: &mut Ui, platform: Platform, out: &mut Commands) {
    let keys = keymap::hint(platform, &Sent::Ui(UiCommand::ShowShortcuts));
    let label = "Keyboard shortcuts";
    if widgets::menu_row(ui, Icon::Keyboard, label, keys.as_deref(), false).clicked() {
        out.push(Sent::Ui(UiCommand::ShowShortcuts));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_footer_animation_stays_in_the_leading_image_slot() {
        let button = Rect::from_min_size(pos2(70.0, 20.0), vec2(126.0, 34.0));
        let mark = footer_icon_rect(button, 8.0, 15.0);
        assert_eq!(mark.min.x, 78.0, "the image starts after button padding");
        assert_eq!(mark.center().y, button.center().y);
        assert!(
            mark.center().x < button.center().x - 30.0,
            "the moving frame must not jump to the text button's centre"
        );
    }
}
