//! The list: a level as a tree of shelf rows that unfold in place.

use std::collections::BTreeSet;

use egui::{FontId, Id, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2};
use mareader_library::{Library, Shelf, ShelfId, View};

use super::{
    A4, Cx, Item, ROW_GAP, ROW_H, ROW_INSET, add, backing, badges, centred, line, links, list_row,
    lit_ring, margin, prefetch, shelf_detail, shelf_hit,
};
use crate::icons::Icon;
use crate::library::menus;
use crate::theme;
use crate::ui_command::Sent;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::library::{Carry, ShelfAction};
use mareader_library::picks::Pick;

/// How far each level down sits in.
const INDENT: f32 = 20.0;
/// Seconds a carry rests on a folded row before it unfolds.
const UNFOLD_DWELL: f64 = 0.65;
/// Deeper than this, a tree is surely a loop.
const DEPTH_MAX: usize = 32;

/// One row of the list: an item, how deep, and the level it stands on.
#[derive(Clone, Copy)]
pub(super) struct Row<'a> {
    item: Item<'a>,
    depth: usize,
    level: Option<ShelfId>,
}

/// How rows unfold: the view's order, which shelves, and whether searching.
type How<'h> = (&'h View, &'h BTreeSet<ShelfId>, bool);

/// The level's items, each unfolded shelf followed by what's on it.
pub(super) fn rows<'a>(
    model: &'a Library,
    items: &[Item<'a>],
    at: Option<ShelfId>,
    how: How<'_>,
) -> Vec<Row<'a>> {
    let mut out = Vec::with_capacity(items.len());
    let mut seen = BTreeSet::new();
    for &item in items {
        grow(model, item, (0, at), how, &mut seen, &mut out);
    }
    out
}

fn grow<'a>(
    model: &'a Library,
    item: Item<'a>,
    (depth, level): (usize, Option<ShelfId>),
    how: How<'_>,
    seen: &mut BTreeSet<ShelfId>,
    out: &mut Vec<Row<'a>>,
) {
    out.push(Row { item, depth, level });
    let (view, unfolded, searching) = how;
    let Item::Shelf(shelf) = item else {
        return;
    };
    if !unfolded.contains(&shelf.id) || depth >= DEPTH_MAX || !seen.insert(shelf.id) {
        return;
    }
    for pick in view.level(model, shelf.id) {
        // Searching, an unfolded shelf shows its shelves, not its books.
        if searching && !matches!(pick, Pick::Shelf(_)) {
            continue;
        }
        if let Some(child) = Item::of(model, pick) {
            grow(model, child, (depth + 1, Some(shelf.id)), how, seen, out);
        }
    }
}

