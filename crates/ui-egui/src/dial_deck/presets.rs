//! Preset gallery and inline save form for the Dial deck popover.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Align2, Button, Color32, Context, CornerRadius, CursorIcon, FontId, Galley, Id, Key,
    Layout, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetType, pos2, vec2,
};
use mareader_appearance::{
    Appearance, BaseMode, FilmGrain, NoiseMode, Preset, group_presets, section_names,
};

use super::dials::blend;
use crate::appearance_menu::MenuContext;
use crate::gpu::{Anchor, Gpu, GrainCallback, PagesCallback, Quad, crawl_offset};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;

const DRAFT_ID: &str = "dial-deck-preset-draft";
const CARD_ZOOM: f32 = 0.34;
const CARD_RADIUS: f32 = 7.0;
const CARD_ASPECT: f32 = 0.72;
const CARD_NAME_H: f32 = 16.0;
const CARD_GAP: f32 = 6.0;
const GALLERY_MAX_H: f32 = 224.0;

#[derive(Clone, Default)]
struct Draft {
    open: bool,
    name: String,
    group: String,
    focus: bool,
}

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
    let frame = egui::Frame::new()
        .fill(recess)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(10.0);
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let name = ui.add(
            egui::TextEdit::singleline(&mut d.name)
                .hint_text("Preset name")
                .desired_width(f32::INFINITY),
        );
        widgets::name(&name, "Preset name");
        if std::mem::take(&mut d.focus) {
            name.request_focus();
        }
        let group = ui.add(
            egui::TextEdit::singleline(&mut d.group)
                .hint_text("Section (optional)")
                .desired_width(f32::INFINITY),
        );
        widgets::name(&group, "Section");
        let sections = section_names(user);
        if !sections.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for s in sections {
                    if ui.selectable_label(d.group.trim() == s, s).clicked() {
                        d.group = s.to_owned();
                    }
                }
            });
        }
        let named = !d.name.trim().is_empty();
        let left = name.lost_focus() || group.lost_focus();
        let enter = left && ui.input(|i| i.key_pressed(Key::Enter));
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let (mut save, mut cancel) = (false, false);
        // A blank name keeps the save button inactive.
        let primary = if named {
            widgets::primary_button(ui, None, "Save", t.accent, t)
        } else {
            Button::new(RichText::new("Save").color(t.ink))
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius)
        };
        ui.columns(2, |cols| {
            if let [l, r] = cols {
                save = l
                    .add_enabled_ui(named, |ui| {
                        ui.add_sized([ui.available_width(), 26.0], primary)
                    })
                    .inner
                    .clicked();
                let size = [r.available_width(), 26.0];
                cancel = r.add_sized(size, Button::new("Cancel")).clicked();
            }
        });
        if named && (save || enter) {
            out.push(Sent::Engine(Command::SavePreset {
                name: d.name.trim().to_owned(),
                group: d.group.trim().to_owned(),
                look: *look,
            }));
            *d = Draft::default();
        } else if cancel || escape {
            *d = Draft::default();
        }
    });
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

    paint_page_preview(ui, cx.gpu, page, &p.appearance);
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
    let galley = elided(ui, &p.name, name_font, name_col, w + CARD_GAP - 2.0);
    let at = pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.bottom() - galley.size().y,
    );
    ui.painter().galley(at, galley, name_col);

    let label = p.name.clone();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &label));

    let own = cx.user.iter().any(|u| u.id == p.id);
    if own {
        let del = delete_badge(ui, page, p, hot, t);
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

fn delete_badge(ui: &mut Ui, page: Rect, p: &Preset, card_hot: bool, t: &Tokens) -> Response {
    let c = page.right_top() + vec2(-5.0, 5.0);
    let rect = Rect::from_center_size(c, Vec2::splat(15.0));
    let id = ui.id().with(("dial-deck-delete-preset", &p.id));
    let resp = ui.interact(rect, id, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if card_hot || resp.hovered() || resp.has_focus() {
        let painter = ui.painter();
        painter.circle(c, 7.5, t.paper, Stroke::new(1.0, t.line));
        let fg = if resp.hovered() { t.ink } else { t.muted };
        let icon_rect = Rect::from_center_size(c, Vec2::splat(8.5));
        Icon::Close.image(ui, 8.5).tint(fg).paint_at(ui, icon_rect);
    }
    let label = format!("Delete {}", p.name);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    widgets::tip(&resp, &label, None);
    resp
}

fn paint_page_preview(ui: &Ui, gpu: Option<&Gpu>, page: Rect, look: &Appearance) {
    let tokens = look.ui_tokens();
    let rgb = |v: mareader_appearance::Rgb| Color32::from_rgb(v.0, v.1, v.2);
    let painter = ui.painter();
    let clip = page.intersect(ui.clip_rect());
    match gpu {
        Some(gpu) => {
            let anchor = Anchor {
                origin: page.min,
                zoom: CARD_ZOOM,
            };
            let mut quad = Quad::backdrop(page, CARD_RADIUS, look.page_tone(), anchor);
            quad.clip = clip;
            painter.add(PagesCallback::shape(gpu.pass.clone(), page, vec![quad]));
        }
        None => {
            let r = CornerRadius::same(CARD_RADIUS as u8);
            painter.rect_filled(page, r, rgb(tokens.paper));
        }
    }
    let inner = page.shrink2(vec2(page.width() * 0.18, page.height() * 0.25));
    for (idx, share) in [1.0_f32, 0.78, 0.56].into_iter().enumerate() {
        let y = inner.top() + idx as f32 * inner.height() * 0.5;
        let origin = pos2(inner.left(), y - 1.0);
        let line = Rect::from_min_size(origin, vec2(inner.width() * share, 2.0));
        painter.rect_filled(
            line,
            CornerRadius::same(1),
            rgb(tokens.ink).gamma_multiply(0.68),
        );
    }
    painter.circle_filled(page.right_top() + vec2(-6.5, 6.5), 2.6, rgb(tokens.accent));
    if let (Some(gpu), Some(grain)) = (gpu, FilmGrain::of(look)) {
        let offset = crawl_offset(ui.ctx(), grain);
        let card = (page, CARD_RADIUS);
        painter.add(GrainCallback::card(gpu.grain.clone(), card, grain, offset));
    }
}

fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping {
        max_width: width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
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
