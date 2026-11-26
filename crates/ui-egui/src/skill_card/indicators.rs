//! Named reopen buttons for selection answers, placed clear of window chrome.

use egui::{Align2, Context, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::skills::{Action, Ask};
use mareader_workspace::PaneId;

use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};

const HEIGHT: f32 = 28.0;
const GAP: f32 = 4.0;
const MAX_WIDTH: f32 = 200.0;
const MIN_WIDTH: f32 = 40.0;

struct Item<'a> {
    ask: &'a Ask,
    at: Rect,
    room: Rect,
}

struct Placed<'a> {
    ask: &'a Ask,
    rect: Rect,
}

pub(super) struct Indicators<'a> {
    items: Vec<Item<'a>>,
    bar: f32,
    reserve: f32,
}

impl<'a> Indicators<'a> {
    pub(super) fn new((bar, reserve): (f32, f32)) -> Self {
        Self {
            items: Vec::new(),
            bar,
            reserve,
        }
    }

    pub(super) fn push(&mut self, ask: &'a Ask, at: Rect, pane: Rect) {
        if let Some(room) = crate::chrome_clearance::room(pane, self.bar, self.reserve) {
            self.items.push(Item { ask, at, room });
        }
    }

    pub(super) fn show(&self, ctx: &Context, out: &mut Commands) {
        let mut panes = Vec::new();
        for item in &self.items {
            if !panes.contains(&item.ask.pane) {
                panes.push(item.ask.pane);
            }
        }
        for pane in panes {
            self.pane(ctx, pane, out);
        }
    }

    fn pane(&self, ctx: &Context, pane: PaneId, out: &mut Commands) {
        let t = Tokens::get(ctx);
        let mut placed: Vec<Placed<'_>> = Vec::new();
        let mut overflow: Vec<&Ask> = Vec::new();
        for item in self.items.iter().filter(|item| item.ask.pane == pane) {
            let font = theme::medium(t.text_small);
            let text = ctx.fonts_mut(|f| f.layout_no_wrap(item.ask.name.clone(), font, t.ink));
            let width = (text.size().x + 24.0)
                .clamp(MIN_WIDTH, MAX_WIDTH)
                .min(item.room.width());
            let used: Vec<Rect> = placed.iter().map(|p| p.rect).collect();
            if let Some(rect) = place(item.at, item.room, vec2(width, HEIGHT), &used) {
                placed.push(Placed {
                    ask: item.ask,
                    rect,
                });
            } else {
                overflow.push(item.ask);
            }
        }
        let more = if overflow.is_empty() {
            None
        } else {
            placed.pop().map(|last| {
                overflow.insert(0, last.ask);
                last.rect
            })
        };
        for p in placed {
            button(ctx, p.ask, p.rect, &t, out);
        }
        if let Some(rect) = more {
            more_button(ctx, pane, rect, &overflow, &t, out);
        }
    }
}

/// Finds the nearest clear slot, then checks the other rows and columns.
fn place(at: Rect, room: Rect, size: egui::Vec2, used: &[Rect]) -> Option<Rect> {
    if size.x < MIN_WIDTH || size.y > room.height() {
        return None;
    }
    let x = (at.right() + GAP).clamp(room.left(), room.right() - size.x);
    let y = (at.top() - size.y - GAP).clamp(room.top(), room.bottom() - size.y);
    let original = Rect::from_min_size(pos2(x, y), size);
    if used
        .iter()
        .all(|r| !r.expand(GAP / 2.0).intersects(original))
    {
        return Some(original);
    }
    let mut best: Option<(f32, Rect)> = None;
    let mut y = room.top();
    let mut checked = 0;
    while y + size.y <= room.bottom() && checked < 512 {
        let mut x = room.left();
        while x + size.x <= room.right() && checked < 512 {
            checked += 1;
            let candidate = Rect::from_min_size(pos2(x, y), size);
            if used
                .iter()
                .all(|r| !r.expand(GAP / 2.0).intersects(candidate))
            {
                let distance = candidate.center().distance_sq(original.center());
                if best.is_none_or(|(d, _)| distance < d) {
                    best = Some((distance, candidate));
                }
            }
            x += size.x + GAP;
        }
        y += size.y + GAP;
    }
    best.map(|(_, rect)| rect)
}

fn button(ctx: &Context, ask: &Ask, rect: Rect, t: &Tokens, out: &mut Commands) {
    crate::z_order::area(
        ctx,
        crate::z_order::Tier::PageCard,
        Id::new(("skill-indicator", ask.id.0)),
    )
    .fixed_pos(rect.min)
    .constrain(false)
    .show(ctx, |ui| {
        let label = format!("Open {} answer", ask.name);
        let response = draw(ui, rect.size(), &ask.name, &label, t);
        if response.clicked() {
            out.push(Sent::Engine(Command::Skill(Action::Show(ask.id))));
        }
        response.on_hover_text(label);
    });
}

fn draw(ui: &mut Ui, size: egui::Vec2, text: &str, label: &str, t: &Tokens) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if r.hovered() || r.has_focus() {
        t.hover
    } else {
        t.surface
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    ui.painter()
        .rect_stroke(rect, t.radius, Stroke::new(1.0, t.line), StrokeKind::Inside);
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        theme::medium(t.text_small),
        t.ink,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 16.0).max(0.0));
    let galley = ui.painter().layout_job(job);
    let pos = Align2::CENTER_CENTER
        .align_size_within_rect(galley.size(), rect)
        .min;
    ui.painter().galley(pos, galley, t.ink);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    r
}

fn more_button(
    ctx: &Context,
    pane: PaneId,
    rect: Rect,
    asks: &[&Ask],
    t: &Tokens,
    out: &mut Commands,
) {
    let id = Id::new(("skill-indicator-more", pane.0));
    crate::z_order::area(ctx, crate::z_order::Tier::PageCard, id)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            let label = format!("More selection AI answers ({})", asks.len());
            let r = draw(ui, rect.size(), "More…", &label, t);
            egui::Popup::from_toggle_button_response(&r)
                .id(id.with("list"))
                .sense(crate::widgets::POPUP_SENSE)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(180.0)
                .show(|ui| {
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for ask in asks {
                                let title = format!("Open {} answer", ask.name);
                                let r = ui.add(egui::Button::new(&ask.name));
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        &title,
                                    )
                                });
                                if r.clicked() {
                                    out.push(Sent::Engine(Command::Skill(Action::Show(ask.id))));
                                    ui.close();
                                }
                            }
                        });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placements_stay_in_room_and_do_not_overlap() {
        let room = Rect::from_min_size(pos2(0.0, 48.0), vec2(240.0, 200.0));
        let at = Rect::from_min_size(pos2(5.0, 0.0), vec2(40.0, 10.0));
        let one = place(at, room, vec2(80.0, HEIGHT), &[]).unwrap();
        let two = place(at, room, vec2(80.0, HEIGHT), &[one]).unwrap();
        assert!(room.contains_rect(one) && room.contains_rect(two));
        assert!(!one.intersects(two));
        assert!(place(at, room, vec2(20.0, HEIGHT), &[]).is_none());
    }
}