pub(super) fn list(ui: &mut Ui, rows: &[Row<'_>], cx: &mut Cx<'_>) {
    let mut area = egui::ScrollArea::vertical()
        .id_salt("library-list")
        .auto_shrink(false)
        .content_margin(margin(cx));
    let lit = cx.scroll_to.and(cx.lit);
    if let Some(at) = lit.and_then(|l| rows.iter().position(|r| r.item.pick() == l.pick)) {
        area = area.vertical_scroll_offset(centred(ui, at, ROW_H));
    }
    let items: Vec<Item<'_>> = rows.iter().map(|r| r.item).collect();
    let slots = rows.len() + usize::from(cx.adding.is_some());
    area.show_rows(ui, ROW_H, slots, |ui, range| {
        for at in range.clone() {
            let Some(row) = rows.get(at) else {
                let size = vec2(ui.available_width(), ROW_H);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                if let Some(here) = cx.adding {
                    add::row(ui, rect.shrink2(ROW_GAP), ROW_INSET, here, cx.t, cx.out);
                }
                continue;
            };
            cx.level = row.level;
            let indent = row.depth as f32 * INDENT;
            match row.item {
                Item::Shelf(shelf) => shelf_row(ui, shelf, indent, cx),
                Item::Book(book) => list_row(ui, book, indent, cx),
                Item::Link(link) => links::row(ui, link, indent, cx),
            }
        }
        cx.level = cx.at;
        prefetch(cx.covers, &items, range, 1);
    });
}

/// A shelf's row: a click unfolds it in place; its button opens it.
fn shelf_row(ui: &mut Ui, shelf: &Shelf, indent: f32, cx: &mut Cx<'_>) {
    let id = shelf.id;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let (resp, target) = shelf_hit(ui, rect, shelf, ShelfAction::Unfold(id), cx);
    let open = cx.unfolded.contains(&id);
    dwell(ui, &resp, id, open, cx);
    let t = cx.t;
    let picked = cx.picks.contains(Pick::Shelf(id));
    let ground = rect.shrink2(ROW_GAP);
    backing(ui, ground, t.radius, (resp.hovered(), picked, target), t);
    lit_ring(ui, ground, t.radius, Pick::Shelf(id), cx);
    let mid = rect.center().y;
    let left = rect.left() + ROW_INSET + indent;
    let chevron = Rect::from_center_size(pos2(left + 7.0, mid), vec2(14.0, 14.0));
    let glyph = match open {
        true => Icon::ChevronDown,
        false => Icon::ChevronRight,
    };
    glyph.paint(ui, chevron, t.muted);
    let thumb = Rect::from_center_size(
        pos2(chevron.right() + 6.0 + 17.0, mid),
        vec2(34.0, 34.0 * A4),
    );
    // While picking, the mark takes the icon's place.
    match cx.picks.active() {
        true => menus::mark(ui, thumb.center(), picked, t),
        false => {
            let icon = Rect::from_center_size(thumb.center(), vec2(24.0, 24.0));
            Icon::Shelf.paint(ui, icon, t.muted);
        }
    }
    let name = shelf.name.value.as_str();
    let go = Rect::from_center_size(pos2(rect.right() - ROW_INSET - 14.0, mid), vec2(28.0, 28.0));
    let button = ui.interact(go, ui.id().with(("open-shelf", id)), Sense::click());
    let said = format!("Open the {name} shelf");
    button.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &said));
    if button.hovered() {
        ui.painter().rect_filled(go, t.radius, t.hover);
    }
    let ink = if button.hovered() { t.ink } else { t.muted };
    Icon::Open.paint(ui, go.shrink(7.0), ink);
    if button.on_hover_text("Open shelf").clicked() {
        let to = ShelfAction::GoTo(Some(id));
        cx.out
            .push(Sent::Engine(Command::Library(Action::Shelf(to))));
    }
    let end = badges(ui, &resp, id, (go.left() - 8.0, mid), cx);
    let p = ui.painter();
    let x = thumb.right() + 14.0;
    let wide = (end - x - 8.0).max(40.0);
    line(p, name, theme::medium(14.0), t.ink, (x, mid - 18.0), wide);
    let detail = shelf_detail(cx.model, id);
    let small = FontId::proportional(12.0);
    line(p, &detail, small, t.muted, (x, mid + 2.0), wide);
    let rule = Stroke::new(1.0, t.line);
    p.hline(rect.x_range().shrink(ROW_INSET), rect.bottom(), rule);
}

/// What's carried, resting on a folded row, unfolds it.
fn dwell(ui: &Ui, resp: &egui::Response, id: ShelfId, open: bool, cx: &mut Cx<'_>) {
    let key = Id::new(("library-unfold", id));
    if open || resp.dnd_hover_payload::<Carry>().is_none() {
        ui.data_mut(|d| d.remove::<f64>(key));
        return;
    }
    let now = ui.input(|i| i.time);
    let since = ui.data_mut(|d| *d.get_temp_mut_or(key, now));
    let left = UNFOLD_DWELL - (now - since);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
        return;
    }
    ui.data_mut(|d| d.remove::<f64>(key));
    let unfold = ShelfAction::Unfold(id);
    cx.out
        .push(Sent::Engine(Command::Library(Action::Shelf(unfold))));
}
