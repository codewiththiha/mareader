//! Unified model input with keyboard suggestions and Choose trigger.

use egui::text::{CCursor, CCursorRange};
use egui::{
    CornerRadius, CursorIcon, Frame, Id, Key, Margin, Modifiers, Rect, Response, RichText,
    ScrollArea, Sense, Shadow, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_assist::{Board, Job, Load};
use mareader_engine::command::Command;
use mareader_settings::{ModelPreset, Provider};

use super::models::{FIELD_W, failure, missing, same};
use super::row;
use crate::icons::Icon;
use crate::input_field::InputResponse;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use crate::z_order::{self, Tier};

const SUGGEST_MAX: usize = 50;
const ROW_ITEM_H: f32 = 26.0;
const CHOOSE_BTN_H: f32 = 32.0;

/// Per-preset transient state for the unified model suggestions picker.
#[derive(Clone, Default)]
struct PickerState {
    open: bool,
    /// Show the full server list until the user edits the query text.
    browse_all: bool,
    /// Keyboard-highlighted index into the visible suggestion rows.
    selected: usize,
    /// Last frame's popup rect for click-outside detection.
    rect: Option<Rect>,
    /// Scroll the keyboard-selected row into view when arrows move it.
    moved: bool,
}

#[derive(Clone, Copy, Default)]
struct NavKeys {
    down: bool,
    up: bool,
    enter: bool,
    escape: bool,
}

/// Model field with refresh, unified Choose trigger, and autocomplete popup.
pub(super) fn model_row(
    ui: &mut Ui,
    p: &mut ModelPreset,
    board: Option<&Board>,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let server = p.provider;
    let input_id = Id::new(("ai-model-field", p.id.0));
    let picker_id = Id::new(("ai-model-suggest", p.id.0));
    let mut state: PickerState = ui.data(|d| d.get_temp(picker_id)).unwrap_or_default();
    state.moved = false;
    let had_focus = ui.memory(|m| m.has_focus(input_id));
    let keys = take_nav_keys(ui, state.open, had_focus);

    let raw_lost = board.is_some_and(|b| missing(&p.model, b.models(server)));
    let gone = format!(
        "{} isn't on the server now; choose another.",
        p.model.trim()
    );
    let show_lost = raw_lost && !state.open && !had_focus;
    let tip = if show_lost {
        gone.as_str()
    } else {
        "Blank uses the default"
    };
    let mut input_shown = None;
    let mut choose_resp = None;
    row(ui, "Model", tip, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some(b) = board {
            refresh(ui, server, b, out);
            choose_resp = Some(choose_button(ui, state.open, &t));
        }
        let hint = match server.default_model() {
            "" => "The server's first",
            m => m,
        };
        let mut input = widgets::input(&mut p.model)
            .id(input_id)
            .hint(hint)
            .width(FIELD_W)
            .compact();
        if show_lost {
            input = input.text_color(t.muted);
        }
        let shown = input.show(ui);
        if show_lost {
            widgets::tip(&shown.response, &gone, None);
        }
        input_shown = Some(shown);
    });
    if let (Some(b), Some(shown)) = (board, input_shown) {
        drive_picker(
            ui,
            p,
            b,
            (input_id, picker_id),
            (&shown, choose_resp.as_ref()),
            (&mut state, keys),
            out,
        );
    }
    let editing = state.open || had_focus || ui.memory(|m| m.has_focus(input_id));
    if raw_lost && !editing {
        ui.label(RichText::new(&gone).size(t.text_small).color(t.danger));
        ui.add_space(8.0);
    }
}

fn take_nav_keys(ui: &Ui, open: bool, focused: bool) -> NavKeys {
    let mut keys = NavKeys::default();
    if open {
        ui.input_mut(|i| {
            if focused {
                keys.down = i.consume_key(Modifiers::NONE, Key::ArrowDown);
                keys.up = i.consume_key(Modifiers::NONE, Key::ArrowUp);
                keys.enter = i.consume_key(Modifiers::NONE, Key::Enter);
            }
            keys.escape = i.consume_key(Modifiers::NONE, Key::Escape);
        });
    }
    keys
}

fn choose_button(ui: &mut Ui, active: bool, t: &Tokens) -> Response {
    let (fill, edge, ink) = if active {
        (t.accent_soft, t.accent, t.accent)
    } else {
        (t.ink.gamma_multiply(0.05), t.line, t.ink)
    };
    let btn = egui::Button::new(RichText::new("Choose").size(t.text_small).color(ink))
        .fill(fill)
        .stroke(Stroke::new(1.0, edge))
        .corner_radius(t.radius)
        .min_size(vec2(58.0, CHOOSE_BTN_H));
    let resp = ui.add(btn);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    resp
}

