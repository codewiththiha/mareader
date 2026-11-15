//! Dial deck Appearance popover: compact tabs, colour wheel and rotary dials.

mod dials;
pub(crate) mod labels;
mod presets;

use egui::{
    Align, Align2, Color32, CornerRadius, CursorIcon, FontId, Id, Layout, Margin, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_appearance::{Appearance, BaseMode, NoiseMode, Preset, TextureMode};
use mareader_settings::TextSettings;

use crate::appearance_menu::{MenuContext, QuickText};
use crate::command::{Command, Commands, Independence};
use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{self, Tokens};
use crate::widgets;
use labels::{mode_icon, noise_label, strength_percent};

pub use dials::{HUES, KnobSpec, STRENGTH_ON_HUE, WHEEL_SIZE, color_wheel, rotary_knob};
pub use presets::forget_draft;

pub const WIDTH: f32 = 316.0;
/// The `Text` tab has the fewest rows, so its dials grow into the slack.
const DIAL_TEXT: f32 = 64.0;
const DIAL_SURFACE: f32 = 52.0;
const INTENSITY_ON: u8 = 25;
const TAB_MEM_ID: &str = "dial-deck-active-tab";
const INNER_PAD: i8 = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DeckTab {
    #[default]
    Look,
    Text,
    Surface,
    Presets,
}

impl DeckTab {
    fn label(self, has_texture: bool) -> &'static str {
        match self {
            Self::Look => "Look",
            Self::Text => "Text",
            Self::Surface => {
                if has_texture {
                    "Page"
                } else {
                    "Grain"
                }
            }
            Self::Presets => "Presets",
        }
    }
}

/// Renders the tabbed Dial deck Appearance popover and emits any changes.
pub fn show(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let mut a = *cx.current;
    let mut text = cx.text;
    let mut split = cx.independence;

    egui::Frame::new()
        .inner_margin(Margin::same(INNER_PAD))
        .show(ui, |ui| {
            ui.set_width(WIDTH);
            ui.spacing_mut().item_spacing.y = 0.0;

            let tab = tab_bar(ui, cx.text.is_some(), cx.texture, &t);
            ui.add_space(8.0);
            bleed_divider(ui, &t);
            ui.add_space(12.0);

            match tab {
                DeckTab::Look => look_tab(ui, cx, &mut a, out, &t),
                DeckTab::Text => {
                    if let Some(q) = &mut text {
                        text_tab(ui, q, &t);
                    }
                }
                DeckTab::Surface => surface_tab(ui, cx.texture, &mut a, &t),
                DeckTab::Presets => presets::presets_tab(ui, cx, &mut a, out),
            }

            if let Some(ind) = &mut split {
                ui.add_space(12.0);
                bleed_divider(ui, &t);
                ui.add_space(8.0);
                split_footer(ui, ind, cx.texture, &t);
            }
        });

    if a != *cx.current {
        out.push(Command::SetAppearance(a));
    }
    if text != cx.text
        && let Some(q) = text
    {
        out.push(Command::SetQuickText(q));
    }
    if split != cx.independence
        && let Some(ind) = split
    {
        out.push(Command::SetIndependence(ind));
    }
}

fn tab_bar(ui: &mut Ui, has_text: bool, has_texture: bool, t: &Tokens) -> DeckTab {
    let mut tabs = Vec::with_capacity(4);
    tabs.push(DeckTab::Look);
    if has_text {
        tabs.push(DeckTab::Text);
    }
    tabs.push(DeckTab::Surface);
    tabs.push(DeckTab::Presets);

    let mem_id = Id::new(TAB_MEM_ID);
    let mut active = ui
        .data(|d| d.get_temp::<DeckTab>(mem_id))
        .unwrap_or_default();
    if !tabs.contains(&active) {
        active = DeckTab::Look;
    }

    let gap = 4.0;
    let count = tabs.len().max(1) as f32;
    let pill_w = (WIDTH - gap * (count - 1.0)) / count;
    let (bar_rect, _) = ui.allocate_exact_size(vec2(WIDTH, 30.0), Sense::hover());

    for (idx, &candidate) in tabs.iter().enumerate() {
        let x = bar_rect.left() + idx as f32 * (pill_w + gap);
        let rect = Rect::from_min_size(pos2(x, bar_rect.top()), vec2(pill_w, 30.0));
        let id = ui.id().with(("dial-deck-tab", idx));
        let resp = ui.interact(rect, id, Sense::click());
        let selected = candidate == active;
        let hot = resp.hovered();
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() && !selected {
            active = candidate;
            ui.data_mut(|d| d.insert_temp(mem_id, active));
        }

        let fill = if selected {
            dials::blend(t.paper, t.ink, if t.dark { 0.13 } else { 0.08 })
        } else if hot {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        if fill != Color32::TRANSPARENT {
            ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        }
        let label = candidate.label(has_texture);
        let font = if selected {
            theme::medium(12.5)
        } else {
            FontId::proportional(12.5)
        };
        let fg = if selected || hot { t.ink } else { t.muted };
        ui.painter()
            .text(rect.center(), Align2::CENTER_CENTER, label, font, fg);
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, label));
    }

    ui.data_mut(|d| d.insert_temp(mem_id, active));
    active
}

