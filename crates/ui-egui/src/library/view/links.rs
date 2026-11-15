//! Link cards and rows: pointers that light what they point at.

use egui::{FontId, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use mareader_library::{Link, LinkTo};

use super::{
    A4, Cx, ROW_GAP, ROW_H, ROW_INSET, backing, card_ring, lift, line, lit_ring, long_press,
    picking,
};
use crate::icons::Icon;
use crate::library::menus;
use crate::theme::{self, Tokens};
use crate::ui_command::Sent;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_library::picks::Pick;

/// The line under a link's name.
fn says(link: &Link) -> &'static str {
    match link.target {
        LinkTo::Book(_) => "Link · opens the book where it is",
        LinkTo::Shelf(_) => "Link · opens the shelf where it is",
    }
}

/// What hovering a link tells.
fn tip(link: &Link) -> &'static str {
    match link.target {
        LinkTo::Book(_) => "A pointer at a book, not a copy of one",
        LinkTo::Shelf(_) => "A pointer at a shelf, not a second one",
    }
}

/// What following a link lights.
pub fn target(link: &Link) -> Pick {
    match link.target {
        LinkTo::Book(id) => Pick::Book(id),
        LinkTo::Shelf(id) => Pick::Shelf(id),
    }
}

/// A link's card or row: following it lights its target, never opens it.
fn hit(ui: &mut Ui, rect: Rect, link: &Link, cx: &mut Cx<'_>) -> egui::Response {
    let pick = Pick::Link(link.id);
    let resp = ui.interact(
        rect,
        ui.id().with(("link", link.id)),
        Sense::click_and_drag(),
    );
    let picked = cx.picks.contains(pick);
    let name = link.name.value.as_str();
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, picked, name));
    let held = long_press(ui, &resp, pick, cx);
    if resp.clicked() && !held && !picking(ui, pick, cx) {
        cx.out
            .push(Sent::Engine(Command::Library(Action::Reveal(target(link)))));
    }
    if resp.drag_started() {
        lift(&resp, pick, cx);
    }
    crate::widgets::context_menu(&resp, |ui| match picked {
        true => menus::picks_menu(ui, cx.picks, cx.t, cx.out),
        false => menus::link_menu(ui, link, cx.t, cx.out),
    });
    resp.on_hover_text(tip(link))
}

/// A link's face: its name on plain paper, the link mark in a corner.
pub fn face(ui: &Ui, bounds: Rect, link: &Link, t: &Tokens) {
    let radius = t.radius.min(6.0);
    let p = ui.painter();
    p.rect_filled(bounds, radius, t.surface);
    let edge = Stroke::new(1.0, t.line);
    p.rect_stroke(bounds, radius, edge, StrokeKind::Inside);
    let pad = (bounds.width() * 0.1).max(6.0);
    let font = theme::medium((bounds.width() * 0.09).clamp(10.0, 15.0));
    let job = egui::text::LayoutJob::simple(
        link.name.value.clone(),
        font,
        t.muted,
        bounds.width() - 2.0 * pad,
    );
    let g = p.layout_job(job);
    p.galley(bounds.min + vec2(pad, pad), g, t.muted);
    let d = (bounds.width() * 0.16).clamp(18.0, 26.0);
    let badge = Rect::from_min_size(
        pos2(bounds.right() - d - 6.0, bounds.bottom() - d - 6.0),
        vec2(d, d),
    );
    p.rect_filled(badge, d / 2.0, t.accent.gamma_multiply(0.16));
    let mark = Rect::from_center_size(badge.center(), vec2(d * 0.5, d * 0.5));
    Icon::Link.paint(ui, mark, t.accent);
}

pub(super) fn card(ui: &mut Ui, rect: Rect, link: &Link, cx: &mut Cx<'_>) {
    let resp = hit(ui, rect, link, cx);
    let t = cx.t;
    let pick = Pick::Link(link.id);
    let picked = cx.picks.contains(pick);
    let pad = rect.expand2(vec2(8.0, 8.0));
    backing(ui, pad, t.radius_lg, (resp.hovered(), picked, false), t);
    lit_ring(ui, pad, t.radius_lg, pick, cx);
    card_ring(&resp, t);
    let bounds = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width() * A4));
    face(ui, bounds, link, t);
    if cx.picks.active() {
        menus::mark(ui, bounds.min + vec2(16.0, 16.0), picked, t);
    }
    let p = ui.painter();
    let width = rect.width();
    let y = bounds.bottom() + 8.0;
    let name = link.name.value.as_str();
    let y = y + line(p, name, theme::medium(13.0), t.ink, (rect.left(), y), width);
    let small = FontId::proportional(12.0);
    line(p, says(link), small, t.muted, (rect.left(), y + 1.0), width);
}

pub(super) fn row(ui: &mut Ui, link: &Link, indent: f32, cx: &mut Cx<'_>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
    let resp = hit(ui, rect, link, cx);
    let t = cx.t;
    let pick = Pick::Link(link.id);
    let picked = cx.picks.contains(pick);
    let ground = rect.shrink2(ROW_GAP);
    backing(ui, ground, t.radius, (resp.hovered(), picked, false), t);
    lit_ring(ui, ground, t.radius, pick, cx);
    let thumb = Rect::from_center_size(
        pos2(rect.left() + ROW_INSET + indent + 17.0, rect.center().y),
        vec2(34.0, 34.0 * A4),
    );
    // While picking, the mark takes the icon's place.
    match cx.picks.active() {
        true => menus::mark(ui, thumb.center(), picked, t),
        false => {
            let icon = Rect::from_center_size(thumb.center(), vec2(20.0, 20.0));
            Icon::Link.paint(ui, icon, t.muted);
        }
    }
    let p = ui.painter();
    let x = thumb.right() + 14.0;
    let wide = (rect.right() - ROW_INSET - x).max(40.0);
    let name = link.name.value.as_str();
    let top = (x, rect.center().y - 18.0);
    line(p, name, theme::medium(14.0), t.ink, top, wide);
    let small = FontId::proportional(12.0);
    line(
        p,
        says(link),
        small,
        t.muted,
        (x, rect.center().y + 2.0),
        wide,
    );
    let rule = Stroke::new(1.0, t.line);
    p.hline(rect.x_range().shrink(ROW_INSET), rect.bottom(), rule);
}