fn select_all(field: &Response, id: Id, text: &str) {
    field.request_focus();
    let mut edit = egui::TextEdit::load_state(&field.ctx, id).unwrap_or_default();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
    edit.cursor.set_char_range(Some(all));
    edit.store(&field.ctx, id);
    field.ctx.request_repaint();
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

/// Coordinates Choose click, focus/typing, keyboard selection, and popup draw.
fn drive_picker(
    ui: &Ui,
    p: &mut ModelPreset,
    board: &Board,
    (input_id, picker_id): (Id, Id),
    (shown, choose): (&InputResponse, Option<&Response>),
    (state, keys): (&mut PickerState, NavKeys),
    out: &mut Commands,
) {
    let server = p.provider;
    let models = board.models(server);
    let choose_clicked = choose.is_some_and(Response::clicked);
    let choose_rect = choose.map(|r| r.rect);
    if choose_clicked {
        if state.open && state.browse_all {
            close_picker(ui, input_id, state);
        } else {
            state.open = true;
            state.browse_all = true;
            state.selected = 0;
            state.moved = true;
            select_all(&shown.response, input_id, &p.model);
            if matches!(models, Load::Idle | Load::Failed(_)) {
                out.push(Sent::Engine(Command::Ai(Job::Models(server))));
            }
        }
    } else {
        let keys_ready = board.keys(server).ready().is_some_and(|r| !r.is_empty());
        let can_fetch = !server.needs_key() || keys_ready;
        if shown.response.gained_focus() {
            if *models == Load::Idle && can_fetch {
                out.push(Sent::Engine(Command::Ai(Job::Models(server))));
            }
            if !state.open {
                let exact = models
                    .ready()
                    .is_some_and(|list| list.iter().any(|m| same(&p.model, m)));
                state.open = true;
                state.browse_all = p.model.trim().is_empty() || exact;
                state.selected = 0;
                state.moved = true;
            }
        }
        if shown.response.changed() {
            state.open = true;
            state.browse_all = false;
            state.selected = 0;
            state.moved = true;
        }
    }
    let pressed = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed()));
    let latest = ui.input(|i| i.pointer.latest_pos());
    let in_popup = |pos: egui::Pos2| state.rect.is_some_and(|r| r.contains(pos));
    let over_popup = latest.is_some_and(in_popup) || pressed.is_some_and(in_popup);
    let in_control = |pos: egui::Pos2| {
        shown.rect.contains(pos) || choose_rect.is_some_and(|r| r.contains(pos)) || in_popup(pos)
    };
    let focused_now = shown.response.has_focus() || ui.memory(|m| m.has_focus(input_id));
    if keys.escape
        || pressed.is_some_and(|pos| !in_control(pos))
        || (!focused_now && !choose_clicked && !over_popup)
    {
        state.open = false;
        state.browse_all = false;
    }
    if state.open {
        let ids = (input_id, picker_id);
        show_picker_popup(ui, p, ids, shown.rect, models, (state, keys), out);
    } else {
        state.rect = None;
    }
    ui.data_mut(|d| d.insert_temp(picker_id, state.clone()));
}

fn close_picker(ui: &Ui, input_id: Id, state: &mut PickerState) {
    state.open = false;
    state.browse_all = false;
    state.rect = None;
    ui.ctx().memory_mut(|m| m.surrender_focus(input_id));
}

fn show_picker_popup(
    ui: &Ui,
    p: &mut ModelPreset,
    (input_id, picker_id): (Id, Id),
    anchor: Rect,
    models: &Load<Vec<String>>,
    (state, keys): (&mut PickerState, NavKeys),
    out: &mut Commands,
) {
    let list = match models {
        Load::Ready(list) if !list.is_empty() => list,
        _ => {
            state.rect = state
                .browse_all
                .then(|| draw_status_popup(ui, picker_id, anchor, p.provider, models, out));
            return;
        }
    };
    let items: Vec<&str> = if state.browse_all {
        list.iter().map(String::as_str).collect()
    } else {
        matching(list, &p.model)
    };
    if items.is_empty() {
        state.rect = None;
        return;
    }
    let count = items.len().min(SUGGEST_MAX);
    state.selected = state.selected.min(count.saturating_sub(1));
    if keys.down && count > 0 {
        state.selected = (state.selected + 1).min(count - 1);
        state.moved = true;
    }
    if keys.up {
        state.selected = state.selected.saturating_sub(1);
        state.moved = true;
    }
    if keys.enter {
        if let Some(&choice) = items.get(state.selected) {
            p.model = choice.to_owned();
        }
        close_picker(ui, input_id, state);
        return;
    }
    let (picked, rect) = draw_suggestions_popup(
        ui,
        picker_id,
        anchor,
        &p.model,
        (&items, state.selected, state.moved),
    );
    if let Some(choice) = picked {
        p.model = choice;
        close_picker(ui, input_id, state);
    } else {
        state.rect = Some(rect);
    }
}