fn bleed_divider(ui: &mut Ui, t: &Tokens) {
    let pad = f32::from(INNER_PAD) + ui.style().spacing.menu_margin.leftf();
    let (line_rect, _) = ui.allocate_exact_size(vec2(WIDTH, 1.0), Sense::hover());
    let left = line_rect.left() - pad;
    let right = line_rect.right() + pad;
    ui.painter()
        .hline(left..=right, line_rect.center().y, Stroke::new(1.0, t.line));
}

fn stage_frame(t: &Tokens) -> egui::Frame {
    let fill = if t.dark {
        dials::blend(t.paper, Color32::BLACK, 0.22)
    } else {
        dials::blend(t.paper, t.surface, 0.55)
    };
    let border = dials::blend(t.line, t.paper, 0.25);
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
}

fn look_tab(ui: &mut Ui, cx: &MenuContext<'_>, a: &mut Appearance, out: &mut Commands, t: &Tokens) {
    mode_tiles(ui, &mut a.base, t);
    ui.add_space(10.0);

    stage_frame(t).show(ui, |ui| {
        let inner_w = WIDTH - 24.0;
        ui.set_width(inner_w);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            dials::color_wheel(ui, a);
            let right_w = (inner_w - WHEEL_SIZE - 12.0).max(80.0);
            ui.allocate_ui_with_layout(
                vec2(right_w, WHEEL_SIZE),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_size(vec2(right_w, WHEEL_SIZE));
                    match cx.ink {
                        Some(ink) => look_side_with_ink(ui, right_w, cx.presets, a, ink, out, t),
                        None => look_side_readout(ui, right_w, cx.presets, a, t),
                    }
                },
            );
        });
    });
}

fn mode_tiles(ui: &mut Ui, base: &mut BaseMode, t: &Tokens) {
    // Both mode controls share one persistent event and icon provider.
    let _act = anim::announce(ui, Act::Click);
    let gap = 6.0;
    let count = BaseMode::ALL.len() as f32;
    let tile_w = (WIDTH - gap * (count - 1.0)) / count;
    let tile_h = 48.0;
    let (row_rect, _) = ui.allocate_exact_size(vec2(WIDTH, tile_h), Sense::hover());

    for (idx, mode) in BaseMode::ALL.into_iter().enumerate() {
        let x = row_rect.left() + idx as f32 * (tile_w + gap);
        let rect = Rect::from_min_size(pos2(x, row_rect.top()), vec2(tile_w, tile_h));
        let id = ui.id().with(("dial-deck-base", idx));
        let resp = ui.interact(rect, id, Sense::click());
        let selected = *base == mode;
        let hot = resp.hovered();
        if hot {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.clicked() {
            *base = mode;
        }

        let fill = if selected {
            t.accent_soft
        } else if hot {
            t.hover
        } else {
            Color32::TRANSPARENT
        };
        let stroke = if selected {
            Stroke::new(1.0, t.accent)
        } else {
            Stroke::NONE
        };
        ui.painter().rect(
            rect,
            CornerRadius::same(10),
            fill,
            stroke,
            StrokeKind::Inside,
        );

        let fg = if selected {
            t.accent
        } else if hot {
            t.ink
        } else {
            t.muted
        };
        let icon_rect =
            Rect::from_center_size(pos2(rect.center().x, rect.top() + 16.0), Vec2::splat(15.0));
        anim::paint(ui, icon_rect, mode_icon(mode), fg, mode.label(), &resp);

        let font = if selected {
            theme::medium(11.5)
        } else {
            FontId::proportional(11.5)
        };
        ui.painter().text(
            pos2(rect.center().x, rect.bottom() - 12.0),
            Align2::CENTER_CENTER,
            mode.label(),
            font,
            fg,
        );
        resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, mode.label()));
    }
}

