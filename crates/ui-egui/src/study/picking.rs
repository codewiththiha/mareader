//! Picking several marks at once, to restyle or remove them together.

use egui::{Align2, Context, Frame, Id, Margin, Order, Rect, RichText, Stroke, vec2};
use mareader_library::{MarkId, Style};
use mareader_settings::HighlightSettings;
use mareader_workspace::PaneId;

use super::restyle;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use mareader_engine::command::Command;
use mareader_engine::study::Action;

/// The Style… popover's id, for shots to open it.
pub const STYLE_ID: &str = "study-picking-style";

/// Below the pane's top, clear of the title bar.
const DOWN: f32 = 56.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Picking {
    pub pane: PaneId,
    pub ids: Vec<MarkId>,
    /// The marks on screen last frame, and every mark of the book.
    pub page: Vec<MarkId>,
    pub all: Vec<MarkId>,
}

impl Picking {
    pub fn new(pane: PaneId, first: MarkId) -> Self {
        Self {
            pane,
            ids: vec![first],
            page: Vec::new(),
            all: Vec::new(),
        }
    }

    pub fn toggle(&mut self, id: MarkId) {
        match self.ids.iter().position(|m| *m == id) {
            Some(i) => {
                self.ids.remove(i);
            }
            None => self.ids.push(id),
        }
    }

    /// Adds `ids` not picked yet, in order.
    pub fn add(&mut self, ids: &[MarkId]) {
        for id in ids {
            if !self.ids.contains(id) {
                self.ids.push(*id);
            }
        }
    }
}

/// The bar over the pane; `looks` are the picked highlights'.
pub fn bar(
    ctx: &Context,
    picking: &Picking,
    (room, looks, pens): (Rect, &[(MarkId, Style)], &HighlightSettings),
    t: &Tokens,
    out: &mut Commands,
) {
    let n = picking.ids.len();
    let shown = egui::Area::new(Id::new("study-picking"))
        .order(Order::Foreground)
        .pivot(Align2::CENTER_TOP)
        .fixed_pos(room.center_top() + vec2(0.0, DOWN))
        .constrain_to(room)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{n} selected")).color(t.ink));
                        ui.separator();
                        let act = |out: &mut Commands, a| out.push(Sent::Engine(Command::Study(a)));
                        if ui.button("This page").clicked() {
                            act(out, Action::PickPage);
                        }
                        if ui.button("All").clicked() {
                            act(out, Action::PickAll);
                        }
                        style(ui, looks, pens, t, out);
                        let remove = RichText::new(format!("Remove ({n})")).color(t.danger);
                        let button = ui.add_enabled(n > 0, egui::Button::new(remove));
                        if button.clicked() {
                            act(out, Action::Remove(picking.ids.clone()));
                        }
                        if ui.button("Done").clicked() {
                            act(out, Action::DonePicking);
                        }
                    });
                });
        });
    crate::toast::keep_clear(ctx, shown.response.rect);
}

/// Style: the picked highlights' pens and colours, in a popover.
fn style(
    ui: &mut egui::Ui,
    looks: &[(MarkId, Style)],
    pens: &HighlightSettings,
    t: &Tokens,
    out: &mut Commands,
) {
    let button = ui.add_enabled(!looks.is_empty(), egui::Button::new("Style…"));
    let button = match looks.len() {
        0 => button.on_disabled_hover_text("Pick a highlight to restyle it"),
        _ => button,
    };
    egui::Popup::from_toggle_button_response(&button)
        .id(Id::new(STYLE_ID))
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_START)
        .gap(6.0)
        .show(|ui| {
            ui.set_width(232.0);
            let what = match looks.len() {
                1 => "Style of 1 highlight".to_owned(),
                n => format!("Style of {n} highlights"),
            };
            ui.label(RichText::new(what).size(t.text_small).color(t.muted));
            ui.add_space(4.0);
            restyle::group(ui, looks, pens, t, out);
            crate::widgets::close_on_key(ui);
        });
}
