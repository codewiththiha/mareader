//! Command palette: fuzzy search over UI and engine commands (`Mod+K`).

use egui::{
    Align, Color32, Context, CornerRadius, CursorIcon, Frame, Id, Key, Label, Layout, Margin,
    Modifiers, RichText, ScrollArea, Sense, Shape, Stroke, Ui, vec2,
};
use mareader_engine::command_spec::{CommandSpec, SPECS};
use mareader_engine::session::Session;
use serde_json::{Map, Value};

use crate::icons::Icon;
use crate::keymap;
use crate::modal::Dialog;
use crate::platform::Platform;
use crate::theme::Tokens;
use crate::ui_command::{Allows, Commands, Sent, UI_COMMANDS, UiCommandMeta, parse_ui_command};
use crate::widgets;

const OPEN: &str = "command-palette-open";
const MODAL: &str = "command-palette";
const STATE: &str = "command-palette-state";
const WIDTH: f32 = 560.0;
const ROW_H: f32 = 36.0;
const LIST_MIN_H: f32 = 240.0;
const LIST_MAX_H: f32 = 440.0;

#[derive(Clone, Default)]
struct State {
    query: String,
    selected: usize,
    focus: bool,
    scroll_to: bool,
}

enum Action {
    Ui(&'static UiCommandMeta),
    Engine(&'static CommandSpec),
}

struct Entry {
    id: &'static str,
    title: &'static str,
    menu: Option<&'static str>,
    shortcut: Option<String>,
    disabled: Option<&'static str>,
    action: Action,
}

/// Whether the command palette modal is currently open.
pub fn is_open(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(Id::new(OPEN)))
        .unwrap_or(false)
}

/// Opens the command palette, or closes it when already open.
pub fn toggle(ctx: &Context) {
    if is_open(ctx) {
        close(ctx);
    } else {
        open(ctx);
    }
}

/// Opens the command palette and resets the filter query.
pub fn open(ctx: &Context) {
    ctx.data_mut(|d| {
        d.insert_temp(Id::new(OPEN), true);
        d.insert_temp(
            Id::new(STATE),
            State {
                query: String::new(),
                selected: 0,
                focus: true,
                scroll_to: false,
            },
        );
    });
}

/// Closes the command palette.
pub fn close(ctx: &Context) {
    ctx.data_mut(|d| {
        d.insert_temp(Id::new(OPEN), false);
        d.remove::<State>(Id::new(STATE));
    });
}

/// Draws the command palette modal while open and queues any chosen command.
pub fn show(ctx: &Context, session: &Session, platform: Platform, out: &mut Commands) {
    if !is_open(ctx) {
        return;
    }
    let state_id = Id::new(STATE);
    let mut state: State = ctx
        .data(|d| d.get_temp::<State>(state_id))
        .unwrap_or_default();
    let entries = matching_entries(session, platform, &state.query);
    if entries.is_empty() {
        state.selected = 0;
    } else if state.selected >= entries.len() {
        state.selected = entries.len() - 1;
    }

    let mut run_index = step_selection(ctx, &mut state, entries.len());
    let mut dismiss = false;
    let max_list_h = (ctx.content_rect().height() - 200.0).clamp(LIST_MIN_H, LIST_MAX_H);
    let list_h = if entries.is_empty() {
        72.0
    } else {
        (entries.len() as f32 * ROW_H + 8.0).min(max_list_h)
    };

    let response = Dialog::new(MODAL).show(ctx, |ui| {
        let max_w = (ctx.content_rect().width() - 32.0).clamp(280.0, WIDTH);
        ui.set_width(max_w);
        ui.set_max_width(max_w);
        dismiss |= header(ui, platform);
        ui.add_space(10.0);
        if filter_box(ui, &mut state) {
            state.selected = 0;
            state.scroll_to = true;
        }
        ui.add_space(10.0);
        ScrollArea::vertical()
            .id_salt((MODAL, "list"))
            .max_height(max_list_h)
            .min_scrolled_height(list_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                if let Some(clicked) = draw_list(ui, &entries, state.selected, state.scroll_to) {
                    run_index = Some(clicked);
                }
            });
        state.scroll_to = false;
        footer(ui, entries.len());
    });

    if let Some(idx) = run_index
        && let Some(entry) = entries.get(idx)
        && entry.disabled.is_none()
        && let Some(sent) = build_command(session, &entry.action)
    {
        out.push(sent);
        dismiss = true;
    }

    if dismiss || response.should_close() {
        close(ctx);
    } else {
        ctx.data_mut(|d| d.insert_temp(state_id, state));
    }
}

fn step_selection(ctx: &Context, state: &mut State, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let down = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowDown));
    let up = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp));
    let enter = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
    if down {
        state.selected = (state.selected + 1) % count;
        state.scroll_to = true;
    } else if up {
        state.selected = (state.selected + count - 1) % count;
        state.scroll_to = true;
    }
    enter.then_some(state.selected)
}

fn header(ui: &mut Ui, platform: Platform) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        let title = RichText::new("Command Palette").size(t.text + 2.0);
        ui.label(title.color(t.ink).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let shut = widgets::small_icon_button(ui, Icon::Close, "Close palette").clicked();
            let show = Sent::Ui(crate::ui_command::UiCommand::ShowPalette);
            if let Some(keys) = keymap::hint(platform, &show) {
                keycap(ui, &keys, &t);
            }
            shut
        })
        .inner
    })
    .inner
}

