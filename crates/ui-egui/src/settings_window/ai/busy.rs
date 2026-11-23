//! The AI tab's busy-model sections, drawn with egui: the backup models every
//! task tries after its own, and which tasks go first in a model's line.

use egui::{Align, Align2, CursorIcon, FontId, Layout, Rect, Response, Sense, Ui, pos2, vec2};
use mareader_settings::{AiSettings, Backups, Class, ModelPreset, PresetId, TaskId};

use super::super::models::badge;
use super::super::{ROW_H, divider, group, row, section_help};
use super::PICK_W;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;
use crate::z_order::{self, Tier};

/// A backup's row: its name over its small print.
const ITEM_H: f32 = 52.0;
/// From one backup's top to the next: a row and its divider.
const PITCH: f32 = ITEM_H + 1.0;
/// Room on a backup's right for its three buttons.
const TOOLS_W: f32 = 84.0;
/// The class picker's height: roomier than the Appearance menu's compact
/// track, still inside a settings row.
const CLASS_H: f32 = 30.0;
/// What the backups' "?" says.
const BACKUPS_HELP: [(&str, &str); 4] = [
    (
        "",
        "When a task's model is down, refuses its keys, or is overloaded or too slow, \
         the task tries these, top first.",
    ),
    ("", "A task skips its own model in this list."),
    ("Reorder", "Drag a row by its grip, or use the arrows."),
    (
        "Automatic",
        "Until you change it, the list is your first 3 models, oldest first, and fills \
         itself. After that, new models wait in Add a backup.",
    ),
];
/// What Who goes first's "?" says.
const PRIORITY_HELP: [(&str, &str); 4] = [
    (
        "",
        "This only matters when a model is busy: tasks waiting for it go in this order. \
         A running task is never stopped, and tasks on different models never wait \
         for each other.",
    ),
    (
        "Right away",
        "You're watching for the answer. Goes first, and tries a backup after 15 \
         seconds in line.",
    ),
    ("Normal", "You asked for it; a short wait is fine."),
    (
        "Background",
        "No one is waiting. Goes last and leaves room for your other tasks.",
    ),
];
/// The grip's dots, from its middle.
const GRIP: [(f32, f32); 6] = [
    (-2.5, -5.0),
    (2.5, -5.0),
    (-2.5, 0.0),
    (2.5, 0.0),
    (-2.5, 5.0),
    (2.5, 5.0),
];

/// Both sections, once a model is saved; backups need two.
pub(super) fn show(ui: &mut Ui, ai: &mut AiSettings) {
    if ai.presets.len() >= 2 {
        backups(ui, ai);
    }
    if !ai.presets.is_empty() {
        priority(ui, ai);
    }
}

/// What a backup's buttons ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Move {
    Up,
    Down,
    Out,
}

/// The ordered list every task falls back on, with a way to add more.
fn backups(ui: &mut Ui, ai: &mut AiSettings) {
    let mut ids = ai.tasks.backups.ids(&ai.presets);
    let how = fold::title(ui, ids.len(), &BACKUPS_HELP);
    let names: Vec<String> = ids
        .iter()
        .filter_map(|id| ai.preset(*id))
        .map(ModelPreset::title)
        .collect();
    let mut changed = false;
    group(ui, |ui| {
        fold::body(ui, how, &names, |ui| {
            changed |= list(ui, ai, &mut ids);
            changed |= add(ui, &ai.presets, &mut ids);
        });
    });
    if changed {
        ai.tasks.backups.arrange(ids);
    }
    if how >= 1.0 {
        footer(ui, &mut ai.tasks.backups);
    }
}

