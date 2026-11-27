//! The reader's way back to the word that opened it.

use egui::{Context, Frame, Id, Margin, Rect, RichText, Stroke, vec2};
use mareader_engine::space::Filter;
use mareader_library::BookId;

use crate::activity::stack;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

pub(super) struct Return {
    pub book: BookId,
    pub row: u128,
    pub filter: Filter,
    pub cards: bool,
    started: Option<f64>,
}

impl Return {
    pub fn new(book: BookId, row: u128, filter: Filter, cards: bool, _started: f64) -> Self {
        Self {
            book,
            row,
            filter,
            cards,
            started: None,
        }
    }
}

pub(super) fn show(ctx: &Context, back: &mut Return, pane: Rect, bar: bool) -> bool {
    let t = Tokens::get(ctx);
    let now = ctx.input(|i| i.time);
    let bottom = pane.bottom()
        - if bar {
            crate::page_bar::BAR_H + 12.0
        } else {
            crate::page_bar::COUNTER_INSET
        };
    let mut pos = egui::pos2(pane.left() + 12.0, bottom - 36.0);
    if stack::under(ctx, pos + vec2(18.0, 18.0)) {
        pos.y -= 46.0;
    }
    let id = Id::new("vocab-return");
    let hovered = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let started = *back.started.get_or_insert(now);
    let expanded = now < started + 5.0 || hovered;
    if now < started + 5.0 {
        ctx.request_repaint_after_secs((started + 5.0 - now).min(0.25) as f32);
    }
    let shown = z_order::area(ctx, Tier::PageCard, id)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            Frame::new()
                .fill(t.card)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(18)
                .inner_margin(Margin::symmetric(8, 5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let icon = Icon::ChevronLeft.image(ui, 18.0);
                        let text = if expanded { "Go back to vocabs" } else { "" };
                        let button =
                            egui::Button::image_and_text(icon, RichText::new(text).color(t.ink))
                                .frame(false);
                        let response = ui.add(button);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Go back to vocabs",
                            )
                        });
                        response.clicked()
                    })
                    .inner
                })
                .inner
        });
    let over = shown.response.contains_pointer();
    ctx.data_mut(|d| d.insert_temp(id, over));
    shown.inner
}
