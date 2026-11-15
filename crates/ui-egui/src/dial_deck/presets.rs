//! Preset gallery and inline save form for the Dial deck popover.

use egui::{
    Align, Align2, Context, CornerRadius, CursorIcon, FontId, Id, Layout, Rect, Response, RichText,
    Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_appearance::{Appearance, BaseMode, NoiseMode, Preset, group_presets};

use super::dials::blend;
use crate::appearance_menu::MenuContext;
use crate::icons::Icon;
use crate::preset_cards::{self, DeleteStyle, Draft, DraftStyle, PreviewStyle};
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;

const DRAFT_ID: &str = "dial-deck-preset-draft";
const CARD_RADIUS: f32 = 7.0;
const CARD_ASPECT: f32 = 0.72;
const CARD_NAME_H: f32 = 16.0;
const CARD_GAP: f32 = 6.0;
const GALLERY_MAX_H: f32 = 224.0;

const PREVIEW: PreviewStyle = PreviewStyle {
    radius: CARD_RADIUS,
    shrink: Vec2::new(0.18, 0.25),
    line_shares: [1.0, 0.78, 0.56],
    ink_alpha: 0.68,
    dot_offset: Vec2::new(-6.5, 6.5),
    dot_radius: 2.6,
};

const DELETE: DeleteStyle = DeleteStyle {
    id_salt: "dial-deck-delete-preset",
    offset: Vec2::new(-5.0, 5.0),
    size: 15.0,
    icon_size: 8.5,
    use_paper_fill: true,
    hover_cursor: true,
    tooltip: true,
};

/// Drops any half-typed preset draft once the popover closes.
pub fn forget_draft(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Draft>(Id::new(DRAFT_ID)));
}

pub(super) fn presets_tab(
    ui: &mut Ui,
    cx: &MenuContext<'_>,
    a: &mut Appearance,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new(DRAFT_ID);
    let mut draft = ui.data(|d| d.get_temp::<Draft>(id)).unwrap_or_default();

    header_row(ui, cx.presets, a, &mut draft, &t);
    if draft.open {
        ui.add_space(8.0);
        draft_card(ui, cx.user, a, &mut draft, out, &t);
    }
    ui.data_mut(|d| d.insert_temp(id, draft));

    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .id_salt("dial-deck-presets-scroll")
        .max_height(GALLERY_MAX_H)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let groups = group_presets(cx.presets);
            let show_headings = groups.len() > 1;
            for (idx, group) in groups.iter().enumerate() {
                if idx > 0 {
                    ui.add_space(8.0);
                }
                if show_headings {
                    let heading = RichText::new(group.name)
                        .font(theme::medium(11.5))
                        .color(t.muted);
                    ui.label(heading);
                    ui.add_space(4.0);
                }
                preset_rows(ui, cx, &group.presets, a, out, &t);
            }
        });
}

fn header_row(ui: &mut Ui, presets: &[Preset], a: &Appearance, draft: &mut Draft, t: &Tokens) {
    let matched = presets.iter().find(|p| p.appearance == *a);
    ui.horizontal(|ui| {
        ui.set_min_height(26.0);
        match matched {
            Some(p) => {
                ui.label(RichText::new("Active:").size(11.5).color(t.muted));
                ui.label(
                    RichText::new(&p.name)
                        .font(theme::medium(12.0))
                        .color(t.accent),
                );
            }
            None => {
                let text = format!("{} presets", presets.len());
                ui.label(RichText::new(text).size(11.5).color(t.muted));
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if save_trigger(ui, draft.open, t).clicked() {
                draft.open = !draft.open;
                draft.focus = draft.open;
            }
        });
    });
}

fn save_trigger(ui: &mut Ui, open: bool, t: &Tokens) -> Response {
    let label = "Save look";
    let font = theme::medium(11.5);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, t.ink);
    let size = vec2(galley.size().x + 28.0, 25.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let fill = if open {
        t.accent_soft
    } else if hot {
        t.hover
    } else {
        blend(t.paper, t.ink, if t.dark { 0.07 } else { 0.04 })
    };
    let stroke = if open {
        Stroke::new(1.0, t.accent)
    } else {
        Stroke::new(1.0, t.line)
    };
    let fg = if open { t.accent } else { t.ink };
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        fill,
        stroke,
        StrokeKind::Inside,
    );
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + 11.0, rect.center().y), Vec2::splat(11.0));
    Icon::Plus.image(ui, 11.0).tint(fg).paint_at(ui, icon_rect);
    ui.painter().text(
        pos2(rect.left() + 20.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::medium(11.5),
        fg,
    );
    resp.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::Button,
            true,
            open,
            "Save current look as preset",
        )
    });
    widgets::tip(&resp, "Save current look as preset", None);
    resp
}

