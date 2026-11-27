//! A row with a list beside it: Custom skills, and the Dictionary
//! picker's AI languages.
//!
//! Hovered or pressed, the row opens a narrow list beside it. Typing
//! while the list shows finds in it: the search field slides in on top,
//! the first match lights, the arrows move the light and Enter picks it.
//! Escape clears the search and folds the list; the next Escape is the
//! menu's own.

use egui::{
    Context, CornerRadius, EventFilter, Id, Key, LayerId, Modifiers, Rect, Response, ScrollArea,
    Sense, Shape, Ui, UiBuilder, pos2, vec2,
};

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

const LIST_H: f32 = 240.0;
/// How long the list stays once the pointer leaves the row.
const GRACE: f64 = 0.24;
/// The search field's slide in, in seconds.
const SLIDE: f32 = 0.16;
const FIELD_H: f32 = 34.0;

/// What a side list holds, and how it reads.
pub struct Spec<'a, T> {
    /// Its rows: what each picks, and its name.
    pub items: &'a [(T, String)],
    /// The search field's hint and name: "Find a skill".
    pub find: &'a str,
    /// Said when nothing matches: "No skill matches".
    pub none: &'a str,
    /// The list's width, narrower than the menu it opens from.
    pub width: f32,
    /// The row's corners, lit while its list shows.
    pub corners: CornerRadius,
}

/// A side list's frame: its row, whose click is also the caller's, and
/// the item picked.
pub struct Shown<T> {
    pub row: Response,
    pub picked: Option<T>,
}

/// The row's state between frames.
#[derive(Clone, Default)]
struct State {
    over: Option<f64>,
    pinned: bool,
    pass: u64,
    query: String,
    /// Folded by Escape under the pointer: hover waits for it to leave.
    hush: bool,
    /// The match Enter takes, lit while searching.
    lit: usize,
    keyboard: bool,
}

/// An open list last frame, and its layer.
#[derive(Clone, Copy)]
struct Held {
    pass: u64,
    layer: LayerId,
}

fn held_id() -> Id {
    Id::new("side-list-held")
}

fn held(ctx: &Context) -> Option<Held> {
    let pass = ctx.cumulative_pass_nr();
    let held: Option<Held> = ctx.data(|d| d.get_temp(held_id()));
    held.filter(|h| h.pass + 1 >= pass)
}

/// Whether a side list is open: Escape folds it first.
pub fn holds_escape(ctx: &Context) -> bool {
    held(ctx).is_some()
}

/// Whether `layer` is the open list's, so a press there keeps its menu.
pub fn owns(ctx: &Context, layer: Option<LayerId>) -> bool {
    layer.is_some() && held(ctx).map(|h| h.layer) == layer
}

/// `spec`'s row, drawn by `row`, and its list beside it, each item drawn
/// by `cell` (true when pressed).
pub fn show<T: Clone>(
    ui: &mut Ui,
    spec: Spec<'_, T>,
    row: impl FnOnce(&mut Ui) -> Response,
    cell: impl FnMut(&mut Ui, &T, &str) -> bool,
) -> Shown<T> {
    show_beside(ui, spec, None, row, cell)
}

/// Like `show`, but keeps the list outside an enclosing menu.
pub fn show_beside<T: Clone>(
    ui: &mut Ui,
    spec: Spec<'_, T>,
    parent: Option<Rect>,
    row: impl FnOnce(&mut Ui) -> Response,
    cell: impl FnMut(&mut Ui, &T, &str) -> bool,
) -> Shown<T> {
    let ctx = ui.ctx().clone();
    let (now, pass) = (ui.input(|i| i.time), ctx.cumulative_pass_nr());
    let under = ui.painter().add(Shape::Noop);
    let resp = row(ui);
    let id = resp.id.with("side-list");
    let mut state: State = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
    if state.pass == 0 || state.pass + 1 < pass {
        state = State::default();
    }
    state.pass = pass;
    state.hush &= resp.contains_pointer();
    if resp.contains_pointer() && !state.hush {
        state.over = Some(now);
    }
    ui.ctx()
        .data_mut(|d| d.insert_temp(resp.id.with("submenu-trigger"), true));
    let right =
        resp.has_focus() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowRight));
    if right {
        state.pinned = true;
    } else if resp.clicked() {
        state.pinned = !state.pinned;
    }
    let lingers = state.over.is_some_and(|t0| now - t0 < GRACE);
    let mut open = state.pinned || !state.query.is_empty() || lingers;
    if open && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        state = State {
            pass,
            hush: true,
            ..State::default()
        };
        open = false;
    }
    let mut picked = None;
    let popup = popup(&resp, id.with("list"), open, spec.width, parent);
    let shown = popup.show(|ui| {
        picked = contents(ui, (&spec, id), &mut state, cell);
        ui.layer_id()
    });
    let (pos, pressed) = ui.input(|i| (i.pointer.interact_pos(), i.pointer.any_pressed()));
    let inside = shown.as_ref().is_some_and(|s| {
        s.response.contains_pointer() || pos.is_some_and(|p| s.response.rect.contains(p))
    });
    if inside {
        state.over = Some(now);
    }
    let away = pressed && !inside && !resp.contains_pointer();
    if picked.is_some() || away {
        state = State {
            pass,
            ..State::default()
        };
    }
    linger(ui, &state, now);
    if let Some(s) = shown.filter(|_| open) {
        let layer = s.inner;
        ctx.data_mut(|d| d.insert_temp(held_id(), Held { pass, layer }));
        if !resp.hovered() {
            let t = Tokens::get(&ctx);
            let lit = Shape::rect_filled(resp.rect, spec.corners, t.selected);
            ui.painter().set(under, lit);
        }
    }
    ui.data_mut(|d| d.insert_temp(id, state));
    Shown { row: resp, picked }
}

