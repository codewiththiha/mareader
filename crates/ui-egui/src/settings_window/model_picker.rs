//! Model chooser, autocomplete suggestions, provider address, and key entry.

use egui::{
    Align, CornerRadius, CursorIcon, Frame, Id, Key, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Shadow, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_assist::{Board, Job, Load};
use mareader_engine::command::Command;
use mareader_settings::{Endpoint, ModelPreset, PresetId, Provider};

use super::models::{FIELD_W, failure, missing, same};
use super::{ROW_H, divider, row};
use crate::icons::Icon;
use crate::input_field::InputResponse;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use crate::z_order::{self, Tier};

const SUGGEST_MAX: usize = 50;
const ROW_ITEM_H: f32 = 26.0;
const KEY_BTN_H: f32 = 32.0;
const KEY_BTN_W: f32 = 64.0;

/// Model field with refresh, expanded Choose menu, and autocomplete popup.
pub(super) fn model_row(
    ui: &mut Ui,
    p: &mut ModelPreset,
    board: Option<&Board>,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let server = p.provider;
    let lost = board.is_some_and(|b| missing(&p.model, b.models(server)));
    let gone = format!(
        "{} isn't on the server now; choose another.",
        p.model.trim()
    );
    let tip = if lost {
        gone.as_str()
    } else {
        "Blank uses the default"
    };
    let mut input_shown = None;
    row(ui, "Model", tip, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some(b) = board {
            refresh(ui, server, b, out);
            model_menu(ui, server, &mut p.model, b.models(server), out);
        }
        let hint = match server.default_model() {
            "" => "The server's first",
            m => m,
        };
        let input = widgets::input(&mut p.model)
            .id(Id::new(("ai-model-field", p.id.0)))
            .hint(hint)
            .width(FIELD_W);
        let mut input = input.compact();
        if lost {
            input = input.text_color(t.muted);
        }
        let shown = input.show(ui);
        if lost {
            widgets::tip(&shown.response, &gone, None);
        }
        input_shown = Some(shown);
    });
    if let (Some(b), Some(shown)) = (board, input_shown) {
        let can_fetch =
            !server.needs_key() || b.keys(server).ready().is_some_and(|rows| !rows.is_empty());
        if shown.response.gained_focus() && *b.models(server) == Load::Idle && can_fetch {
            out.push(Sent::Engine(Command::Ai(Job::Models(server))));
        }
        suggestions(ui, p.id, &mut p.model, &shown, b.models(server));
    }
    if lost {
        ui.label(RichText::new(&gone).size(t.text_small).color(t.danger));
        ui.add_space(8.0);
    }
}

/// Asks the server for its models again; a spinner while it answers.
fn refresh(ui: &mut Ui, p: Provider, board: &Board, out: &mut Commands) {
    if board.fetching(p) {
        let r = ui.add_sized(vec2(24.0, 24.0), egui::Spinner::new().size(12.0));
        widgets::name(&r, "Refreshing models");
    } else if widgets::small_icon_button(ui, Icon::Rescan, "Refresh models").clicked() {
        out.push(Sent::Engine(Command::Ai(Job::Models(p))));
    }
}

/// The server's models, fetched when the menu first opens.
fn model_menu(
    ui: &mut Ui,
    p: Provider,
    model: &mut String,
    models: &Load<Vec<String>>,
    out: &mut Commands,
) {
    let menu = ui.menu_button("Choose", |ui| {
        ui.set_min_width(FIELD_W);
        ui.set_max_width(FIELD_W + 40.0);
        match models {
            Load::Idle | Load::Busy => {
                ui.label("Loading models…");
            }
            Load::Failed(f) => {
                failure(ui, f);
                if ui.button("Try again").clicked() {
                    out.push(Sent::Engine(Command::Ai(Job::Models(p))));
                }
            }
            Load::Ready(list) if list.is_empty() => {
                ui.label("The server lists no models.");
            }
            Load::Ready(list) => {
                let cap = (ui.ctx().content_rect().height() * 0.45).clamp(220.0, 360.0);
                let want = (list.len() as f32 * ROW_ITEM_H + 8.0).clamp(32.0, cap);
                ScrollArea::vertical()
                    .id_salt(("ai-model-menu", p.scope()))
                    .max_height(cap)
                    .min_scrolled_height(want)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(FIELD_W - 12.0);
                        for id in list {
                            if ui.selectable_label(same(model, id), id).clicked() {
                                model.clone_from(id);
                                ui.close();
                            }
                        }
                    });
            }
        }
    });
    if menu.response.clicked() && matches!(models, Load::Idle | Load::Failed(_)) {
        out.push(Sent::Engine(Command::Ai(Job::Models(p))));
    }
}

/// Prefix matches first, then substring matches, hiding sole exact matches.
pub(super) fn matching<'a>(list: &'a [String], query: &str) -> Vec<&'a str> {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return list.iter().map(String::as_str).collect();
    }
    let mut prefix = Vec::new();
    let mut contains = Vec::new();
    for id in list {
        let lower = id.to_ascii_lowercase();
        if lower.starts_with(&q) {
            prefix.push(id.as_str());
        } else if lower.contains(&q) {
            contains.push(id.as_str());
        }
    }
    prefix.extend(contains);
    if prefix.len() == 1 && prefix.first().is_some_and(|m| same(query, m)) {
        prefix.clear();
    }
    prefix
}