fn popup_frame(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(4))
        .shadow(Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: t.shadow.gamma_multiply(0.55),
        })
}

fn draw_status_popup(
    ui: &Ui,
    open_id: Id,
    anchor: Rect,
    server: Provider,
    models: &Load<Vec<String>>,
    out: &mut Commands,
) -> Rect {
    let t = Tokens::get(ui.ctx());
    let width = anchor.width().max(FIELD_W);
    let area = z_order::area(ui.ctx(), Tier::ModalPopover, open_id.with("popup"))
        .fixed_pos(pos2(anchor.left(), anchor.bottom() + 4.0))
        .constrain(true)
        .show(ui.ctx(), |ui| {
            popup_frame(&t).show(ui, |ui| {
                ui.set_width(width - 8.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                match models {
                    Load::Idle | Load::Busy => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(12.0));
                            ui.label(RichText::new("Loading models…").color(t.muted));
                        });
                    }
                    Load::Failed(f) => {
                        failure(ui, f);
                        if ui.button("Try again").clicked() {
                            out.push(Sent::Engine(Command::Ai(Job::Models(server))));
                        }
                    }
                    Load::Ready(_) => {
                        ui.label(RichText::new("The server lists no models.").color(t.muted));
                    }
                }
            });
        });
    area.response.rect
}

fn draw_suggestions_popup(
    ui: &Ui,
    open_id: Id,
    anchor: Rect,
    model: &str,
    (matches, selected, moved): (&[&str], usize, bool),
) -> (Option<String>, Rect) {
    let t = Tokens::get(ui.ctx());
    let width = anchor.width().max(FIELD_W);
    let visible = matches.len().min(SUGGEST_MAX);
    let cap = (ui.ctx().content_rect().height() * 0.45).clamp(220.0, 360.0);
    let want_h = (visible as f32 * ROW_ITEM_H + 8.0).clamp(32.0, cap);
    let mut picked: Option<String> = None;
    let area = z_order::area(ui.ctx(), Tier::ModalPopover, open_id.with("popup"))
        .fixed_pos(pos2(anchor.left(), anchor.bottom() + 4.0))
        .constrain(true)
        .show(ui.ctx(), |ui| {
            popup_frame(&t).show(ui, |ui| {
                ui.set_width(width - 8.0);
                ScrollArea::vertical()
                    .id_salt(open_id.with("scroll"))
                    .max_height(cap)
                    .min_scrolled_height(want_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 1.0;
                        for (idx, &id) in matches.iter().take(SUGGEST_MAX).enumerate() {
                            let active = idx == selected;
                            let current = same(model, id);
                            let resp = suggestion_row(ui, id, active, current, &t);
                            if active && moved {
                                resp.scroll_to_me(None);
                            }
                            if resp.clicked() {
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

fn suggestion_row(ui: &mut Ui, id: &str, active: bool, current: bool, t: &Tokens) -> Response {
    let size = vec2(ui.available_width(), ROW_ITEM_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if active {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), t.accent_soft);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), t.hover);
    }
    let ink = if active { t.accent } else { t.ink };
    let font = egui::FontId::proportional(t.text_small);
    let right_pad = if current { 24.0 } else { 8.0 };
    let max_x = (rect.right() - right_pad).max(rect.left() + 20.0);
    let text_rect = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.top()),
        pos2(max_x, rect.bottom()),
    );
    ui.painter().with_clip_rect(text_rect).text(
        pos2(text_rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        id,
        font,
        ink,
    );
    if current {
        let at = pos2(rect.right() - 12.0, rect.center().y);
        let mark = Rect::from_center_size(at, vec2(12.0, 12.0));
        Icon::Check.paint(ui, mark, t.accent);
    }
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, active, id));
    resp
}