/// Keeps a selection list outside its enclosing card when space allows.
fn popup<'a>(
    resp: &Response,
    popup_id: Id,
    open: bool,
    width: f32,
    parent: Option<Rect>,
) -> egui::Popup<'a> {
    let mut popup = egui::Popup::from_response(resp)
        .id(popup_id)
        .open(open)
        .sense(widgets::POPUP_SENSE)
        .align(egui::RectAlign::RIGHT_START)
        .gap(4.0)
        .width(width);
    let ctx = &resp.ctx;
    if let Some(parent) = parent {
        let width = ctx
            .memory(|m| m.area_rect(popup_id))
            .map_or(width + 14.0, |r| r.width());
        let room = ctx.content_rect();
        if room.right() - parent.right() >= width + 4.0
            || parent.left() - room.left() >= width + 4.0
        {
            let anchor = Rect::from_min_max(
                pos2(parent.left(), resp.interact_rect.top()),
                pos2(parent.right(), resp.interact_rect.bottom()),
            );
            popup = popup
                .anchor(anchor)
                .align_alternatives(&[egui::RectAlign::LEFT_START]);
        } else {
            popup = popup
                .anchor(parent)
                .align(egui::RectAlign::BOTTOM_START)
                .align_alternatives(&[
                    egui::RectAlign::TOP_START,
                    egui::RectAlign::BOTTOM_END,
                    egui::RectAlign::TOP_END,
                ]);
        }
    }
    popup
}

/// The open list: its search field over its items; the one picked.
fn contents<T: Clone>(
    ui: &mut Ui,
    (spec, id): (&Spec<'_, T>, Id),
    state: &mut State,
    cell: impl FnMut(&mut Ui, &T, &str) -> bool,
) -> Option<T> {
    crate::z_order::register_popup(ui.ctx(), ui.layer_id());
    widgets::menu_style(ui, spec.width);
    let step = arrows(ui);
    state.keyboard |= step != 0;
    let typed = state.query.clone();
    let enter = find(ui, (id, spec.find), &mut state.query);
    if state.query != typed {
        state.lit = 0;
    }
    let keys = (enter, step, &mut state.lit, state.keyboard);
    list(ui, spec, &state.query, keys, cell)
}

/// Wakes the frame when a hovered list's grace runs out.
fn linger(ui: &Ui, state: &State, now: f64) {
    if state.pinned || !state.query.is_empty() {
        return;
    }
    if let Some(t0) = state.over {
        let left = GRACE - (now - t0);
        if left > 0.0 {
            ui.ctx().request_repaint_after_secs(left as f32);
        }
    }
}

/// Which way the arrows move the light while `searching`: down 1, up -1.
fn arrows(ui: &Ui) -> i8 {
    let press = |key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    match (press(Key::ArrowDown), press(Key::ArrowUp)) {
        (true, _) => 1,
        (_, true) => -1,
        _ => 0,
    }
}

/// The search field, named `name`: it holds the keys while the list
/// shows, and slides in once something is typed. True when Enter picks
/// the lit match.
fn find(ui: &mut Ui, (id, name): (Id, &str), query: &mut String) -> bool {
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
    // The arrows move the light, not the focus.
    let keep = EventFilter {
        horizontal_arrows: true,
        vertical_arrows: true,
        ..EventFilter::default()
    };
    let input = widgets::input(query)
        .event_filter(keep)
        .id(id.with("find"))
        .icon(Icon::Search)
        .hint(name)
        .label(name);
    let field = input.clearable(true).compact().show(&mut child).response;
    // Read before the keys are taken back, or Enter never counts.
    let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
    if !field.has_focus() {
        field.request_focus();
    }
    enter
}

/// The items whose names hold `query`, as rows, the match `lit` lit
/// once moved by `step`; the one picked, the lit one with `enter`.
fn list<T: Clone>(
    ui: &mut Ui,
    spec: &Spec<'_, T>,
    query: &str,
    (enter, step, lit, keyboard): (bool, i8, &mut usize, bool),
    mut cell: impl FnMut(&mut Ui, &T, &str) -> bool,
) -> Option<T> {
    let find = query.trim().to_lowercase();
    let found: Vec<&(T, String)> = (spec.items.iter())
        .filter(|(_, name)| find.is_empty() || name.to_lowercase().contains(&find))
        .collect();
    let last = found.len().saturating_sub(1);
    let moved = match step {
        1 => *lit + 1,
        -1 => lit.saturating_sub(1),
        _ => *lit,
    };
    *lit = moved.min(last);
    let mut picked = found
        .get(*lit)
        .map(|(item, _)| item.clone())
        .filter(|_| enter);
    let light = Tokens::get(ui.ctx()).accent_soft;
    ScrollArea::vertical().max_height(LIST_H).show(ui, |ui| {
        for (i, (item, name)) in found.iter().enumerate() {
            let under = ui.painter().add(Shape::Noop);
            let top = ui.cursor().top();
            if cell(ui, item, name) {
                picked = Some(item.clone());
            }
            if (find.is_empty() && !keyboard) || i != *lit {
                continue;
            }
            let rows = ui.min_rect();
            let rect = Rect::from_x_y_ranges(rows.x_range(), top..=rows.bottom());
            let fill = Shape::rect_filled(rect, spec.corners, light);
            ui.painter().set(under, fill);
            if step != 0 {
                ui.scroll_to_rect(rect, None);
            }
        }
        if found.is_empty() {
            widgets::menu_note(ui, spec.none);
        }
    });
    picked
}