/// The backups, top first, each draggable and with its buttons; true when
/// the order changed.
fn list(ui: &mut Ui, ai: &AiSettings, ids: &mut Vec<PresetId>) -> bool {
    let shown: Vec<&ModelPreset> = ids.iter().filter_map(|id| ai.preset(*id)).collect();
    if shown.is_empty() {
        empty(ui);
        return false;
    }
    let (n, top) = (shown.len(), ui.cursor().top());
    let pointer = ui.ctx().pointer_latest_pos();
    let from = carried(ui, &shown);
    let carry = from.zip(pointer).map(|(from, at)| Carry {
        from,
        to: landing(top, n, from, at.y),
    });
    let lines: Vec<String> = shown.iter().map(|p| line(ai, p)).collect();
    let (mut asked, mut dropped) = (None, None);
    for (i, (p, line)) in shown.iter().zip(&lines).enumerate() {
        let (r, by) = item(ui, (p, line), (i, n), carry);
        asked = by.map(|by| (i, by)).or(asked);
        if r.drag_stopped() {
            dropped = Some(i);
        }
        divider(ui);
    }
    if let (Some(c), Some(at)) = (carry, pointer) {
        let carried = shown.get(c.from).zip(lines.get(c.from));
        if let Some((p, line)) = carried {
            // The copy stays over the list, however far the pointer goes.
            let last = top + (n - 1) as f32 * PITCH;
            let y = (at.y - ITEM_H / 2.0).clamp(top, last);
            ghost(ui, (p, line), c.to, y);
        }
        ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    }
    match (asked, carry.filter(|c| dropped == Some(c.from))) {
        (Some((i, by)), _) => moved(ids, i, by),
        (None, Some(c)) => reorder(ids, c.from, c.to),
        (None, None) => false,
    }
}

/// The backup being carried, if any. A press alone carries nothing, so a
/// button on a row still clicks; the drop's own frame still carries.
fn carried(ui: &Ui, shown: &[&ModelPreset]) -> Option<usize> {
    let key = ui.id().with("ai-backup-carrying");
    let was = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let moving = ui.input(|i| i.pointer.is_decidedly_dragging());
    let stopped = was.then(|| ui.ctx().drag_stopped_id()).flatten();
    let id = match moving {
        true => ui.ctx().dragged_id().or(stopped),
        false => stopped,
    };
    let from = shown.iter().position(|p| Some(row_id(ui, p.id)) == id);
    ui.data_mut(|d| d.insert_temp(key, moving && from.is_some()));
    from
}

/// A backup being dragged: its row, and the row it would land in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Carry {
    from: usize,
    to: usize,
}

impl Carry {
    /// Where the `i`th row shows while this one is carried.
    fn place(self, i: usize) -> usize {
        let Carry { from, to } = self;
        match i {
            _ if i == from => to,
            _ if from < i && i <= to => i - 1,
            _ if to <= i && i < from => i + 1,
            _ => i,
        }
    }
}

/// A backup's row id, the same each frame wherever it sits.
fn row_id(ui: &Ui, id: PresetId) -> egui::Id {
    ui.id().with(("ai-backup", id))
}

/// The row a backup carried from `from` lands in when dropped at height
/// `y`, in a list of `n` from `top`.
fn landing(top: f32, n: usize, from: usize, y: f32) -> usize {
    let mid = |i: usize| top + i as f32 * PITCH + ITEM_H / 2.0;
    let above = (0..n).filter(|i| mid(*i) < y).count();
    let to = if above > from { above - 1 } else { above };
    to.min(n.saturating_sub(1))
}

/// A backup's small print: the tasks that skip it, else what it runs.
fn line(ai: &AiSettings, p: &ModelPreset) -> String {
    let first = |t: &&TaskId| ai.model(**t).is_some_and(|m| m.id == p.id);
    let skip: Vec<&str> = TaskId::ALL
        .iter()
        .filter(first)
        .map(|t| t.spec().label)
        .collect();
    match skip.as_slice() {
        [] if p.title() == ModelPreset::suggested_name(p.provider, &p.model) => {
            "Backs up every task".to_owned()
        }
        [] => format!("{} · {}", p.provider.label(), p.model()),
        [one] => format!("Skipped by {one} (its own model)"),
        [one, two] => format!("Skipped by {one} and {two} (their own model)"),
        [one, rest @ ..] => format!("Skipped by {one} and {} more (their own model)", rest.len()),
    }
}

