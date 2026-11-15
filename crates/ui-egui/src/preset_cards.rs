//! Shared preset preview cards, delete badges, and inline save form.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Button, Color32, CornerRadius, CursorIcon, FontId, Galley, Key, Rect, Response, RichText,
    Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_appearance::{Appearance, FilmGrain, Preset, section_names};
use mareader_engine::command::Command;

use crate::gpu::{Anchor, Gpu, GrainCallback, PagesCallback, Quad, crawl_offset};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// A preset card's texture against a page's, as mareader's preview.
const CARD_ZOOM: f32 = 0.34;

/// A preset being named; it lives in egui's memory while the form shows.
#[derive(Clone, Default)]
pub(crate) struct Draft {
    pub open: bool,
    pub name: String,
    pub group: String,
    pub focus: bool,
}

/// Visual geometry for a mini page preview card.
pub(crate) struct PreviewStyle {
    pub radius: f32,
    pub shrink: Vec2,
    pub line_shares: [f32; 3],
    pub ink_alpha: f32,
    pub dot_offset: Vec2,
    pub dot_radius: f32,
}

/// Visual styling for the inline preset save form.
pub(crate) struct DraftStyle {
    pub fill: Option<Color32>,
    pub radius: u8,
    pub margin: f32,
    pub button_height: f32,
    pub muted_when_empty: bool,
}

/// Visual styling for a user preset's corner delete badge.
pub(crate) struct DeleteStyle {
    pub id_salt: &'static str,
    pub offset: Vec2,
    pub size: f32,
    pub icon_size: f32,
    pub use_paper_fill: bool,
    pub hover_cursor: bool,
    pub tooltip: bool,
}

/// Paints a mini page preview in the look's own paper, texture, and grain.
pub(crate) fn paint_page(
    ui: &Ui,
    gpu: Option<&Gpu>,
    page: Rect,
    look: &Appearance,
    style: &PreviewStyle,
) {
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
            let mut quad = Quad::backdrop(page, style.radius, look.page_tone(), anchor);
            quad.clip = clip;
            painter.add(PagesCallback::shape(gpu.pass.clone(), page, vec![quad]));
        }
        None => {
            let r = CornerRadius::same(style.radius as u8);
            painter.rect_filled(page, r, rgb(tokens.paper));
        }
    }
    let inner = page.shrink2(vec2(
        page.width() * style.shrink.x,
        page.height() * style.shrink.y,
    ));
    let ink = rgb(tokens.ink).gamma_multiply(style.ink_alpha);
    for (idx, share) in style.line_shares.into_iter().enumerate() {
        let y = inner.top() + idx as f32 * inner.height() * 0.5;
        let origin = pos2(inner.left(), y - 1.0);
        let line = Rect::from_min_size(origin, vec2(inner.width() * share, 2.0));
        painter.rect_filled(line, CornerRadius::same(1), ink);
    }
    painter.circle_filled(
        page.right_top() + style.dot_offset,
        style.dot_radius,
        rgb(tokens.accent),
    );
    if let (Some(gpu), Some(grain)) = (gpu, FilmGrain::of(look)) {
        let offset = crawl_offset(ui.ctx(), grain);
        let card = (page, style.radius);
        painter.add(GrainCallback::card(gpu.grain.clone(), card, grain, offset));
    }
}

/// Lays out a single-line elided label fitting within `width`.
pub(crate) fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping {
        max_width: width.max(8.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Paints and interacts with a user preset's corner delete badge.
pub(crate) fn delete_badge(
    ui: &mut Ui,
    page: Rect,
    preset: &Preset,
    visible: bool,
    style: &DeleteStyle,
    t: &Tokens,
) -> Response {
    let center = page.right_top() + style.offset;
    let rect = Rect::from_center_size(center, Vec2::splat(style.size));
    let id = ui.id().with((style.id_salt, &preset.id));
    let resp = ui.interact(rect, id, Sense::click());
    if style.hover_cursor && resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if visible || resp.hovered() || resp.has_focus() {
        let fill = if style.use_paper_fill {
            t.paper
        } else {
            t.surface
        };
        ui.painter()
            .circle(center, style.size * 0.5, fill, Stroke::new(1.0, t.line));
        let fg = if resp.hovered() { t.ink } else { t.muted };
        let icon_rect = Rect::from_center_size(center, Vec2::splat(style.icon_size));
        Icon::Close
            .image(ui, style.icon_size)
            .tint(fg)
            .paint_at(ui, icon_rect);
    }
    let label = format!("Delete {}", preset.name);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    if style.tooltip {
        widgets::tip(&resp, &label, None);
    }
    resp
}

/// Renders the inline preset save form and emits save commands.
pub(crate) fn draft_form(
    ui: &mut Ui,
    user: &[Preset],
    look: &Appearance,
    draft: &mut Draft,
    out: &mut Commands,
    style: &DraftStyle,
    t: &Tokens,
) {
    let mut frame = egui::Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(style.radius))
        .inner_margin(style.margin);
    if let Some(fill) = style.fill {
        frame = frame.fill(fill);
    }
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let name = ui.add(
            egui::TextEdit::singleline(&mut draft.name)
                .hint_text("Preset name")
                .desired_width(f32::INFINITY),
        );
        widgets::name(&name, "Preset name");
        if std::mem::take(&mut draft.focus) {
            name.request_focus();
        }
        let group = ui.add(
            egui::TextEdit::singleline(&mut draft.group)
                .hint_text("Section (optional)")
                .desired_width(f32::INFINITY),
        );
        widgets::name(&group, "Section");
        let sections = section_names(user);
        if !sections.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for s in sections {
                    if ui.selectable_label(draft.group.trim() == s, s).clicked() {
                        draft.group = s.to_owned();
                    }
                }
            });
        }
        let named = !draft.name.trim().is_empty();
        let left = name.lost_focus() || group.lost_focus();
        let enter = left && ui.input(|i| i.key_pressed(Key::Enter));
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        let (mut save, mut cancel) = (false, false);
        let primary = if named || !style.muted_when_empty {
            widgets::primary_button(ui, None, "Save", t.accent, t)
        } else {
            Button::new(RichText::new("Save").color(t.ink))
                .fill(t.surface)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius)
        };
        ui.columns(2, |cols| {
            if let [l, r] = cols {
                let h = style.button_height;
                save = l
                    .add_enabled_ui(named, |ui| ui.add_sized([ui.available_width(), h], primary))
                    .inner
                    .clicked();
                let size = [r.available_width(), h];
                cancel = r.add_sized(size, Button::new("Cancel")).clicked();
            }
        });
        if named && (save || enter) {
            out.push(Sent::Engine(Command::SavePreset {
                name: draft.name.trim().to_owned(),
                group: draft.group.trim().to_owned(),
                look: *look,
            }));
            *draft = Draft::default();
        } else if cancel || escape {
            *draft = Draft::default();
        }
    });
}
