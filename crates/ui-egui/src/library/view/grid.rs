//! The living grid: rows drawn only in view, cards easing to slots.

use egui::epaint::Shadow;
use egui::{
    Color32, CornerRadius, CursorIcon, DragAndDrop, Id, LayerId, Order, Painter, Pos2, Rect, Sense,
    Shape, Stroke, Ui, Vec2, vec2,
};
use mareader_library::BookId;

use super::super::add::Here;
use super::super::living::{Aim, Rested, Slot};
use super::{
    A4, CARD_TEXT, Cx, GAP, Item, add, centred, cover, line, margin, paper, plate, prefetch,
};
use super::{grid_card, links, shelf_card};
use crate::motion;
use crate::theme;
use crate::ui_command::Sent;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::library::{Carry, ShelfAction};
use mareader_library::picks::Pick;

/// The cards in rows; a carried card opens a hole the rest move around.
pub(super) fn show(ui: &mut Ui, items: &[Item<'_>], columns: u8, cx: &mut Cx<'_>) {
    let cols = usize::from(columns.max(1));
    let width = ui.available_width();
    let card = ((width - GAP * (cols - 1) as f32) / cols as f32).max(1.0);
    let height = card * A4 + CARD_TEXT;
    let pitch = vec2(card + GAP, height + GAP);
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    // Another level is another page: its cards start in place.
    let shape = (cols, card, cx.at);
    cx.motion.begin(shape, dt, motion::enabled(ui.ctx()));
    follow_carry(ui, items, cx);
    // The rest: the cards not lifted, as indexes into `items`.
    let rest: Vec<usize> = match &cx.motion.lift {
        Some(lift) => (items.iter().enumerate())
            .filter(|(_, i)| !lift.holds(i.pick()))
            .map(|(n, _)| n)
            .collect(),
        None => Vec::new(),
    };
    // Cards brought from elsewhere add their hole to the grid.
    let cards = match &cx.motion.lift {
        Some(lift) => rest.len() + lift.hole.size,
        None => items.len(),
    };
    let slots = cards + usize::from(cx.adding.is_some());
    let rows = slots.div_ceil(cols);
    let mut area = egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink(false)
        .content_margin(margin(cx));
    if let Some(at) = cx.scroll_to {
        area = area.vertical_scroll_offset(centred(ui, at / cols, pitch.y));
    }
    area.show_rows(ui, pitch.y, rows, |ui, range| {
        let size = vec2(width, range.len() as f32 * pitch.y);
        let (block, _) = ui.allocate_exact_size(size, Sense::hover());
        let origin = block.min - vec2(0.0, range.start as f32 * pitch.y);
        cx.motion.origin = origin;
        cx.motion.card = vec2(card, height);
        let fill = Fill {
            items,
            rest: &rest,
            cols,
            pitch,
            card: vec2(card, height),
            rows,
        };
        let carry = DragAndDrop::payload::<Carry>(ui.ctx());
        if let Some(carry) = carry.filter(|_| cx.motion.lift.is_some()) {
            carry_over(ui, &fill, &carry, cx);
        }
        for slot in range.start * cols..(range.end * cols).min(slots) {
            let target = vec2(
                (slot % cols) as f32 * pitch.x,
                (slot / cols) as f32 * pitch.y,
            );
            let rect = |at: Vec2| Rect::from_min_size(origin + at, fill.card);
            match fill.at(slot, cx) {
                Some(InSlot::Card(item)) => {
                    let at = cx.motion.place(Slot::Card(item.pick()), target);
                    match item {
                        Item::Shelf(shelf) => shelf_card(ui, rect(at), shelf, cx),
                        Item::Book(book) => grid_card(ui, rect(at), book, cx),
                        Item::Link(link) => links::card(ui, rect(at), link, cx),
                    }
                }
                Some(InSlot::Add(here)) => {
                    let at = cx.motion.place(Slot::Add, target);
                    let cover = Rect::from_min_size(origin + at, vec2(card, card * A4));
                    add::card(ui, cover, here, cx.t, cx.out);
                }
                Some(InSlot::Hole) => {
                    let cover = Rect::from_min_size(origin + target, vec2(card, card * A4));
                    ui.painter()
                        .rect_filled(cover, cx.t.radius_lg, cx.t.hover.gamma_multiply(0.6));
                }
                None => {}
            }
        }
        prefetch(cx.covers, items, range, cols);
    });
    if cx.motion.end() {
        ui.ctx().request_repaint();
    }
}

/// What the grid shows in a slot this frame.
enum InSlot<'a> {
    Card(Item<'a>),
    Hole,
    Add(Here<'a>),
}

/// The grid's shape this frame, for finding what sits where.
struct Fill<'a, 'b> {
    items: &'b [Item<'a>],
    rest: &'b [usize],
    cols: usize,
    pitch: Vec2,
    card: Vec2,
    rows: usize,
}

impl<'a> Fill<'a, '_> {
    /// What `slot` holds: a card, the hole, the plus card, or nothing.
    fn at(&self, slot: usize, cx: &Cx<'a>) -> Option<InSlot<'a>> {
        let (item, end) = match &cx.motion.lift {
            None => (self.items.get(slot), slot == self.items.len()),
            Some(lift) => {
                let Some(r) = lift.hole.rest_at(slot) else {
                    return Some(InSlot::Hole);
                };
                let item = self.rest.get(r).and_then(|&n| self.items.get(n));
                (item, r == self.rest.len())
            }
        };
        match (item, end) {
            (Some(item), _) => Some(InSlot::Card(*item)),
            (None, true) => cx.adding.map(InSlot::Add),
            (None, false) => None,
        }
    }
}

/// Starts or ends following a carry over this level.
fn follow_carry(ui: &Ui, items: &[Item<'_>], cx: &mut Cx<'_>) {
    let carry = DragAndDrop::payload::<Carry>(ui.ctx());
    let level = cx.arranging.map(|a| a.level);
    match (carry, level) {
        (None, _) => {
            if cx.motion.lift.is_some() {
                cx.motion.put_down();
            }
            cx.motion.grab = None;
        }
        // Searching or sorted elsewhere: the label carries it.
        (Some(_), None) => cx.motion.lift = None,
        (Some(carry), Some(level)) => {
            // A shelf opened mid-carry gets a lift of its own.
            let fresh = (cx.motion.lift.as_ref()).is_none_or(|l| l.level != level);
            if fresh {
                let native = carry.items_at(level, cx.model);
                let moving = native.clone().unwrap_or_else(|| carry.picks());
                let shown: Vec<Pick> = items.iter().map(|i| i.pick()).collect();
                cx.motion.lift = None;
                cx.motion.pick_up(moving, &shown, level, native.is_some());
                if let Some(lift) = cx.motion.lift.as_mut().filter(|l| !l.native()) {
                    lift.hold = ui.ctx().pointer_latest_pos();
                }
            }
        }
    }
}

/// How far the pointer moves after a shelf opens before it aims again.
const HOLD: f32 = 8.0;

/// Moves the hole, stacks, files or opens under the pointer, and drops.
fn carry_over(ui: &Ui, fill: &Fill<'_, '_>, carry: &Carry, cx: &mut Cx<'_>) {
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return;
    };
    let view = ui.clip_rect();
    if !view.contains(pointer) {
        return;
    }
    // Room to scroll: the first row above the view, the last below it.
    let origin = cx.motion.origin;
    let bottom = origin.y + fill.rows as f32 * fill.pitch.y;
    let room = (origin.y < view.top() - 0.5, bottom > view.bottom() + 0.5);
    scroll_near_edges(ui, view, pointer, room);
    let p = pointer - cx.motion.origin;
    let (col, row) = ((p.x / fill.pitch.x).floor(), (p.y / fill.pitch.y).floor());
    let inside = vec2(p.x - col * fill.pitch.x, p.y - row * fill.pitch.y);
    let on_card = inside.x <= fill.card.x && inside.y <= fill.card.y;
    // Freshly opened, what's under the pointer waits till it moves.
    if let Some(lift) = &mut cx.motion.lift
        && lift.hold.is_some_and(|at| at.distance(pointer) > HOLD)
    {
        lift.hold = None;
    }
    // What a rest here asks for, and where a drop lands without one.
    let (mut want, mut halves) = (None, None);
    if let Some(lift) = cx.motion.lift.as_ref().filter(|l| l.hold.is_none())
        && (0.0..fill.cols as f32).contains(&col)
        && row >= 0.0
        && on_card
    {
        let slot = row as usize * fill.cols + col as usize;
        let across = inside.x / fill.card.x;
        let middle = (0.25..=0.75).contains(&across);
        let rest = lift.hole.rest_at(slot);
        let item = rest
            .and_then(|r| fill.rest.get(r))
            .and_then(|&n| fill.items.get(n));
        halves = rest
            .filter(|_| item.is_some())
            .map(|r| r + usize::from(across > 0.5));
        want = match item {
            // A shelf's middle takes the card in, and opens on a rest.
            Some(Item::Shelf(shelf)) if middle => {
                cx.filing = Some(shelf.id);
                Some(Aim::Open(shelf.id))
            }
            // A book's middle makes a shelf of both, on a rest.
            Some(Item::Book(book)) if middle && carry.has_books() => Some(Aim::Stack(book.id)),
            _ => halves.map(Aim::Hole),
        };
    }
    let now = ui.input(|i| i.time);
    if let Some(lift) = &mut cx.motion.lift {
        match lift.aim(want, now) {
            Rested::Wait(left) => ui.ctx().request_repaint_after_secs(left as f32),
            Rested::Open(shelf) => {
                let go = ShelfAction::GoTo(Some(shelf));
                cx.out
                    .push(Sent::Engine(Command::Library(Action::Shelf(go))));
            }
            Rested::Nothing => {}
        }
        cx.stacking = lift.stack;
    }
    // Letting go lands where the pointer is, without waiting.
    let (released, keep) = ui.input(|i| (i.pointer.any_released(), i.modifiers.alt));
    if cx.filing.is_none() && released {
        match cx.stacking {
            Some(onto) => stack(onto, carry, keep, cx),
            None => {
                let at = halves.or(cx.motion.lift.as_ref().map(|l| l.hole.at));
                land(fill, (at, keep), carry, cx);
            }
        }
    }
}

/// Lets go on a book made ready: the two become a shelf.
fn stack(onto: BookId, carry: &Carry, keep: bool, cx: &mut Cx<'_>) {
    let Some(arranging) = cx.arranging else {
        return;
    };
    let action = ShelfAction::Stack {
        onto,
        carry: carry.clone(),
        level: arranging.level,
        keep,
    };
    cx.out
        .push(Sent::Engine(Command::Library(Action::Shelf(action))));
}

/// Lands what's carried at rest index `at`, joining this level first.
fn land(fill: &Fill<'_, '_>, (at, keep): (Option<usize>, bool), carry: &Carry, cx: &mut Cx<'_>) {
    let (Some(lift), Some(arranging), Some(at)) = (&cx.motion.lift, cx.arranging, at) else {
        return;
    };
    if lift.hole.start == Some(at) {
        return;
    }
    if !lift.native() {
        let join = carry.onto(arranging.level, keep);
        cx.out
            .extend(join.map(|a| Sent::Engine(Command::Library(a))));
    }
    let next = fill.rest.get(at).and_then(|&n| fill.items.get(n));
    let action = ShelfAction::Arrange {
        items: lift.moving.clone(),
        level: arranging.level,
        before: next.map(|i| i.pick()),
    };
    cx.out
        .push(Sent::Engine(Command::Library(Action::Shelf(action))));
    // Dropped while sorted, the page shows the order just made.
    cx.out.extend(
        arranging
            .switch
            .map(|v| Sent::Engine(Command::SetLibraryView(v))),
    );
}

/// Held near the top or bottom, the grid scrolls that way if it can.
fn scroll_near_edges(ui: &Ui, view: Rect, pointer: Pos2, (can_up, can_down): (bool, bool)) {
    const BAND: f32 = 32.0;
    let near = |gap: f32, can: bool| if can { gap.clamp(0.0, BAND) } else { 0.0 };
    let up = near(view.top() + BAND - pointer.y, can_up);
    let down = near(pointer.y - (view.bottom() - BAND), can_down);
    if up == 0.0 && down == 0.0 {
        return;
    }
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    // Up to 900 points a second, faster the closer to the edge.
    let speed = (up - down) / BAND * 900.0;
    ui.scroll_with_delta(vec2(0.0, speed * dt));
    ui.ctx().request_repaint();
}

/// The lifted card under the pointer, raised; false if none is lifted.
pub(super) fn ghost(ui: &mut Ui, cx: &mut Cx<'_>) -> bool {
    let (Some(lift), Some(grab)) = (&cx.motion.lift, cx.motion.grab) else {
        return false;
    };
    let Some(pointer) = ui.ctx().pointer_latest_pos() else {
        return false;
    };
    let count = lift.moving.len();
    let Some(item) = Item::of(cx.model, grab.pick) else {
        return false;
    };
    let size = cx.motion.card;
    let rect = Rect::from_min_size(pointer - grab.offset, size);
    cx.motion.ghost = Some(rect.min - cx.motion.origin);
    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    let layer = LayerId::new(Order::Tooltip, Id::new("library-ghost"));
    let ui = ui.new_child(egui::UiBuilder::new().layer_id(layer).max_rect(rect));
    let t = cx.t;
    // Lifted: a touch larger, on a deeper shadow.
    let lifted = rect.expand(rect.width() * 0.03);
    let bounds = Rect::from_min_size(lifted.min, vec2(lifted.width(), lifted.width() * A4));
    // A selection fans out behind the card held.
    let behind = lift.moving.iter().filter(|p| **p != grab.pick).take(2);
    let fan: Vec<Color32> = behind.map(|&p| tint(p, cx)).collect();
    for (&fill, angle) in fan.iter().zip([-0.07, 0.06]).rev() {
        fanned(ui.painter(), bounds, angle, fill, t);
    }
    let shadow = Shadow {
        offset: [0, 10],
        blur: 28,
        spread: 0,
        color: t.shadow,
    };
    ui.painter()
        .add(shadow.as_shape(bounds, CornerRadius::from(t.radius.min(6.0))));
    let name = match item {
        Item::Book(book) => {
            cover(&ui, bounds, book, cx, 1.0, false);
            book.title.value.as_str()
        }
        Item::Shelf(shelf) => {
            plate(&ui, bounds, shelf.id, 0, cx);
            shelf.name.value.as_str()
        }
        Item::Link(link) => {
            links::face(&ui, bounds, link, t);
            link.name.value.as_str()
        }
    };
    let p = ui.painter();
    let font = theme::medium(13.0);
    line(
        p,
        name,
        font,
        t.ink,
        (bounds.left(), bounds.bottom() + 8.0),
        bounds.width(),
    );
    // Alt adds a copy instead of moving it off its shelf.
    let carry = DragAndDrop::payload::<Carry>(ui.ctx());
    let keep = ui.input(|i| i.modifiers.alt) && carry.is_some_and(|c| c.off_shelf());
    let badge = match (count, keep) {
        (1, false) => None,
        (1, true) => Some("+".to_owned()),
        (n, false) => Some(n.to_string()),
        (n, true) => Some(format!("+{n}")),
    };
    if let Some(text) = badge {
        let g = p.layout_no_wrap(text, theme::medium(12.0), Color32::WHITE);
        let d = g.size().x.max(g.size().y) + 10.0;
        let dot = Rect::from_center_size(bounds.right_top(), vec2(d, d));
        p.rect_filled(dot, d / 2.0, t.accent);
        p.galley(dot.center() - g.size() / 2.0, g, Color32::WHITE);
    }
    true
}

/// The colour a card shows when only its outline is drawn.
fn tint(pick: Pick, cx: &Cx<'_>) -> Color32 {
    match Item::of(cx.model, pick) {
        Some(Item::Book(book)) => paper(book, cx.t),
        _ => cx.t.hover,
    }
}

/// A card behind the ghost, turned `angle` radians about its foot.
fn fanned(p: &Painter, bounds: Rect, angle: f32, fill: Color32, t: &theme::Tokens) {
    let pivot = bounds.center_bottom();
    let (sin, cos) = angle.sin_cos();
    let turn = |q: Pos2| {
        let d = q - pivot;
        pivot + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
    };
    let corners = [
        bounds.left_top(),
        bounds.right_top(),
        bounds.right_bottom(),
        bounds.left_bottom(),
    ]
    .map(turn);
    let drop = corners.map(|c| c + vec2(0.0, 6.0)).to_vec();
    let shade = t.shadow.gamma_multiply(0.5);
    p.add(Shape::convex_polygon(drop, shade, Stroke::NONE));
    p.add(Shape::convex_polygon(
        corners.to_vec(),
        fill,
        Stroke::new(1.0, t.line),
    ));
}
