//! Skills in a menu: the built-ins side by side in one row, and the
//! reader's own folded into one more.
//!
//! Hovered or pressed, the folded row opens a narrow list beside it. Typing
//! while the list shows finds a skill: the search field slides in on top.
//! Escape clears the search and folds the list; the next Escape is the
//! menu's own.

use egui::{
    Context, CornerRadius, FontId, Id, Key, LayerId, Modifiers, Rect, Response, ScrollArea, Sense,
    Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_settings::{PageTask, Skill};

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

/// The folded row's name.
pub const ROW: &str = "Custom skills";
/// The search field's name.
pub const FIND: &str = "Find a skill";

/// The list beside the row: narrower than the menu it opens from.
const WIDTH: f32 = 196.0;
const LIST_H: f32 = 240.0;
/// How long the list stays once the pointer leaves the row.
const GRACE: f64 = 0.24;
/// The search field's slide in, in seconds.
const SLIDE: f32 = 0.16;
const FIELD_H: f32 = 34.0;
const CELL_H: f32 = 34.0;

/// The folded row's state between frames.
#[derive(Clone, Default)]
struct Fold {
    over: Option<f64>,
    pinned: bool,
    pass: u64,
    query: String,
    /// Folded by Escape under the pointer: hover waits for it to leave.
    hush: bool,
}

/// An open list last frame, and its layer.
#[derive(Clone, Copy)]
struct Held {
    pass: u64,
    layer: LayerId,
}

fn held_id() -> Id {
    Id::new("skill-menu-held")
}

fn held(ctx: &Context) -> Option<Held> {
    let pass = ctx.cumulative_pass_nr();
    let held: Option<Held> = ctx.data(|d| d.get_temp(held_id()));
    held.filter(|h| h.pass + 1 >= pass)
}

/// Whether a list of skills is open: Escape folds it first.
pub fn holds_escape(ctx: &Context) -> bool {
    held(ctx).is_some()
}

/// Whether `layer` is the open list's, so a press there keeps its menu.
pub fn owns(ctx: &Context, layer: Option<LayerId>) -> bool {
    layer.is_some() && held(ctx).map(|h| h.layer) == layer
}

/// Translate, Summarize and Simplify side by side; the one pressed.
pub fn builtins(ui: &mut Ui) -> Option<Skill> {
    let t = Tokens::get(ui.ctx());
    let gap = 2.0;
    let cell = vec2((ui.available_width() - gap * 2.0) / 3.0, CELL_H);
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for task in [PageTask::Translate, PageTask::Summarize, PageTask::Simplify] {
            if cell_button(ui, task, cell, &t).clicked() {
                picked = Some(Skill::Builtin(task));
            }
        }
    });
    picked
}

/// The icon a built-in shows.
pub fn icon(task: PageTask) -> Icon {
    match task {
        PageTask::Translate => Icon::Translate,
        PageTask::Summarize => Icon::Summarize,
        PageTask::Simplify => Icon::Simplify,
    }
}

/// One built-in: its icon and name, centred in its share of the row.
fn cell_button(ui: &mut Ui, task: PageTask, size: Vec2, t: &Tokens) -> Response {
    let label = task.label();
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let fill = match (
        resp.is_pointer_button_down_on(),
        resp.hovered() || resp.has_focus(),
    ) {
        (true, _) => Some(t.pressed),
        (false, true) => Some(t.hover),
        _ => None,
    };
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let galley = (ui.painter()).layout_no_wrap(label.to_owned(), FontId::proportional(13.5), t.ink);
    let x = rect.center().x - (15.0 + 6.0 + galley.size().x) / 2.0;
    let y = rect.center().y;
    let tint = if fill.is_some() { t.ink } else { t.muted };
    let mark = Rect::from_min_size(pos2(x, y - 7.5), Vec2::splat(15.0));
    icon(task).paint(ui, mark, tint);
    let at = pos2(x + 21.0, y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.ink);
    resp
}