fn look_side_with_ink(
    ui: &mut Ui,
    width: f32,
    presets: &[Preset],
    a: &mut Appearance,
    ink: f32,
    out: &mut Commands,
    t: &Tokens,
) {
    ui.add_space(10.0);
    tint_compact_header(ui, width, presets, a, t);
    ui.add_space(9.0);
    let (sep_rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    let sep_col = dials::blend(t.line, t.paper, 0.25);
    ui.painter().hline(
        sep_rect.x_range(),
        sep_rect.center().y,
        Stroke::new(1.0, sep_col),
    );
    ui.add_space(9.0);

    let (lo, hi) = TextSettings::INK;
    let shown_ink = ink.round().clamp(lo, hi);
    let mut val = shown_ink;
    let spec = KnobSpec {
        id: "text-ink",
        label: "Text ink",
        range: lo..=hi,
        step: 5.0,
        default: 100.0,
        enabled: true,
        shown: format!("{val:.0}%"),
    };
    if dials::inline_knob(ui, width, &spec, &mut val) && (val - shown_ink).abs() > 0.1 {
        out.push(Command::SetInkContrast(val));
    }
}

fn tint_compact_header(
    ui: &mut Ui,
    width: f32,
    presets: &[Preset],
    a: &mut Appearance,
    t: &Tokens,
) {
    let pct = strength_percent(a.tint_strength);
    let matched = presets.iter().find(|p| p.appearance == *a);
    let headline = if !a.has_tint() {
        "No tint".to_owned()
    } else if let Some(name) = dials::landmark_name(a.tint_hue) {
        format!("{name} · {pct}%")
    } else {
        format!("{}° · {pct}%", a.tint_hue)
    };
    let subline = if !a.has_tint() {
        "Drag outward to tint".to_owned()
    } else if let Some(p) = matched {
        format!("Preset · {}", p.name)
    } else {
        format!("{}° hue", a.tint_hue)
    };

    let (head_rect, _) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    let dot_c = pos2(head_rect.left() + 5.0, head_rect.center().y);
    let dot_col = if a.has_tint() {
        dials::hue_colour(f32::from(a.tint_hue))
    } else {
        dials::blend(t.paper, t.muted, 0.35)
    };
    ui.painter()
        .circle(dot_c, 4.2, dot_col, Stroke::new(1.0, t.line));
    ui.painter().text(
        pos2(head_rect.left() + 14.0, head_rect.center().y),
        Align2::LEFT_CENTER,
        headline,
        theme::medium(12.5),
        t.ink,
    );
    if a.has_tint() {
        let btn_rect = Rect::from_center_size(
            pos2(head_rect.right() - 9.0, head_rect.center().y),
            Vec2::splat(18.0),
        );
        if clear_tint_icon(ui, btn_rect, t).clicked() {
            a.tint_strength = 0;
        }
    }

    ui.add_space(2.0);
    let (sub_rect, _) = ui.allocate_exact_size(vec2(width, 15.0), Sense::hover());
    ui.painter().text(
        pos2(sub_rect.left(), sub_rect.center().y),
        Align2::LEFT_CENTER,
        subline,
        FontId::proportional(11.0),
        t.muted,
    );
}

fn look_side_readout(ui: &mut Ui, width: f32, presets: &[Preset], a: &mut Appearance, t: &Tokens) {
    let pct = strength_percent(a.tint_strength);
    let matched = presets.iter().find(|p| p.appearance == *a);
    let eyebrow = match matched {
        Some(p) => format!("Preset · {}", p.name),
        None => format!("{} mode", a.base.label()),
    };
    let headline = if !a.has_tint() {
        "No tint".to_owned()
    } else if let Some(name) = dials::landmark_name(a.tint_hue) {
        name.to_owned()
    } else {
        format!("{}° tint", a.tint_hue)
    };
    let detail = if a.has_tint() {
        format!("{pct}% strength · {}°", a.tint_hue)
    } else {
        "Drag outward to tint".to_owned()
    };

    ui.add_space(16.0);
    ui.label(RichText::new(eyebrow).size(11.0).color(t.muted));
    ui.add_space(4.0);
    ui.label(
        RichText::new(headline)
            .font(theme::medium(15.0))
            .color(t.ink),
    );
    ui.add_space(3.0);
    ui.label(RichText::new(detail).size(11.5).color(t.muted));
    ui.add_space(12.0);

    if a.has_tint() {
        if reset_tint_pill(ui, width.min(96.0), t).clicked() {
            a.tint_strength = 0;
        }
    } else {
        let hint = RichText::new("Rim dots pick a hue")
            .size(11.0)
            .color(t.muted.gamma_multiply(0.75));
        ui.label(hint);
    }
}

fn clear_tint_icon(ui: &mut Ui, rect: Rect, t: &Tokens) -> Response {
    let id = ui.id().with("dial-deck-clear-tint");
    let resp = ui.interact(rect, id, Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(5), t.hover);
    }
    let fg = if hot { t.ink } else { t.muted };
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(10.0));
    Icon::Close.image(ui, 10.0).tint(fg).paint_at(ui, icon_rect);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Reset tint"));
    widgets::tip(&resp, "Reset tint", None);
    resp
}

