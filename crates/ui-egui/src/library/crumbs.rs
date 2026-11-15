//! The title bar's breadcrumb: Home down to the open shelf, folding
//! when cramped.

use egui::text::{CCursor, CCursorRange};
use egui::{
    Button, DragAndDrop, Id, Popup, PopupCloseBehavior, RectAlign, Response, RichText, Stroke,
    StrokeKind, Ui,
};
use mareader_library::{NAME_MAX, ShelfId};

use super::living::OPEN_DWELL;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::library::{Action, BookAction, Carry, Crumb, Named, ShelfAction};

pub const CRUMB_MENU: &str = "library-crumb-menu";
const ELIDED_MENU: &str = "library-crumb-elided";
/// A long name is cut, so the title bar keeps its buttons.
const CRUMB_MAX: f32 = 160.0;
const MENU_WIDTH: f32 = 224.0;
/// The levels a fold keeps: three share the row with the search field.
const KEEP: usize = 3;
/// A shallower trail never folds: the smallest fold hides two levels.
const FOLD_MIN: usize = 4;
const GAP: f32 = 2.0;
const CHEVRON: f32 = 14.0;
/// The folded levels stay open this long after the pointer leaves.
const GRACE: f64 = 0.22;

/// The trail from Home to the open level, within `room` once measured.
pub fn show(
    ui: &mut Ui,
    trail: &[Crumb],
    renaming: Option<ShelfId>,
    room: Option<f32>,
    out: &mut Commands,
) {
    let Some((home, levels)) = trail.split_first() else {
        return;
    };
    ui.spacing_mut().item_spacing.x = GAP;
    let r = crumb_button(ui, home, levels.is_empty());
    let r = r.on_hover_text("The top of your library");
    if r.clicked() {
        out.push(go(None));
    }
    take_drops(ui, &r, None, out);
    let Some((current, _)) = levels.split_last() else {
        return;
    };
    let widths = measure(ui, levels);
    let available = room.map(|room| room - r.rect.width() - GAP);
    let split = choose_split(&widths, available, levels.len());
    let (elided, shown) = levels.split_at(split.min(levels.len()));
    if !elided.is_empty() {
        chevron(ui);
        folded(ui, elided, out);
    }
    for crumb in shown {
        chevron(ui);
        if crumb.id == current.id {
            match (renaming, current.id) {
                (Some(editing), Some(id)) if editing == id => rename(ui, id, &current.name, out),
                _ => current_menu(ui, current, out),
            }
            continue;
        }
        level_crumb(ui, crumb, out);
    }
}

fn go(id: Option<ShelfId>) -> Sent {
    Sent::Engine(Command::Library(Action::Shelf(ShelfAction::GoTo(id))))
}

fn chevron(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add(Icon::ChevronRight.image(ui, CHEVRON).tint(t.muted));
}

/// The ellipsis, then each level with its chevron, as the bar draws them.
fn measure(ui: &Ui, levels: &[Crumb]) -> Vec<f32> {
    let pad = 2.0 * ui.spacing().button_padding.x;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let wide = |text: &str| {
        let t =
            ui.fonts_mut(|f| f.layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE));
        t.size().x
    };
    let last = levels.len().saturating_sub(1);
    let ellipsis = CHEVRON + GAP + CHEVRON + pad + GAP;
    let crumbs = levels.iter().enumerate().map(|(n, c)| {
        // The open level's crumb carries its menu's chevron.
        let menu = if n == last { 11.0 + GAP } else { 0.0 };
        CHEVRON + GAP + (wide(&c.name) + pad + menu).min(CRUMB_MAX) + GAP
    });
    std::iter::once(ellipsis).chain(crumbs).collect()
}

/// Never one: one hidden level costs a hover for the same bar width.
fn elide_at(len: usize) -> usize {
    match len.saturating_sub(KEEP) {
        1 => 0,
        split => split,
    }
}