/// The reader's skills folded into one row; none, no row. The one picked.
pub fn custom(ui: &mut Ui, skills: &[(Skill, String)]) -> Option<Skill> {
    if skills.is_empty() {
        return None;
    }
    let ctx = ui.ctx().clone();
    let (now, pass) = (ui.input(|i| i.time), ctx.cumulative_pass_nr());
    let under = ui.painter().add(egui::Shape::Noop);
    let count = skills.len().to_string();
    let resp = widgets::menu_row_chevron(ui, Icon::Ai, ROW, Some(&count));
    let id = resp.id.with("skills-fold");
    let mut fold: Fold = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    if fold.pass == 0 || fold.pass + 1 < pass {
        fold = Fold::default();
    }
    fold.pass = pass;
    fold.hush &= resp.contains_pointer();
    if resp.contains_pointer() && !fold.hush {
        fold.over = Some(now);
    }
    if resp.clicked() {
        fold.pinned = !fold.pinned;
    }
    let lingers = fold.over.is_some_and(|t0| now - t0 < GRACE);
    let mut open = fold.pinned || !fold.query.is_empty() || lingers;
    if open && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        fold = Fold {
            pass,
            hush: true,
            ..Fold::default()
        };
        open = false;
    }
    let mut picked = None;
    let shown = egui::Popup::from_response(&resp)
        .id(id.with("list"))
        .open(open)
        .sense(widgets::POPUP_SENSE)
        .align(egui::RectAlign::RIGHT_START)
        .gap(4.0)
        .width(WIDTH)
        .show(|ui| {
            crate::z_order::register_popup(ui.ctx(), ui.layer_id());
            widgets::menu_style(ui, WIDTH);
            let enter = find(ui, id, &mut fold.query);
            picked = list(ui, skills, &fold.query, enter);
            ui.layer_id()
        });
    let (pos, pressed) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_pressed()));
    let inside = shown.as_ref().is_some_and(|s| {
        s.response.contains_pointer() || pos.is_some_and(|p| s.response.rect.contains(p))
    });
    if inside {
        fold.over = Some(now);
    }
    let away = pressed && !inside && !resp.contains_pointer();
    if picked.is_some() || away {
        fold = Fold {
            pass,
            ..Fold::default()
        };
    }
    linger(ui, &fold, now);
    if let Some(s) = shown.filter(|_| open) {
        let layer = s.inner;
        ctx.data_mut(|d| d.insert_temp(held_id(), Held { pass, layer }));
        if !resp.hovered() {
            let t = Tokens::get(&ctx);
            let lit = egui::Shape::rect_filled(resp.rect, CornerRadius::same(8), t.selected);
            ui.painter().set(under, lit);
        }
    }
    ui.data_mut(|d| d.insert_temp(id, fold));
    picked
}

/// Wakes the frame when a hovered list's grace runs out.
fn linger(ui: &Ui, fold: &Fold, now: f64) {
    if fold.pinned || !fold.query.is_empty() {
        return;
    }
    if let Some(t0) = fold.over {
        let left = GRACE - (now - t0);
        if left > 0.0 {
            ui.ctx().request_repaint_after_secs(left as f32);
        }
    }
}

/// The search field: it holds the keys while the list shows, and slides
/// in once something is typed. True when Enter picks the first match.
fn find(ui: &mut Ui, id: Id, query: &mut String) -> bool {
    let k = ui
        .ctx()
        .animate_bool_with_time(id.with("find-in"), !query.is_empty(), SLIDE);
    let width = ui.available_width();
    let (room, _) = ui.allocate_exact_size(vec2(width, FIELD_H * k), Sense::hover());
    let top = pos2(room.left(), room.bottom() - FIELD_H);
    let whole = Rect::from_min_size(top, vec2(width, FIELD_H));
    let mut child = ui.new_child(UiBuilder::new().max_rect(whole));
    // Only the top is cut while it slides; the focus glow spills sideways.
    let spill = Rect::from_min_max(room.min - vec2(6.0, 0.0), room.max + vec2(6.0, 6.0));
    let clip = match k < 1.0 {
        true => spill,
        false => spill.expand2(vec2(0.0, 6.0)),
    };
    child.set_clip_rect(clip.intersect(ui.clip_rect()));
    child.multiply_opacity(k);
    let input = widgets::input(query)
        .id(id.with("find"))
        .icon(Icon::Search)
        .hint(FIND)
        .label(FIND);
    let field = input.clearable(true).compact().show(&mut child).response;
    if !field.has_focus() {
        field.request_focus();
    }
    field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

/// The skills whose names hold `query`, as rows; the one picked.
fn list(ui: &mut Ui, skills: &[(Skill, String)], query: &str, enter: bool) -> Option<Skill> {
    let find = query.trim().to_lowercase();
    let found: Vec<&(Skill, String)> = (skills.iter())
        .filter(|(_, name)| find.is_empty() || name.to_lowercase().contains(&find))
        .collect();
    let mut picked = found.first().map(|(s, _)| *s).filter(|_| enter);
    ScrollArea::vertical().max_height(LIST_H).show(ui, |ui| {
        for (skill, name) in &found {
            if widgets::menu_row(ui, Icon::Ai, name, None, false).clicked() {
                picked = Some(*skill);
            }
        }
        if found.is_empty() {
            widgets::menu_note(ui, "No skill matches");
        }
    });
    picked
}
