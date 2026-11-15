//! Command palette: fuzzy search over UI and engine commands (`Mod+K`).

use egui::{
    Align, Color32, Context, CornerRadius, Frame, Id, Key, Label, Layout, Margin, Modifiers,
    RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, vec2,
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
const LIST_HEIGHT: f32 = 340.0;

#[derive(Clone, Default)]
struct State {
    query: String,
    selected: usize,
    focus: bool,
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

    let response = Dialog::new(MODAL).show(ctx, |ui| {
        let max_w = (ctx.content_rect().width() - 32.0).clamp(280.0, WIDTH);
        ui.set_width(max_w);
        ui.set_max_width(max_w);
        dismiss |= header(ui, platform);
        ui.add_space(6.0);
        if filter_box(ui, &mut state) {
            state.selected = 0;
        }
        ui.add_space(8.0);
        ScrollArea::vertical()
            .max_height(LIST_HEIGHT)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                if let Some(clicked) = draw_list(ui, &entries, state.selected) {
                    run_index = Some(clicked);
                }
            });
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
    } else if up {
        state.selected = (state.selected + count - 1) % count;
    }
    enter.then_some(state.selected)
}

fn header(ui: &mut Ui, platform: Platform) -> bool {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add(Icon::Search.image(ui, 16.0).tint(t.ink));
        let title = RichText::new("Command Palette").size(t.text + 2.0);
        ui.label(title.color(t.ink).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let shut = widgets::small_icon_button(ui, Icon::Close, "Close palette").clicked();
            let show = Sent::Ui(crate::ui_command::UiCommand::ShowPalette);
            if let Some(keys) = keymap::hint(platform, &show) {
                ui.label(RichText::new(keys).size(t.text_small).color(t.muted));
            }
            shut
        })
        .inner
    })
    .inner
}

fn filter_box(ui: &mut Ui, state: &mut State) -> bool {
    let edit = TextEdit::singleline(&mut state.query)
        .hint_text("Type a command or category...")
        .desired_width(f32::INFINITY);
    let response = ui.add(edit);
    if state.focus {
        response.request_focus();
        state.focus = false;
    }
    response.changed()
}

fn draw_list(ui: &mut Ui, entries: &[Entry], selected: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    if entries.is_empty() {
        ui.label(RichText::new("No matching commands").color(t.muted));
        return None;
    }
    let mut picked = None;
    for (idx, entry) in entries.iter().enumerate() {
        let active = idx == selected;
        let fill = if active {
            t.accent.gamma_multiply(0.16)
        } else {
            Color32::TRANSPARENT
        };
        let frame = Frame::new()
            .fill(fill)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(8, 6));
        let row = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            draw_entry(ui, entry, &t);
        });
        let sense = if entry.disabled.is_none() {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(row.response.rect, Id::new((MODAL, entry.id)), sense);
        if let Some(reason) = entry.disabled {
            resp.on_hover_text(reason);
        } else if resp.clicked() {
            picked = Some(idx);
        }
    }
    picked
}

fn draw_entry(ui: &mut Ui, entry: &Entry, t: &Tokens) {
    let ink = entry.disabled.map_or(t.ink, |_| t.muted);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 0.0);
        ui.add(Label::new(RichText::new(entry.title).color(ink)).selectable(false));
        if let Some(menu) = entry.menu {
            let tag = RichText::new(menu).size(t.text_small).color(t.muted);
            ui.add(Label::new(tag).selectable(false));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(keys) = &entry.shortcut {
                let cap = RichText::new(keys).size(t.text_small).color(t.muted);
                ui.add(Label::new(cap).selectable(false));
            }
            if let Some(why) = entry.disabled {
                let note = RichText::new(why).size(t.text_small).color(t.muted);
                ui.add(Label::new(note).selectable(false));
            }
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