/// How many of the oldest levels fold: the fewest that let the rest fit.
fn choose_split(widths: &[f32], available: Option<f32>, len: usize) -> usize {
    if len < FOLD_MIN {
        return 0;
    }
    let available = available.unwrap_or(0.0);
    let (Some((&ellipsis, items)), true) = (widths.split_first(), available > 0.0) else {
        return elide_at(len);
    };
    if items.len() != len {
        return elide_at(len);
    }
    let gaps = |n: usize| GAP * n.saturating_sub(1) as f32;
    if items.iter().sum::<f32>() + gaps(len) <= available {
        return 0;
    }
    for split in 2..len {
        let rest = items.get(split..).unwrap_or_default();
        if ellipsis + rest.iter().sum::<f32>() + gaps(rest.len()) + GAP <= available {
            return split;
        }
    }
    (len - 1).max(2)
}

/// A crumb; the open level reads strong.
fn crumb_button(ui: &mut Ui, crumb: &Crumb, current: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = match current {
        true => RichText::new(&crumb.name)
            .font(theme::medium(13.0))
            .color(t.ink),
        false => RichText::new(&crumb.name).color(t.muted),
    };
    ui.scope(|ui| {
        ui.set_max_width(CRUMB_MAX);
        ui.add(Button::new(text).truncate().frame_when_inactive(false))
    })
    .inner
}

/// A level above the open one: a way back up, and a drop target.
fn level_crumb(ui: &mut Ui, crumb: &Crumb, out: &mut Commands) {
    let r = crumb_button(ui, crumb, false).on_hover_text(&crumb.name);
    widgets::name(&r, &format!("Go back to {}", crumb.name));
    if r.clicked() {
        out.push(go(crumb.id));
    }
    take_drops(ui, &r, crumb.id, out);
}

/// A book or shelf dropped on a crumb moves there.
fn take_drops(ui: &Ui, r: &Response, id: Option<ShelfId>, out: &mut Commands) {
    let keep = ui.input(|i| i.modifiers.alt);
    let hovered = r.dnd_hover_payload::<Carry>();
    if hovered.as_ref().is_some_and(|c| c.onto(id, keep).is_some()) {
        let t = Tokens::get(ui.ctx());
        let ring = Stroke::new(2.0, t.accent);
        ui.painter()
            .rect_stroke(r.rect, t.radius, ring, StrokeKind::Inside);
    }
    if hovered.is_some() {
        rest_to_go(ui, id, out);
    }
    if let Some(carry) = r.dnd_release_payload::<Carry>() {
        out.extend(
            carry
                .onto(id, keep)
                .map(|a| Sent::Engine(Command::Library(a))),
        );
    }
}

/// A carry resting on a crumb goes there, to be dropped in place.
fn rest_to_go(ui: &Ui, id: Option<ShelfId>, out: &mut Commands) {
    let key = Id::new("library-crumb-rest");
    let (now, pass) = (ui.input(|i| i.time), ui.ctx().cumulative_pass_nr());
    let since = ui.data_mut(|d| {
        let rest = d.get_temp_mut_or_insert_with(key, || (id, now, pass));
        // Another crumb, or a gap since the last frame: a fresh rest.
        if rest.0 != id || rest.2 + 1 < pass {
            *rest = (id, now, pass);
        }
        rest.2 = pass;
        rest.1
    });
    let left = OPEN_DWELL - (now - since);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
        return;
    }
    ui.data_mut(|d| d.remove::<(Option<ShelfId>, f64, u64)>(key));
    out.push(go(id));
}

/// When the pointer last rested on the fold, and whether a press keeps
/// it open.
#[derive(Clone, Copy, Default)]
struct Fold {
    over: f64,
    pinned: bool,
}

