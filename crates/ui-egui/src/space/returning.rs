//! The reader's way back to the word that opened it.

use egui::{Context, Id, Rect, Sense, Stroke, StrokeKind, vec2};
use mareader_engine::space::{Filter, NotesFilter};
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
    pub notes: Option<NotesFilter>,
    started: Option<f64>,
}

impl Return {
    pub fn new(book: BookId, row: u128, filter: Filter, cards: bool, _started: f64) -> Self {
        Self {
            book,
            row,
            filter,
            cards,
            notes: None,
            started: None,
        }
    }

    pub fn notes(book: BookId, row: u128, filter: NotesFilter) -> Self {
        let mut back = Self::new(book, row, Filter::default(), false, 0.0);
        back.notes = Some(filter);
        back
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
    let ceiling = stack::top(ctx, pane).map(|top| top - 42.0);
    if let Some(y) = ceiling {
        pos.y = pos.y.min(y);
    }
    let id = Id::new("vocab-return");
    let secs = crate::motion::secs(ctx, 0.2);
    pos.y = ctx.animate_value_with_time(id.with("y"), pos.y, secs);
    if let Some(y) = ceiling {
        pos.y = pos.y.min(y);
    }
    let hovered = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let started = *back.started.get_or_insert(now);
    let expanded = now < started + 5.0 || hovered;
    if now < started + 5.0 {
        ctx.request_repaint_after_secs((started + 5.0 - now).min(0.25) as f32);
    }
    let amount =
        ctx.animate_bool_with_time(id.with("expand"), expanded, crate::motion::secs(ctx, 0.25));
    let label = if back.notes.is_some() {
        "Go back to notes"
    } else {
        "Go back to vocabs"
    };
    let shown = z_order::area(ctx, Tier::PageCard, id)
        .fixed_pos(pos)
        .show(ctx, |ui| {
            let galley = ui.painter().layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(t.text_small + 1.0),
                t.ink,
            );
            let width = 36.0 + (galley.size().x + 6.0) * amount;
            let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
            ui.painter().rect(
                rect,
                18.0,
                t.card,
                Stroke::new(1.0, t.line),
                StrokeKind::Inside,
            );
            let icon =
                Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), vec2(18.0, 18.0));
            Icon::ChevronLeft.paint(ui, icon, t.ink);
            if amount > 0.0 {
                let at = egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0);
                ui.painter()
                    .with_clip_rect(rect.shrink2(vec2(4.0, 0.0)))
                    .galley_with_override_text_color(at, galley, t.ink.gamma_multiply(amount));
            }
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            response.clicked()
        });
    crate::hover_guard::protect(ctx, shown.response.rect, true, true);
    let over = shown.response.contains_pointer();
    ctx.data_mut(|d| d.insert_temp(id, over));
    shown.inner
}