fn reset_tint_pill(ui: &mut Ui, width: f32, t: &Tokens) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 25.0), Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let fill = if hot {
        t.hover
    } else {
        dials::blend(t.paper, t.ink, if t.dark { 0.07 } else { 0.04 })
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(7),
        fill,
        Stroke::new(1.0, t.line),
        StrokeKind::Inside,
    );
    let fg = if hot { t.ink } else { t.muted };
    let icon_rect =
        Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(11.0));
    Icon::Restore
        .image(ui, 11.0)
        .tint(fg)
        .paint_at(ui, icon_rect);
    ui.painter().text(
        pos2(rect.left() + 22.0, rect.center().y),
        Align2::LEFT_CENTER,
        "Reset tint",
        theme::medium(11.5),
        fg,
    );
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Reset tint"));
    resp
}

fn text_tab(ui: &mut Ui, q: &mut QuickText, t: &Tokens) {
    stage_frame(t).show(ui, |ui| {
        let inner_w = WIDTH - 24.0;
        let gap = 8.0;
        let col_w = (inner_w - gap * 2.0) / 3.0;
        ui.set_width(inner_w);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;

            let (s_lo, s_hi) = TextSettings::SIZE_PCT;
            let size_spec = KnobSpec {
                id: "text-size",
                label: "Size",
                range: s_lo..=s_hi,
                step: 5.0,
                default: 100.0,
                enabled: true,
                shown: format!("{:.0}%", q.size_pct),
            };
            rotary_knob(ui, col_w, DIAL_TEXT, &size_spec, &mut q.size_pct);

            let (l_lo, l_hi) = TextSettings::LINE_HEIGHT;
            let lines_spec = KnobSpec {
                id: "text-lines",
                label: "Lines",
                range: l_lo..=l_hi,
                step: 0.1,
                default: 1.7,
                enabled: true,
                shown: format!("{:.1}×", q.line_height),
            };
            rotary_knob(ui, col_w, DIAL_TEXT, &lines_spec, &mut q.line_height);

            let (p_lo, p_hi) = TextSettings::PARAGRAPH;
            let gaps_spec = KnobSpec {
                id: "text-gaps",
                label: "Gaps",
                range: p_lo..=p_hi,
                step: 0.1,
                default: 1.0,
                enabled: true,
                shown: format!("{:.1} em", q.paragraph_margin),
            };
            rotary_knob(ui, col_w, DIAL_TEXT, &gaps_spec, &mut q.paragraph_margin);
        });
    });

    ui.add_space(10.0);
    switch_line(
        ui,
        "Override book font",
        "Use your typeface and size over the book's own",
        &mut q.override_font,
    );
    ui.add_space(6.0);
    switch_line(
        ui,
        "Override book layout",
        "Use your line height and spacing over the book's own",
        &mut q.override_layout,
    );
}

fn surface_tab(ui: &mut Ui, has_texture: bool, a: &mut Appearance, t: &Tokens) {
    if has_texture {
        let tex_summary = match a.texture {
            TextureMode::None => "None".to_owned(),
            m => format!("{} · {}%", m.label(), a.texture_opacity),
        };
        section_caption(ui, "Page texture", &tex_summary, t);
        ui.add_space(5.0);
        widgets::segments(ui, &mut a.texture, &TextureMode::ALL, 3, |m| {
            (None, m.label())
        });
        ui.add_space(10.0);
    }

    let grain_summary = match a.noise {
        NoiseMode::Off => "Off".to_owned(),
        n => format!("{} · {}%", noise_label(n), a.noise_intensity),
    };
    section_caption(ui, "Film grain", &grain_summary, t);
    ui.add_space(5.0);
    let before = a.noise;
    let grain_modes = [NoiseMode::Off, NoiseMode::Static, NoiseMode::Animated];
    widgets::segments(ui, &mut a.noise, &grain_modes, 3, |n| {
        (None, noise_label(n))
    });
    if a.noise != before && a.noise != NoiseMode::Off && a.noise_intensity == 0 {
        a.noise_intensity = INTENSITY_ON;
    }

    ui.add_space(10.0);
    stage_frame(t).show(ui, |ui| {
        let inner_w = WIDTH - 24.0;
        ui.set_width(inner_w);
        if has_texture {
            surface_three_knobs(ui, inner_w, a);
        } else {
            grain_single_knob(ui, inner_w, a);
        }
    });
}