fn filter_box(ui: &mut Ui, state: &mut State) -> bool {
    let field = widgets::input(&mut state.query)
        .id(Id::new((MODAL, "filter")))
        .icon(Icon::Search)
        .hint("Type a command or category...")
        .label("Command palette filter")
        .clearable(true)
        .large()
        .show(ui);
    if state.focus {
        field.response.request_focus();
        state.focus = false;
    }
    field.changed()
}

fn draw_list(ui: &mut Ui, entries: &[Entry], selected: usize, scroll_to: bool) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    if entries.is_empty() {
        ui.add_space(16.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("No matching commands").color(t.muted));
        });
        return None;
    }
    let mut picked = None;
    for (idx, entry) in entries.iter().enumerate() {
        let active = idx == selected;
        let bg = ui.painter().add(Shape::Noop);
        let frame = Frame::new()
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(10, 7));
        let row = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            draw_entry(ui, entry, active, &t);
        });
        let rect = row.response.rect;
        if active && scroll_to {
            row.response.scroll_to_me(None);
        }
        let sense = if entry.disabled.is_none() {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(rect, Id::new((MODAL, entry.id)), sense);
        let fill = if active {
            t.accent.gamma_multiply(0.14)
        } else if resp.hovered() && entry.disabled.is_none() {
            t.ink.gamma_multiply(0.05)
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            ui.painter()
                .set(bg, Shape::rect_filled(rect, CornerRadius::same(8), fill));
        }
        if let Some(reason) = entry.disabled {
            resp.on_hover_text(reason);
        } else {
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.clicked() {
                picked = Some(idx);
            }
        }
    }
    picked
}

fn draw_entry(ui: &mut Ui, entry: &Entry, active: bool, t: &Tokens) {
    let ink = entry.disabled.map_or(t.ink, |_| t.muted);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
        let mut title = RichText::new(entry.title).color(ink);
        if active && entry.disabled.is_none() {
            title = title.strong();
        }
        ui.add(Label::new(title).selectable(false));
        if let Some(menu) = entry.menu {
            let tag = RichText::new(menu).size(t.text_small).color(t.muted);
            ui.add(Label::new(tag).selectable(false));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(keys) = &entry.shortcut {
                keycap(ui, keys, t);
            }
            if let Some(why) = entry.disabled {
                let note = RichText::new(why).size(t.text_small).color(t.muted);
                ui.add(Label::new(note).selectable(false));
            }
        });
    });
}

fn keycap(ui: &mut Ui, keys: &str, t: &Tokens) {
    Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(0.5, t.line))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            let cap = RichText::new(keys).size(t.text_small - 0.5).color(t.muted);
            ui.add(Label::new(cap).selectable(false));
        });
}

fn footer(ui: &mut Ui, count: usize) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(6.0);
    let (_, sep) = ui.allocate_space(vec2(ui.available_width(), 1.0));
    ui.painter().hline(
        sep.x_range(),
        sep.center().y,
        Stroke::new(0.5, t.line.gamma_multiply(0.75)),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let noun = if count == 1 { "command" } else { "commands" };
        let summary = format!("{count} {noun}");
        ui.label(RichText::new(summary).size(t.text_small).color(t.muted));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let guide = RichText::new("↑↓ Navigate  ·  ↵ Run  ·  Esc Close")
                .size(t.text_small)
                .color(t.muted.gamma_multiply(0.8));
            ui.label(guide);
        });
    });
}

fn matching_entries(session: &Session, platform: Platform, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let route = session.route();
    let mut entries = Vec::new();

    for meta in UI_COMMANDS {
        if meta.id == "ui.palette" {
            continue;
        }
        if !matches_query(&q, meta.title, meta.id, meta.menu, meta.label) {
            continue;
        }
        let disabled = match parse_ui_command(meta.id) {
            Ok(cmd) => (!route.allows(&Sent::Ui(cmd))).then_some("Unavailable on this view"),
            Err(_) => Some("Unavailable"),
        };
        let shortcut = parse_ui_command(meta.id)
            .ok()
            .and_then(|cmd| keymap::hint(platform, &Sent::Ui(cmd)))
            .or_else(|| meta.shortcut.map(str::to_owned));
        entries.push(Entry {
            id: meta.id,
            title: meta.title,
            menu: meta.menu,
            shortcut,
            disabled,
            action: Action::Ui(meta),
        });
    }

    for spec in SPECS {
        if !spec.params.is_empty() && !spec.params.iter().all(|(_, k)| k.ends_with('?')) {
            continue;
        }
        if !matches_query(&q, spec.title, spec.id, spec.menu, spec.label) {
            continue;
        }
        let disabled = (spec.can_run)(session).err();
        entries.push(Entry {
            id: spec.id,
            title: spec.title,
            menu: spec.menu,
            shortcut: spec.shortcut.map(str::to_owned),
            disabled,
            action: Action::Engine(spec),
        });
    }

    entries.sort_by_key(|e| (e.disabled.is_some(), e.title));
    entries
}

fn matches_query(q: &str, title: &str, id: &str, menu: Option<&str>, label: &str) -> bool {
    if q.is_empty() {
        return true;
    }
    title.to_lowercase().contains(q)
        || id.to_lowercase().contains(q)
        || menu.is_some_and(|m| m.to_lowercase().contains(q))
        || label.to_lowercase().contains(q)
}

fn build_command(session: &Session, action: &Action) -> Option<Sent> {
    match action {
        Action::Ui(meta) => parse_ui_command(meta.id).ok().map(Sent::Ui),
        Action::Engine(spec) => (spec.parse)(session, &Value::Object(Map::new()))
            .ok()
            .map(Sent::Engine),
    }
}
