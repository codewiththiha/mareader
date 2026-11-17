//! Provider address toggle and API key input row.

use egui::{Align, CursorIcon, Id, Key, Layout, RichText, Stroke, Ui, vec2};
use mareader_assist::Job;
use mareader_engine::command::Command;
use mareader_settings::{Endpoint, Provider};

use super::models::FIELD_W;
use super::{ROW_H, divider, row};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

const KEY_BTN_H: f32 = 32.0;
const KEY_BTN_W: f32 = 64.0;
const KEY_GAP: f32 = 10.0;

/// Provider URL: default-checked toggle for built-in servers, raw for custom.
pub(super) fn provider_address(ui: &mut Ui, p: Provider, e: &mut Endpoint) {
    let default_url = p.default_url();
    if default_url.is_empty() {
        address_row(ui, p, e);
        return;
    }
    let custom_id = Id::new(("ai-custom-url", p.scope()));
    let custom_open: bool = ui
        .data(|d| d.get_temp(custom_id))
        .unwrap_or_else(|| !e.url.trim().is_empty());
    let mut use_default = !custom_open;
    let tip = format!("Uses {default_url}");
    row(ui, "Use default address", &tip, |ui| {
        let r = widgets::checkbox_labeled(ui, &mut use_default, "Use default address");
        if r.changed() {
            if use_default {
                e.url.clear();
            }
            ui.data_mut(|d| d.insert_temp(custom_id, !use_default));
        }
    });
    if !use_default {
        divider(ui);
        address_row(ui, p, e);
    }
}

fn address_row(ui: &mut Ui, p: Provider, e: &mut Endpoint) {
    let tip = if p.needs_key() {
        "Blank uses the provider's own; change it only for a proxy"
    } else {
        "Up to and including /v1"
    };
    let hint = match p.default_url() {
        "" => "https://host/v1/",
        url => url,
    };
    row(ui, "Address", tip, |ui| {
        widgets::input(&mut e.url)
            .hint(hint)
            .width(FIELD_W)
            .compact()
            .show(ui);
    });
}

/// A field for a new key with a cleanly aligned Add button.
pub(super) fn add_key(ui: &mut Ui, p: Provider, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new(("ai-new-key", p.scope()));
    let mut text: String = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    let row_w = ui.available_width();
    let field_w = (row_w - KEY_BTN_W - KEY_GAP).max(160.0);
    ui.allocate_ui_with_layout(
        vec2(row_w, ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(ROW_H);
            ui.spacing_mut().item_spacing.x = KEY_GAP;
            let field = widgets::input(&mut text)
                .id(id.with("field"))
                .icon(Icon::Key)
                .password(true)
                .hint("Paste an API key")
                .label("Paste an API key")
                .width(field_w)
                .compact()
                .show(ui)
                .response;
            let ready = !text.trim().is_empty();
            let entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let fill = if ready {
                t.accent
            } else {
                t.ink.gamma_multiply(0.08)
            };
            let ink = if ready {
                crate::theme::ink_on(t.accent)
            } else {
                t.muted
            };
            let stroke = if ready {
                Stroke::NONE
            } else {
                Stroke::new(1.0, t.line)
            };
            let button = egui::Button::new(RichText::new("Add").color(ink))
                .fill(fill)
                .stroke(stroke)
                .corner_radius(t.radius + 1.0)
                .min_size(vec2(KEY_BTN_W, KEY_BTN_H));
            let add = ui.add_enabled(ready, button);
            if ready && add.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if (add.clicked() || entered) && ready {
                out.push(Sent::Engine(Command::Ai(Job::AddKey(
                    p,
                    std::mem::take(&mut text),
                ))));
            }
        },
    );
    ui.data_mut(|d| d.insert_temp(id, text));
}

#[cfg(test)]
mod tests {
    use egui::Rect;
    use egui_kittest::Harness;

    use super::*;

    #[test]
    fn add_key_keeps_margin_and_aligns_with_field() {
        let mut h = Harness::new_ui_state(
            |ui, state: &mut (String, Option<(Rect, Rect)>)| {
                let row_w = 400.0;
                let field_w = row_w - KEY_BTN_W - KEY_GAP;
                ui.allocate_ui_with_layout(
                    vec2(row_w, ROW_H),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_height(ROW_H);
                        ui.spacing_mut().item_spacing.x = KEY_GAP;
                        let shown = widgets::input(&mut state.0)
                            .icon(Icon::Key)
                            .width(field_w)
                            .compact()
                            .show(ui);
                        let size = vec2(KEY_BTN_W, KEY_BTN_H);
                        let btn = egui::Button::new("Add").min_size(size);
                        let add = ui.add(btn);
                        state.1 = Some((shown.rect, add.rect));
                    },
                );
            },
            (String::new(), None),
        );
        h.step();
        let (field, add) = h.state().1.expect("measured rects");
        let gap = add.left() - field.right();
        assert!((gap - KEY_GAP).abs() < 0.5, "gap={gap}");
        assert!((add.center().y - field.center().y).abs() < 0.5);
        assert!((add.height() - field.height()).abs() < 0.5);
    }
}