/// The `i`th of `n` backups' row, `p` over its small print; its drag
/// handle, and what its buttons asked. While one is carried the others
/// slide aside to make room where it would land.
fn item(
    ui: &mut Ui,
    (p, line): (&ModelPreset, &str),
    (i, n): (usize, usize),
    carry: Option<Carry>,
) -> (Response, Option<Move>) {
    let id = row_id(ui, p.id);
    let size = vec2(ui.available_width(), ITEM_H);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let r = ui.interact(rect, id, Sense::drag());
    let title = p.title();
    let spoken = format!("Backup {}: {title}", i + 1);
    let kind = egui::WidgetType::Other;
    r.widget_info(|| egui::WidgetInfo::labeled(kind, true, &spoken));
    let place = carry.map_or(i, |c| c.place(i));
    let slot = ui
        .ctx()
        .animate_value_with_time(id.with("slot"), place as f32, 0.12);
    let shown = rect.translate(vec2(0.0, (slot - i as f32) * PITCH));
    if carry.is_some_and(|c| c.from == i) {
        hole(ui, shown);
        return (r, None);
    }
    face(ui, shown, (p, line), place, r.hovered() && carry.is_none());
    if carry.is_some() {
        return (r, None);
    }
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    let tools = Rect::from_min_max(pos2(rect.right() - TOOLS_W, rect.top()), rect.max);
    let layout = Layout::right_to_left(Align::Center);
    let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(tools).layout(layout));
    ui.spacing_mut().item_spacing.x = 2.0;
    let mut by = None;
    let remove = format!("Remove {title}");
    if tool(&mut ui, Icon::Close, ("Remove from backups", &remove), true) {
        by = Some(Move::Out);
    }
    let down = format!("Move {title} down");
    if tool(&mut ui, Icon::MoveDown, ("Move down", &down), i + 1 < n) {
        by = Some(Move::Down);
    }
    let up = format!("Move {title} up");
    if tool(&mut ui, Icon::MoveUp, ("Move up", &up), i > 0) {
        by = Some(Move::Up);
    }
    (r, by)
}

/// The room left where a carried backup would land.
fn hole(ui: &Ui, rect: Rect) {
    let t = Tokens::get(ui.ctx());
    let room = rect.shrink2(vec2(0.0, 5.0));
    ui.painter().rect_filled(room, t.radius, t.accent_soft);
    let edge = egui::Stroke::new(1.0, t.accent.gamma_multiply(0.5));
    let inside = egui::StrokeKind::Inside;
    ui.painter().rect_stroke(room, t.radius, edge, inside);
}

/// A small icon button, greyed when `on` is false; true when clicked.
fn tool(ui: &mut Ui, icon: Icon, (tip, name): (&str, &str), on: bool) -> bool {
    let r = ui.add_enabled_ui(on, |ui| widgets::small_icon_button(ui, icon, tip));
    widgets::name(&r.inner, name);
    r.inner.clicked()
}

/// Paints a backup in `rect`: grip, place, provider, name, small print.
fn face(ui: &Ui, rect: Rect, (p, line): (&ModelPreset, &str), i: usize, hot: bool) {
    let t = Tokens::get(ui.ctx());
    let (left, mid) = (rect.left(), rect.center().y);
    let painter = ui.painter();
    let dots = if hot { t.ink } else { t.muted };
    for (dx, dy) in GRIP {
        painter.circle_filled(pos2(left + 6.0 + dx, mid + dy), 1.3, dots);
    }
    let small = FontId::proportional(t.text_small);
    let (place, at) = (format!("{}", i + 1), pos2(left + 26.0, mid));
    let middle = Align2::CENTER_CENTER;
    painter.text(at, middle, place, small.clone(), t.muted);
    let mark = Rect::from_center_size(pos2(left + 54.0, mid), vec2(28.0, 28.0));
    badge(ui, mark, p.provider);
    let top_left = pos2(left + 78.0, rect.top());
    let text = Rect::from_min_max(top_left, pos2(rect.right() - TOOLS_W, rect.bottom()));
    let painter = painter.with_clip_rect(text);
    let (font, start) = (FontId::proportional(t.text), Align2::LEFT_CENTER);
    let ink = if hot { t.accent } else { t.ink };
    painter.text(pos2(text.left(), mid - 9.0), start, p.title(), font, ink);
    painter.text(pos2(text.left(), mid + 9.0), start, line, small, t.muted);
}