/// The folded levels: open on hover, a carry, a click or Down.
fn folded(ui: &mut Ui, elided: &[Crumb], out: &mut Commands) {
    let id = Id::new(ELIDED_MENU);
    let now = ui.input(|i| i.time);
    let mut fold = ui.data(|d| d.get_temp::<Fold>(id)).unwrap_or_default();
    let t = Tokens::get(ui.ctx());
    let tip = format!("Show the {} levels above", elided.len());
    let open_before = fold.pinned || now - fold.over < GRACE;
    let button = Button::image(Icon::More.image(ui, CHEVRON).tint(t.muted));
    let r = ui.add(
        button
            .frame_when_inactive(open_before)
            .selected(open_before),
    );
    let r = r.on_hover_text(&tip);
    widgets::name(&r, &tip);
    if r.contains_pointer() {
        fold.over = now;
    }
    let down = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
    if r.clicked() || down {
        fold.pinned = !fold.pinned || down;
    }
    let open = fold.pinned || now - fold.over < GRACE;
    let chosen = Popup::from_response(&r)
        .id(id)
        .open(open)
        .align(RectAlign::BOTTOM_START)
        .gap(6.0)
        .show(|ui| {
            let wide = (ui.ctx().content_rect().width() - 24.0).max(160.0);
            ui.set_max_width(wide);
            ui.spacing_mut().item_spacing.x = GAP;
            let mut chosen = false;
            ui.horizontal_wrapped(|ui| {
                for (n, crumb) in elided.iter().enumerate() {
                    if n > 0 {
                        chevron(ui);
                    }
                    let before = out.len();
                    level_crumb(ui, crumb, out);
                    chosen |= out.len() > before;
                }
            });
            chosen
        });
    let inside = chosen
        .as_ref()
        .is_some_and(|c| c.response.contains_pointer());
    if inside {
        fold.over = now;
    }
    let pressed_away = ui.input(|i| i.pointer.any_pressed()) && !inside && !r.contains_pointer();
    let escape = open && ui.input(|i| i.key_pressed(egui::Key::Escape));
    if chosen.is_some_and(|c| c.inner) || pressed_away || escape {
        fold = Fold::default();
    }
    let carrying = DragAndDrop::has_payload_of_type::<Carry>(ui.ctx());
    if carrying && r.contains_pointer() {
        fold.over = now;
    }
    let left = GRACE - (now - fold.over);
    if !fold.pinned && left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
    }
    ui.data_mut(|d| d.insert_temp(id, fold));
}

/// The open shelf's crumb: a menu to rename it or take it apart.
fn current_menu(ui: &mut Ui, current: &Crumb, out: &mut Commands) {
    let menu = Id::new(CRUMB_MENU);
    let t = Tokens::get(ui.ctx());
    let open = Popup::is_id_open(ui.ctx(), menu);
    let name = RichText::new(&current.name)
        .font(theme::medium(13.0))
        .color(t.ink);
    let mark = Icon::ChevronDown.image(ui, 11.0).tint(t.muted);
    let r = ui
        .scope(|ui| {
            ui.set_max_width(CRUMB_MAX + 16.0);
            let button = Button::new((name, mark)).truncate();
            ui.add(button.frame_when_inactive(open).selected(open))
        })
        .inner
        .on_hover_text(&current.name);
    widgets::name(&r, &format!("{} shelf options", current.name));
    take_drops(ui, &r, current.id, out);
    let Some(id) = current.id else {
        return;
    };
    let act = |a: ShelfAction| Sent::Engine(Command::Library(Action::Shelf(a)));
    Popup::from_toggle_button_response(&r)
        .id(menu)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::BOTTOM_START)
        .gap(6.0)
        .width(MENU_WIDTH)
        .show(|ui| {
            widgets::menu_style(ui, MENU_WIDTH);
            if widgets::menu_row(ui, Icon::Rename, "Rename…", None, false).clicked() {
                out.push(act(ShelfAction::StartRename(id)));
            }
            let copy = widgets::menu_row(ui, Icon::Copy, "Duplicate", None, false);
            let tip = "A second shelf of your own, holding the same books";
            if copy.on_hover_text(tip).clicked() {
                let picks = vec![mareader_library::picks::Pick::Shelf(id)];
                out.push(Sent::Engine(Command::Library(Action::Book(
                    BookAction::Duplicate(picks),
                ))));
            }
            widgets::menu_rule(ui);
            if widgets::menu_row_danger(ui, Icon::Remove, "Remove shelf").clicked() {
                out.push(act(ShelfAction::Apart(id)));
            }
            if current.watched {
                let note = "Cut from a watched folder: removing takes it off the list, and it \
                            returns if the folder places a book here again.";
                widgets::menu_note(ui, note);
            }
            widgets::close_on_key(ui);
        });
}