fn surface_three_knobs(ui: &mut Ui, inner_w: f32, a: &mut Appearance) {
    let gap = 8.0;
    let col_w = (inner_w - gap * 2.0) / 3.0;
    let tex_on = a.texture != TextureMode::None;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;

        let mut opacity = f32::from(a.texture_opacity);
        let op_spec = KnobSpec {
            id: "texture-opacity",
            label: "Opacity",
            range: 0.0..=100.0,
            step: 5.0,
            default: 90.0,
            enabled: tex_on,
            shown: if tex_on {
                format!("{}%", a.texture_opacity)
            } else {
                "—".to_owned()
            },
        };
        if rotary_knob(ui, col_w, DIAL_SURFACE, &op_spec, &mut opacity) {
            a.texture_opacity = opacity.round().clamp(0.0, 100.0) as u8;
        }

        let mut scale = f32::from(a.texture_scale);
        let sc_spec = KnobSpec {
            id: "texture-scale",
            label: "Scale",
            range: 25.0..=400.0,
            step: 5.0,
            default: 100.0,
            enabled: tex_on,
            shown: if tex_on {
                format!("{}%", a.texture_scale)
            } else {
                "—".to_owned()
            },
        };
        if rotary_knob(ui, col_w, DIAL_SURFACE, &sc_spec, &mut scale) {
            a.texture_scale = scale.round().clamp(25.0, 400.0) as u16;
        }

        grain_knob_column(ui, col_w, "Grain", a);
    });
}

fn grain_single_knob(ui: &mut Ui, inner_w: f32, a: &mut Appearance) {
    ui.vertical_centered(|ui| {
        grain_knob_column(ui, inner_w.min(120.0), "Intensity", a);
    });
}

fn grain_knob_column(ui: &mut Ui, col_w: f32, label: &'static str, a: &mut Appearance) {
    let active = a.noise != NoiseMode::Off;
    let mut val = if active {
        f32::from(a.noise_intensity)
    } else {
        0.0
    };
    let spec = KnobSpec {
        id: "grain-intensity",
        label,
        range: 0.0..=100.0,
        step: 5.0,
        default: f32::from(INTENSITY_ON),
        enabled: true,
        shown: if active {
            format!("{}%", a.noise_intensity)
        } else {
            "Off".to_owned()
        },
    };
    if rotary_knob(ui, col_w, DIAL_SURFACE, &spec, &mut val) {
        let next = val.round().clamp(0.0, 100.0) as u8;
        a.noise_intensity = next;
        if !active && next > 0 {
            a.noise = NoiseMode::Static;
        }
    }
}

fn section_caption(ui: &mut Ui, title: &str, summary: &str, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(title)
                .font(theme::medium(11.5))
                .color(t.muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(summary).size(11.0).color(t.muted));
        });
    });
}

fn split_footer(ui: &mut Ui, i: &mut Independence, has_texture: bool, t: &Tokens) {
    ui.label(
        RichText::new("Split view")
            .font(theme::medium(11.5))
            .color(t.muted),
    );
    ui.add_space(6.0);
    switch_line(
        ui,
        "Theme per pane",
        "Each pane keeps its own colour; grain stays shared.",
        &mut i.themes,
    );
    if has_texture {
        ui.add_space(5.0);
        switch_line(
            ui,
            "Texture per pane",
            "Each pane keeps its own texture and dials.",
            &mut i.textures,
        );
    }
}

fn switch_line(ui: &mut Ui, title: &str, hint: &str, on: &mut bool) {
    ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        let r = ui.label(title);
        widgets::tip(&r, hint, None);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let sw = widgets::switch(ui, on, title);
            widgets::tip(&sw, hint, None);
        });
    });
}
