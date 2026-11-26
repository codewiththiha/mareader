//! Page-edge stacks for closed selection answers.

use egui::{Align2, Context, Id, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::skills::{Action, Anchor, Ask, AskId};
use mareader_workspace::PaneId;

use crate::documents::DocumentsView;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};

const HEIGHT: f32 = 28.0;
const GAP: f32 = 4.0;
const INSET: f32 = 8.0;
const MAX_WIDTH: f32 = 180.0;
const MIN_WIDTH: f32 = 40.0;

struct Item<'a> {
    ask: &'a Ask,
    page: usize,
    room: Rect,
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

    pub(super) fn push(&mut self, ask: &'a Ask, panes: &[(PaneId, Rect)], docs: DocumentsView<'_>) {
        if docs
            .reading(ask.pane)
            .is_none_or(|r| r.doc.get() != ask.doc)
        {
            return;
        }
        let Some(pane) = panes
            .iter()
            .find(|(id, _)| *id == ask.pane)
            .map(|(_, r)| *r)
        else {
            return;
        };
        let Some(clear) = crate::chrome_clearance::room(pane, self.bar, self.reserve) else {
            return;
        };
        let Anchor::Text(spans) = &ask.anchor else {
            return;
        };
        for sheet in docs.sheets(ask.pane) {
            if !spans.iter().any(|(page, _)| *page == sheet.page) {
                continue;
            }
            let room = sheet.rect.intersect(sheet.shown).intersect(clear);
            if room.is_positive() {
                self.items.push(Item {
                    ask,
                    page: sheet.page,
                    room,
                });
                break;
            }
        }
    }

    pub(super) fn show(&self, ctx: &Context, out: &mut Commands) -> Vec<AskId> {
        let mut hovered = Vec::new();
        let mut pages = Vec::new();
        for item in &self.items {
            let key = (item.ask.pane, item.page);
            if !pages.contains(&key) {
                pages.push(key);
                self.page(ctx, key, out, &mut hovered);
            }
        }
        hovered
    }

    fn page(
        &self,
        ctx: &Context,
        key: (PaneId, usize),
        out: &mut Commands,
        hovered: &mut Vec<AskId>,
    ) {
        let items: Vec<_> = self
            .items
            .iter()
            .filter(|i| (i.ask.pane, i.page) == key)
            .collect();
        let room = items[0].room;
        let bubbles: Vec<_> = crate::page_ai::paint::bubble_rects(ctx)
            .into_iter()
            .filter(|r| room.intersects(*r))
            .collect();
        let t = Tokens::get(ctx);
        let width = items
            .iter()
            .map(|i| {
                ctx.fonts_mut(|f| {
                    f.layout_no_wrap(i.ask.name.clone(), theme::medium(t.text_small), t.ink)
                })
                .size()
                .x + 24.0
            })
            .fold(MIN_WIDTH, f32::max)
            .min(MAX_WIDTH)
            .min(room.width() - INSET * 2.0);
        if width < MIN_WIDTH {
            return;
        }
        let top = bubbles
            .iter()
            .fold(room.top() + INSET, |y, b| y.max(b.bottom() + GAP));
        let (right, top) = if top + HEIGHT <= room.bottom() - INSET {
            (room.right() - INSET, top)
        } else if let Some(left) = bubbles.iter().map(|r| r.left()).reduce(f32::min) {
            (left - GAP, room.top() + INSET)
        } else {
            return;
        };
        if right - width < room.left() {
            return;
        }
        let slots = (((room.bottom() - INSET - top + GAP) / (HEIGHT + GAP)).floor() as usize)
            .min(items.len());
        if slots == 0 {
            return;
        }
        let direct = if items.len() > slots {
            slots - 1
        } else {
            slots
        };
        for (i, item) in items.iter().take(direct).enumerate() {
            let rect = Rect::from_min_size(
                pos2(right - width, top + i as f32 * (HEIGHT + GAP)),
                vec2(width, HEIGHT),
            );
            if button(ctx, item.ask, rect, &t, out) {
                hovered.push(item.ask.id);
            }
        }
        if direct < items.len() {
            let rect = Rect::from_min_size(
                pos2(right - width, top + direct as f32 * (HEIGHT + GAP)),
                vec2(width, HEIGHT),
            );
            more_button(ctx, key, rect, &items[direct..], &t, out, hovered);
        }
    }
}

fn button(ctx: &Context, ask: &Ask, rect: Rect, t: &Tokens, out: &mut Commands) -> bool {
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
        let lit = response.hovered() || response.has_focus();
        if response.clicked() {
            out.push(Sent::Engine(Command::Skill(Action::Show(ask.id))));
        }
        response.on_hover_text(label);
        lit
    })
    .inner
}

fn draw(ui: &mut Ui, size: egui::Vec2, text: &str, label: &str, t: &Tokens) -> egui::Response {
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let fill = if r.hovered() || r.has_focus() {
        t.hover.blend(t.surface)
    } else {
        t.surface
    };
    ui.painter()
        .rect_filled(rect.translate(vec2(0.0, 1.0)), t.radius, t.shadow);
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
    key: (PaneId, usize),
    rect: Rect,
    items: &[&Item<'_>],
    t: &Tokens,
    out: &mut Commands,
    hovered: &mut Vec<AskId>,
) {
    let id = Id::new(("skill-indicator-more", key.0.0, key.1));
    crate::z_order::area(ctx, crate::z_order::Tier::PageCard, id)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            let label = format!("More selection AI answers ({})", items.len());
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
                            for item in items {
                                let title = format!("Open {} answer", item.ask.name);
                                let r = ui.add(egui::Button::new(&item.ask.name));
                                r.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        true,
                                        &title,
                                    )
                                });
                                if r.hovered() || r.has_focus() {
                                    hovered.push(item.ask.id);
                                }
                                if r.clicked() {
                                    out.push(Sent::Engine(Command::Skill(Action::Show(
                                        item.ask.id,
                                    ))));
                                    ui.close();
                                }
                            }
                        });
                });
        });
}