/// Focuses a fresh name field with all of it selected.
pub(super) fn select_all(field: &egui::Response, id: Id, text: &str) {
    field.request_focus();
    let mut state = egui::TextEdit::load_state(&field.ctx, id).unwrap_or_default();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(text.chars().count()));
    state.cursor.set_char_range(Some(all));
    state.store(&field.ctx, id);
}

/// The name in place: Enter keeps it, Escape doesn't.
fn rename(ui: &mut Ui, id: ShelfId, name: &str, out: &mut Commands) {
    let field = Id::new("library-shelf-name").with(id);
    let draft = ui.data(|d| d.get_temp::<String>(field));
    let fresh = draft.is_none();
    let mut text = draft.unwrap_or_else(|| name.to_owned());
    let edit = egui::TextEdit::singleline(&mut text)
        .id(field)
        .char_limit(NAME_MAX)
        .desired_width(CRUMB_MAX + 20.0);
    let r = ui.add(edit);
    widgets::name(&r, "Shelf name");
    if fresh {
        // The whole name selected, so typing replaces it.
        select_all(&r, field, &text);
    }
    let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if !escape && !enter {
        ui.data_mut(|d| d.insert_temp(field, text));
        return;
    }
    ui.data_mut(|d| d.remove::<String>(field));
    let action = match escape {
        true => Action::Shelf(ShelfAction::CancelRename),
        false => Action::Rename(Named::Shelf(id), text),
    };
    out.push(Sent::Engine(Command::Library(action)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(len: usize, ellipsis: f32, crumb: f32) -> Vec<f32> {
        std::iter::once(ellipsis)
            .chain(std::iter::repeat_n(crumb, len))
            .collect()
    }

    #[test]
    fn a_fold_never_hides_one_level_and_keeps_three_by_count() {
        assert_eq!(elide_at(KEEP + 1), 0);
        assert_eq!(elide_at(KEEP + 2), 2);
        for len in (KEEP + 2)..=12 {
            assert_eq!(len - elide_at(len), KEEP);
        }
    }

    #[test]
    fn a_shallow_trail_never_folds_however_cramped() {
        for len in 0..FOLD_MIN {
            for room in [None, Some(0.0), Some(40.0), Some(5000.0)] {
                let widths = measured(len, 22.0, 900.0);
                assert_eq!(choose_split(&widths, room, len), 0, "{len} at {room:?}");
            }
        }
    }

    #[test]
    fn a_cramped_bar_folds_the_oldest_levels_it_must_and_no_more() {
        let widths = measured(4, 22.0, 200.0);
        assert_eq!(choose_split(&widths, Some(1000.0), 4), 0, "it all fits");
        assert_eq!(
            choose_split(&widths, Some(426.0), 4),
            2,
            "the fit is inclusive"
        );
        assert_eq!(choose_split(&widths, Some(425.0), 4), 3);
        assert_eq!(
            choose_split(&widths, Some(100.0), 4),
            3,
            "never past the trail"
        );
    }

    #[test]
    fn unmeasured_widths_fold_by_count() {
        assert_eq!(choose_split(&[], Some(500.0), 6), elide_at(6));
        assert_eq!(
            choose_split(&measured(4, 22.0, 9.0), Some(500.0), 6),
            elide_at(6)
        );
        assert_eq!(choose_split(&measured(5, 22.0, 9.0), None, 5), elide_at(5));
        for len in 1..8 {
            for room in [Some(0.0), Some(24.0), Some(646.0)] {
                let split = choose_split(&measured(len, 22.0, 300.0), room, len);
                assert!(split != 1 && split <= len, "{split} of {len}");
            }
        }
    }
}