/// Search-style suggestion popup below the model field once models load.
fn suggestions(
    ui: &Ui,
    preset_id: PresetId,
    model: &mut String,
    shown: &InputResponse,
    models: &Load<Vec<String>>,
) {
    let Load::Ready(list) = models else {
        return;
    };
    if list.is_empty() {
        return;
    }
    let open_id = Id::new(("ai-model-suggest", preset_id.0));
    let rect_id = open_id.with("rect");
    let prev_rect: Option<Rect> = ui.data(|d| d.get_temp(rect_id));
    let mut open: bool = ui.data(|d| d.get_temp(open_id)).unwrap_or(false);
    if shown.response.gained_focus() || shown.response.changed() {
        open = true;
    }
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    let enter = shown.response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    let pressed = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
    let latest = ui.input(|i| i.pointer.latest_pos());
    let in_popup = |pos: egui::Pos2| prev_rect.is_some_and(|r| r.contains(pos));
    let over_popup = latest.is_some_and(in_popup) || pressed.is_some_and(in_popup);
    if escape
        || enter
        || pressed.is_some_and(|p| !shown.rect.contains(p) && !in_popup(p))
        || (!shown.response.has_focus() && !over_popup)
    {
        open = false;
    }
    let matches = matching(list, model);
    if !open || matches.is_empty() {
        ui.data_mut(|d| {
            d.insert_temp(open_id, open);
            d.remove::<Rect>(rect_id);
        });
        return;
    }
    let (picked, rect) = draw_suggestions_popup(ui, open_id, shown.rect, model, &matches);
    if let Some(choice) = picked {
        *model = choice;
        open = false;
    }
    ui.data_mut(|d| {
        d.insert_temp(open_id, open);
        d.insert_temp(rect_id, rect);
    });
}

fn draw_suggestions_popup(
    ui: &Ui,
    open_id: Id,
    anchor: Rect,
    model: &str,
    matches: &[&str],
) -> (Option<String>, Rect) {
    let t = Tokens::get(ui.ctx());
    let width = anchor.width().max(FIELD_W);
    let visible = matches.len().min(SUGGEST_MAX);
    let want_h = (visible as f32 * ROW_ITEM_H + 8.0).clamp(32.0, 220.0);
    let mut picked: Option<String> = None;
    let area = z_order::area(ui.ctx(), Tier::ModalPopover, open_id.with("popup"))
        .fixed_pos(pos2(anchor.left(), anchor.bottom() + 4.0))
        .constrain(true)
        .show(ui.ctx(), |ui| {
            let frame = Frame::new()
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(4))
                .shadow(Shadow {
                    offset: [0, 4],
                    blur: 14,
                    spread: 0,
                    color: t.shadow.gamma_multiply(0.55),
                });
            frame.show(ui, |ui| {
                ui.set_width(width - 8.0);
                ScrollArea::vertical()
                    .id_salt(open_id.with("scroll"))
                    .max_height(220.0)
                    .min_scrolled_height(want_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for &id in matches.iter().take(SUGGEST_MAX) {
                            if suggestion_row(ui, id, same(model, id), &t).clicked() {
                                picked = Some(id.to_owned());
                            }
                        }
                        if matches.len() > SUGGEST_MAX {
                            let more = matches.len() - SUGGEST_MAX;
                            ui.label(
                                RichText::new(format!("+{more} more — keep typing to narrow"))
                                    .size(t.text_small)
                                    .color(t.muted),
                            );
                        }
                    });
            });
        });
    (picked, area.response.rect)
}

fn suggestion_row(ui: &mut Ui, id: &str, selected: bool, t: &Tokens) -> egui::Response {
    let size = vec2(ui.available_width(), ROW_ITEM_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), t.hover);
    } else if selected {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), t.accent_soft);
    }
    let ink = if selected { t.accent } else { t.ink };
    let font = egui::FontId::proportional(t.text_small);
    let text_rect = rect.shrink2(vec2(8.0, 0.0));
    ui.painter().with_clip_rect(text_rect).text(
        pos2(text_rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        id,
        font,
        ink,
    );
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, id));
    resp
}

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
        let r = ui.checkbox(&mut use_default, "");
        widgets::name(&r, "Use default address");
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
    let gap = 8.0;
    let row_w = ui.available_width();
    let field_w = (row_w - KEY_BTN_W - gap).max(160.0);
    ui.allocate_ui_with_layout(
        vec2(row_w, ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = gap;
            let field = widgets::input(&mut text)
                .id(id.with("field"))
                .icon(Icon::Key)
                .password(true)
                .hint("Paste an API key")
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
            let button = egui::Button::new(RichText::new("Add").color(ink))
                .fill(fill)
                .stroke(if ready {
                    Stroke::NONE
                } else {
                    Stroke::new(1.0, t.line)
                })
                .corner_radius(t.radius)
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