fn draft_card(
    ui: &mut Ui,
    user: &[Preset],
    look: &Appearance,
    d: &mut Draft,
    out: &mut Commands,
    t: &Tokens,
) {
    let recess = blend(t.paper, t.surface, if t.dark { 0.45 } else { 0.55 });
    let style = DraftStyle {
        fill: Some(recess),
        radius: 10,
        margin: 10.0,
        button_height: 26.0,
        muted_when_empty: true,
    };
    preset_cards::draft_form(ui, user, look, d, out, &style, t);
}

fn preset_rows(
    ui: &mut Ui,
    cx: &MenuContext<'_>,
    presets: &[&Preset],
    a: &mut Appearance,
    out: &mut Commands,
    t: &Tokens,
) {
    let cols = if presets.len() > 5 && presets.len() % 5 == 1 {
        4
    } else {
        5
    };
    let room = ui.available_width();
    let w = (room - CARD_GAP * (cols - 1) as f32) / cols as f32;
    let h = w * CARD_ASPECT + CARD_NAME_H;
    for (row_idx, row) in presets.chunks(cols).enumerate() {
        if row_idx > 0 {
            ui.add_space(CARD_GAP);
        }
        let used = row.len() as f32 * w + (row.len().max(1) - 1) as f32 * CARD_GAP;
        let (band, _) = ui.allocate_exact_size(vec2(room, h), Sense::hover());
        let mut x = band.left() + (band.width() - used) * 0.5;
        for p in row {
            let cell = Rect::from_min_size(pos2(x, band.top()), vec2(w, h));
            let mut child = ui.new_child(UiBuilder::new().max_rect(cell));
            preset_card(&mut child, cx, p, a, out, t);
            x += w + CARD_GAP;
        }
    }
}

fn preset_card(
    ui: &mut Ui,
    cx: &MenuContext<'_>,
    p: &Preset,
    a: &mut Appearance,
    out: &mut Commands,
    t: &Tokens,
) {
    let selected = p.appearance == *a;
    let w = ui.available_width();
    let size = vec2(w, w * CARD_ASPECT + CARD_NAME_H);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let page = Rect::from_min_size(rect.min, vec2(w, w * CARD_ASPECT));
    let hot = ui.rect_contains_pointer(rect);
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }

    preset_cards::paint_page(ui, cx.gpu, page, &p.appearance, &PREVIEW);
    let border = match (selected, hot) {
        (true, _) => Stroke::new(2.0, t.accent),
        (false, true) => Stroke::new(1.5, t.muted),
        (false, false) => Stroke::new(1.0, t.line),
    };
    let radius = CornerRadius::same(CARD_RADIUS as u8);
    ui.painter()
        .rect_stroke(page, radius, border, StrokeKind::Inside);

    let name_font = if selected {
        theme::medium(10.5)
    } else {
        FontId::proportional(10.5)
    };
    let name_col = if selected { t.accent } else { t.ink };
    let galley = preset_cards::elided(ui, &p.name, name_font, name_col, w + CARD_GAP - 2.0);
    let at = pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.bottom() - galley.size().y,
    );
    ui.painter().galley(at, galley, name_col);

    let label = p.name.clone();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &label));

    let own = cx.user.iter().any(|u| u.id == p.id);
    if own {
        let del = preset_cards::delete_badge(ui, page, p, hot, &DELETE, t);
        if del.clicked() {
            out.push(Sent::Engine(Command::DeletePreset(p.id.clone())));
            return;
        }
    }
    if resp.clicked() {
        *a = p.appearance;
    }
    widgets::tip(&resp, &p.name, None);
}

pub(crate) fn noise_label(n: NoiseMode) -> &'static str {
    match n {
        NoiseMode::Off => "Off",
        NoiseMode::Static => "Static",
        NoiseMode::Animated => "Animated",
    }
}

pub(crate) fn mode_icon(b: BaseMode) -> Icon {
    match b {
        BaseMode::Light => Icon::Light,
        BaseMode::Dark => Icon::Dark,
        BaseMode::Dim => Icon::Dim,
        BaseMode::Black => Icon::Black,
    }
}
