//! A fold keeping chosen rows while shut, other rows growing in.

use egui::{CornerRadius, Id, Layout, Rect, Sense, Ui, UiBuilder, Vec2, pos2, vec2};

use crate::icons::Icon;
use crate::theme::{self, Tokens};

/// The title row's height.
pub const TITLE_H: f32 = 26.0;

/// How open a fold is: as last clicked, and its eased share, 0 to 1.
#[derive(Clone, Copy, Debug)]
pub struct Fold {
    pub open: bool,
    pub how: f32,
}

/// Whether the fold `id` is open.
pub fn is_open(ui: &Ui, id: Id) -> bool {
    ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false)
}

/// The fold's title row, with a count pill when given; a click turns it.
pub fn title(ui: &mut Ui, id: Id, name: &str, count: Option<usize>) -> Fold {
    let t = Tokens::get(ui.ctx());
    let mut open = is_open(ui, id);
    let size = vec2(ui.available_width(), TITLE_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    if r.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let kind = egui::WidgetType::CollapsingHeader;
    r.widget_info(|| egui::WidgetInfo::selected(kind, true, open, name));
    let hot = r.hovered();
    let painter = ui.painter();
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        painter.rect_filled(rect, t.radius, t.hover);
    }
    let y = rect.center().y;
    let ink = if hot { t.ink } else { t.muted };
    let font = theme::medium(t.text_small);
    let left = egui::Align2::LEFT_CENTER;
    painter.text(pos2(rect.left() + 10.0, y), left, name, font, ink);
    let how = ui.ctx().animate_bool_responsive(id, open);
    let turn = how * std::f32::consts::FRAC_PI_2;
    let chevron = Rect::from_center_size(pos2(rect.right() - 14.0, y), Vec2::splat(12.0));
    let icon = Icon::ChevronRight.image(ui, 12.0).tint(ink);
    icon.rotate(turn, Vec2::splat(0.5)).paint_at(ui, chevron);
    if let Some(count) = count {
        pill(ui, count, chevron.left() - 6.0, y);
    }
    Fold { open, how }
}

/// A small count pill on the right of the title.
fn pill(ui: &Ui, count: usize, right: f32, y: f32) {
    let t = Tokens::get(ui.ctx());
    let painter = ui.painter();
    let small = egui::FontId::proportional(t.text_small - 1.0);
    let galley = painter.layout_no_wrap(count.to_string(), small, t.muted);
    let size = vec2(galley.size().x + 10.0, galley.size().y + 2.0);
    let pill = Rect::from_center_size(pos2(right - size.x / 2.0, y), size);
    painter.rect_filled(pill, CornerRadius::same(255), t.line.gamma_multiply(0.7));
    painter.galley(pill.center() - galley.size() / 2.0, galley, t.muted);
}

/// The fold's rows: kept rows always show; others grow in with the fold.
pub fn rows<T>(
    ui: &mut Ui,
    fold: Fold,
    (row_h, indent): (f32, f32),
    items: impl IntoIterator<Item = (T, bool)>,
    mut draw: impl FnMut(&mut Ui, T, bool),
) {
    let gap = ui.spacing().item_spacing.y;
    let room = ui.available_rect_before_wrap();
    let inner = room.with_min_x(room.left() + indent);
    let layout = Layout::top_down(egui::Align::Min);
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(layout), |ui| {
        for (item, kept) in items {
            let f = if kept { 1.0 } else { fold.how };
            if f >= 1.0 {
                draw(ui, item, true);
                continue;
            }
            // Partway, a row grows in, clipped and fading.
            let height = ((row_h + gap) * f - gap).max(0.0);
            if height <= 0.0 {
                continue;
            }
            let size = vec2(ui.available_width(), height);
            let (slot, _) = ui.allocate_exact_size(size, Sense::hover());
            let whole = Rect::from_min_size(slot.min, vec2(slot.width(), row_h));
            let mut row = ui.new_child(UiBuilder::new().max_rect(whole).layout(layout));
            row.set_clip_rect(slot.intersect(ui.clip_rect()));
            row.multiply_opacity(f);
            draw(&mut row, item, false);
        }
    });
}