/// The carried backup, lifted over the list with its top at `y`, numbered
/// for the row it would land in.
fn ghost(ui: &Ui, (p, line): (&ModelPreset, &str), to: usize, y: f32) {
    let t = Tokens::get(ui.ctx());
    let width = ui.min_rect().width();
    let at = pos2(ui.min_rect().left(), y);
    let id = egui::Id::new("ai-backup-carried");
    let area = z_order::area(ui.ctx(), Tier::Tooltip, id);
    let area = area.fixed_pos(at).interactable(false);
    area.show(ui.ctx(), |ui| {
        let size = vec2(width, ITEM_H);
        let shape = Rect::from_min_size(at, size).expand2(vec2(8.0, 0.0));
        let shadow = egui::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: t.shadow,
        };
        let painter = ui.painter();
        painter.add(shadow.as_shape(shape, t.radius));
        painter.rect_filled(shape, t.radius, t.card);
        let edge = egui::Stroke::new(1.0, t.accent);
        painter.rect_stroke(shape, t.radius, edge, egui::StrokeKind::Inside);
        let (place, _) = ui.allocate_exact_size(size, Sense::hover());
        face(ui, place, (p, line), to, true);
    });
}

/// Moves the `from`th id to be the `to`th; true if it moved.
fn reorder(ids: &mut Vec<PresetId>, from: usize, to: usize) -> bool {
    let to = to.min(ids.len().saturating_sub(1));
    if to == from || from >= ids.len() {
        return false;
    }
    let id = ids.remove(from);
    ids.insert(to, id);
    true
}

/// What a button asked of the `i`th id; true if the list changed.
fn moved(ids: &mut Vec<PresetId>, i: usize, by: Move) -> bool {
    match by {
        Move::Up => reorder(ids, i, i.saturating_sub(1)),
        Move::Down => reorder(ids, i, i + 1),
        Move::Out if i < ids.len() => {
            ids.remove(i);
            true
        }
        Move::Out => false,
    }
}

/// The list's row once every backup is removed.
fn empty(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), ROW_H);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        let text = "No backups: a task whose model fails tells you instead";
        ui.label(egui::RichText::new(text).color(t.muted));
    });
    divider(ui);
}

/// A saved model not in the list yet, added at its end; true if one was.
fn add(ui: &mut Ui, presets: &[ModelPreset], ids: &mut Vec<PresetId>) -> bool {
    let out = |p: &&ModelPreset| !ids.contains(&p.id);
    let left: Vec<&ModelPreset> = presets.iter().filter(out).collect();
    let mut picked = None;
    let tip = "Tasks try it after the models above";
    row(ui, "Add a backup", tip, |ui| {
        let ready = !left.is_empty();
        let shown = if ready {
            "Choose a model"
        } else {
            "All are in"
        };
        ui.add_enabled_ui(ready, |ui| {
            widgets::combo("ai-backup-add")
                .width(PICK_W)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for p in &left {
                        if ui.selectable_label(false, p.title()).clicked() {
                            picked = Some(p.id);
                        }
                    }
                });
        });
    });
    let Some(id) = picked else {
        return false;
    };
    ids.push(id);
    true
}

/// The way back to the automatic list, once the reader arranged it.
fn footer(ui: &mut Ui, backups: &mut Backups) {
    let first = Backups::AUTO;
    if !backups.arranged() {
        return;
    }
    ui.add_space(8.0);
    if ui
        .link(format!("Go back to your first {first} models"))
        .clicked()
    {
        backups.reset();
    }
}

/// Which tasks a busy model takes first: one class picker per task.
fn priority(ui: &mut Ui, ai: &mut AiSettings) {
    section_help(ui, "Who goes first", &PRIORITY_HELP);
    let wide = widgets::segments_width(ui, &Class::ALL.map(Class::label));
    group(ui, |ui| {
        for (i, task) in TaskId::ALL.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            let spec = task.spec();
            let tip = format!("Usually {}", spec.class.label());
            row(ui, spec.label, &tip, |ui| {
                let mut class = ai.class(task);
                let named = |c: Class| (None, c.label());
                ui.allocate_ui(vec2(wide, CLASS_H), |ui| {
                    widgets::segments_of(ui, CLASS_H, &mut class, &Class::ALL, 3, named);
                });
                if class != ai.class(task) {
                    ai.set_class(task, class);
                }
            });
        }
    });
}

mod fold;

#[cfg(test)]
#[path = "busy_tests.rs"]
mod tests;
