//! The Appearance popover: presets, mode and colour, texture and grain.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Align2, Color32, Context, CornerRadius, CursorIcon, FontId, Galley, Id, Key, Layout,
    Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, WidgetInfo,
    WidgetType, pos2, vec2,
};
use mareader_appearance::{
    Appearance, BaseMode, FULL_TINT, FilmGrain, NoiseMode, Preset, TextureMode, group_presets,
    section_names,
};
use mareader_settings::TextSettings;

use crate::command::{Command, Commands, Independence};
use crate::dial_deck::labels::{mode_icon, noise_label, strength_percent};
use crate::gpu::{Anchor, Gpu, GrainCallback, PagesCallback, Quad, crawl_offset};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub const WIDTH: f32 = 300.0;
/// A slider row's name and value boxes: fixed, so no value widens it.
const NAME_W: f32 = 64.0;
const VALUE_W: f32 = 52.0;
/// A group's fold header.
const HEADER: f32 = 24.0;
/// A sub-group's header sits in from its parent's.
const SUB_INDENT: f32 = 12.0;
/// A hue picked at zero strength would show nothing.
const STRENGTH_ON_HUE: u8 = 18;
/// Grain switched on at zero intensity would read as a dead control.
const INTENSITY_ON: u8 = 25;
/// The tallest the popover grows before it scrolls, as mareader's 32 rem.
pub const MAX_HEIGHT: f32 = 512.0;
/// Kept clear between the popover and the window's foot.
const FOOT_MARGIN: f32 = 16.0;
/// A card's texture against a page's, as mareader's preview.
const CARD_ZOOM: f32 = 0.34;
const CARD_RADIUS: f32 = 6.0;
/// A card's page height per width, then the name strip below it.
const CARD_ASPECT: f32 = 0.7;
const CARD_NAME: f32 = 14.0;
const CARD_GAP: f32 = 6.0;
const SWATCH: f32 = 20.0;
/// Named landmarks on the hue circle, as mareader's.
const HUES: [(u16, &str); 7] = [
    (34, "Sepia"),
    (14, "Rose"),
    (104, "Green"),
    (160, "Mint"),
    (200, "Sky"),
    (240, "Blue"),
    (290, "Violet"),
];
const DRAFT: &str = "appearance-preset-draft";
/// Folded groups, persisted so a fold outlives a restart.
const FOLDS: &str = "appearance-folds";
/// The pinned pane switches' height, measured the frame before.
const FOOTER: &str = "appearance-footer";

/// What the popover reads besides the look it edits.
pub struct MenuContext<'a> {
    pub current: &'a Appearance,
    /// The per-pane modes, while the workspace is split.
    pub independence: Option<Independence>,
    /// Every preset, built-ins first.
    pub presets: &'a [Preset],
    /// The user's own, which can be deleted and name sections.
    pub user: &'a [Preset],
    /// Draws cards as real pages; without it they are flat.
    pub gpu: Option<&'a Gpu>,
    /// The ink dial's value while a book is active; none hides it.
    pub ink: Option<f32>,
    /// Offers the page texture; the library has no page.
    pub texture: bool,
    /// A book's quick text dials while one is active; none hides them.
    pub text: Option<QuickText>,
}

/// The text dials the popover offers for a book, from its text settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuickText {
    pub size_pct: f32,
    pub line_height: f32,
    pub paragraph_margin: f32,
    pub override_font: bool,
    pub override_layout: bool,
}

impl QuickText {
    pub fn of(t: &TextSettings) -> Self {
        Self {
            size_pct: t.size_pct(),
            line_height: t.line_height,
            paragraph_margin: t.paragraph_margin,
            override_font: t.override_font,
            override_layout: t.override_layout,
        }
    }

    /// Writes the dials back; an untouched size keeps its exact points.
    pub fn apply(self, t: &mut TextSettings) {
        if self.size_pct != t.size_pct() {
            t.set_size_pct(self.size_pct);
        }
        t.line_height = self.line_height;
        t.paragraph_margin = self.paragraph_margin;
        t.override_font = self.override_font;
        t.override_layout = self.override_layout;
        t.sanitize();
    }
}

/// Edits a copy and pushes one command when anything changed.
pub fn show(ui: &mut Ui, cx: &MenuContext<'_>, out: &mut Commands) {
    let mut a = *cx.current;
    ui.set_width(WIDTH);
    let mut split = cx.independence;
    let mut text = cx.text;
    // The pane switches stay pinned below, so the scroll gives up their room.
    let footer = match split {
        Some(_) => ui
            .data(|d| d.get_temp::<f32>(Id::new(FOOTER)))
            .unwrap_or(96.0),
        None => 0.0,
    };
    // The room down to the window's foot holds the body and the switches.
    let room = ui.ctx().content_rect().bottom() - ui.cursor().top() - FOOT_MARGIN;
    let max = (room - footer).clamp(160.0, MAX_HEIGHT);
    // egui caps a popup's first frame; claiming the height lifts it.
    let t = Tokens::get(ui.ctx());
    let body = egui::ScrollArea::vertical()
        .max_height(max)
        .min_scrolled_height(max)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            // A smaller interact height keeps slider knobs in proportion.
            ui.spacing_mut().interact_size.y = 20.0;
            presets_group(ui, cx, &mut a, out);
            ui.separator();
            colour_group(ui, cx, &mut a, out);
            if let Some(q) = &mut text {
                ui.separator();
                text_group(ui, q);
            }
            if cx.texture {
                ui.separator();
                texture_group(ui, &mut a);
            }
            ui.separator();
            grain_group(ui, &mut a);
        });
    // Rows past the edge fade, so the cut reads as more to scroll.
    widgets::scroll_fades(ui, &body, t.surface);
    if let Some(i) = &mut split {
        let top = ui.cursor().top();
        panes(ui, i, cx.texture);
        let height = ui.cursor().top() - top;
        ui.data_mut(|d| d.insert_temp(Id::new(FOOTER), height));
    }
    if split != cx.independence
        && let Some(i) = split
    {
        out.push(Command::SetIndependence(i));
    }
    if a != *cx.current {
        out.push(Command::SetAppearance(a));
    }
    if text != cx.text
        && let Some(q) = text
    {
        out.push(Command::SetQuickText(q));
    }
}

/// A book's size, spacing and overrides; the rest are in Settings.
fn text_group(ui: &mut Ui, q: &mut QuickText) {
    let summary = format!("{:.0}% · {:.1}×", q.size_pct, q.line_height);
    if !fold(ui, "text", "Text", &summary, 0.0, |_| {}) {
        return;
    }
    let pct = |v: f32| format!("{v:.0}%");
    dial(
        ui,
        "Size",
        &mut q.size_pct,
        TextSettings::SIZE_PCT,
        5.0,
        pct,
    );
    let times = |v: f32| format!("{v:.1}×");
    let lines = &mut q.line_height;
    dial(ui, "Lines", lines, TextSettings::LINE_HEIGHT, 0.1, times);
    let ems = |v: f32| format!("{v:.2}");
    let gaps = &mut q.paragraph_margin;
    dial(ui, "Gaps", gaps, TextSettings::PARAGRAPH, 0.25, ems);
    let hint = "Your fonts and text size for all of a book's text";
    pane_switch(ui, "Override book font", hint, &mut q.override_font);
    let hint = "Your spacing, indent and justification for its paragraphs";
    pane_switch(ui, "Override book layout", hint, &mut q.override_layout);
}

/// Label left, a stepped slider, then its value in a fixed box on the right.
fn dial(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    (lo, hi): (f32, f32),
    step: f32,
    show: impl Fn(f32) -> String,
) {
    ui.horizontal(|ui| {
        let named = name_box(ui, label);
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().slider_width = (ui.available_width() - VALUE_W - gap).max(40.0);
        widgets::labelled(ui, named, |ui| {
            let slider = egui::Slider::new(value, lo..=hi).step_by(f64::from(step));
            ui.add(slider.show_value(false));
        });
        value_box(ui, &show(*value));
    });
}

/// The presets, with Save current look on the header.
fn presets_group(ui: &mut Ui, cx: &MenuContext<'_>, a: &mut Appearance, out: &mut Commands) {
    let picked = cx.presets.iter().find(|p| p.appearance == *a);
    let summary = picked.map_or("Unsaved look", |p| p.name.as_str());
    let mut save = false;
    let open = fold(ui, "presets", "Presets", summary, 0.0, |ui| {
        save = widgets::small_icon_button(ui, Icon::Plus, "Save current look").clicked();
    });
    if save {
        start_draft(ui.ctx());
        ui.data_mut(|d| d.insert_persisted(fold_id("presets"), true));
    }
    if open || save {
        presets(ui, cx, a, out);
        save_form(ui, cx.user, a, out);
    }
}

/// Base mode, hue, strength and, for a book, its ink.
fn colour_group(ui: &mut Ui, cx: &MenuContext<'_>, a: &mut Appearance, out: &mut Commands) {
    if !fold(
        ui,
        "colour",
        "Mode & colour",
        &colour_summary(a),
        0.0,
        |_| {},
    ) {
        return;
    }
    widgets::segments(ui, &mut a.base, &BaseMode::ALL, 4, |b| {
        (Some(mode_icon(b)), b.label())
    });
    hue(ui, a);
    strength(ui, a);
    if let Some(ink) = cx.ink {
        ink_dial(ui, ink, out);
    }
}

/// The page texture; its dials show once there is a texture to dial.
fn texture_group(ui: &mut Ui, a: &mut Appearance) {
    let summary = match a.texture {
        TextureMode::None => "None".to_owned(),
        m => format!("{} · {}%", m.label(), a.texture_opacity),
    };
    if !fold(ui, "texture", "Page texture", &summary, 0.0, |_| {}) {
        return;
    }
    widgets::segments(ui, &mut a.texture, &TextureMode::ALL, 3, |m| {
        (None, m.label())
    });
    if a.texture != TextureMode::None {
        slider(ui, "Opacity", &mut a.texture_opacity, 0..=100);
        slider(ui, "Scale", &mut a.texture_scale, 25..=400);
    }
}

/// Film grain; its intensity shows once the grain is on.
fn grain_group(ui: &mut Ui, a: &mut Appearance) {
    let summary = match a.noise {
        NoiseMode::Off => "Off".to_owned(),
        n => format!("{} · {}%", noise_label(n), a.noise_intensity),
    };
    if !fold(ui, "grain", "Film grain", &summary, 0.0, |_| {}) {
        return;
    }
    let before = a.noise;
    let grain = [NoiseMode::Off, NoiseMode::Static, NoiseMode::Animated];
    widgets::segments(ui, &mut a.noise, &grain, 3, |n| (None, noise_label(n)));
    if a.noise != before && a.noise != NoiseMode::Off && a.noise_intensity == 0 {
        a.noise_intensity = INTENSITY_ON;
    }
    if a.noise != NoiseMode::Off {
        slider(ui, "Intensity", &mut a.noise_intensity, 0..=100);
    }
}

/// While split: each pane's own theme and texture, pinned at the foot.
fn panes(ui: &mut Ui, i: &mut Independence, texture: bool) {
    let t = Tokens::get(ui.ctx());
    ui.separator();
    ui.spacing_mut().item_spacing.y = 6.0;
    let title = RichText::new("Split view")
        .size(t.text_small)
        .color(t.muted);
    ui.label(title);
    let hint = "Each pane keeps its own colour; grain stays shared.";
    pane_switch(ui, "Theme per pane", hint, &mut i.themes);
    if texture {
        let hint = "Each pane keeps its own texture and dials.";
        pane_switch(ui, "Texture per pane", hint, &mut i.textures);
    }
    let note = RichText::new("Film grain stays shared.").size(t.text_small);
    ui.label(note.color(t.muted));
}

/// A title on the left, its switch on the right; the hint on hover.
fn pane_switch(ui: &mut Ui, title: &str, hint: &str, on: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(title).on_hover_text(hint);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            widgets::switch(ui, on, title).on_hover_text(hint);
        });
    });
}

/// A group's title row; a click folds or unfolds what follows it.
fn fold(
    ui: &mut Ui,
    key: &str,
    title: &str,
    summary: &str,
    indent: f32,
    trailing: impl FnOnce(&mut Ui),
) -> bool {
    let t = Tokens::get(ui.ctx());
    let id = fold_id(key);
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or(id, true));
    let size = vec2(ui.available_width(), HEADER);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_persisted(id, open));
    }
    let hot = resp.hovered();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::CollapsingHeader, true, open, title));
    if hot {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), t.hover);
    }
    // Laid first and later in the pass, the control wins its own clicks.
    let room = rect.shrink2(vec2(2.0, 0.0));
    let layout = Layout::right_to_left(Align::Center);
    let mut right = ui.new_child(UiBuilder::new().max_rect(room).layout(layout));
    trailing(&mut right);
    let used = right.min_rect().width();
    let end = room.right() - if used.is_finite() { used + 6.0 } else { 0.0 };
    let ink = match (hot, indent > 0.0) {
        (false, true) => t.muted,
        _ => t.ink,
    };
    let y = rect.center().y;
    let x = rect.left() + indent;
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let at = Rect::from_center_size(pos2(x + 8.0, y), Vec2::splat(12.0));
    chevron.image(ui, 12.0).tint(t.muted).paint_at(ui, at);
    let font = FontId::proportional(t.text_small);
    let painter = ui.painter();
    let named = painter.text(
        pos2(x + 18.0, y),
        Align2::LEFT_CENTER,
        title,
        font.clone(),
        ink,
    );
    // Folded, the row still tells what the group is set to.
    if !open {
        let width = (end - named.right() - 12.0).max(0.0);
        let galley = elided(ui, summary, font, t.muted, width);
        let size = galley.size();
        painter.galley(pos2(end - size.x, y - size.y / 2.0), galley, t.muted);
    }
    open
}

fn fold_id(key: &str) -> Id {
    Id::new(FOLDS).with(key)
}

/// One line of text, cut with an ellipsis where it would overflow.
fn elided(ui: &Ui, text: &str, font: FontId, colour: Color32, width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, colour);
    job.wrap = TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

/// Drops a half-typed preset once the popover has closed.
pub fn forget_draft(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Draft>(Id::new(DRAFT)));
}

/// A row's muted name in its fixed box; returns its id to label by.
fn name_box(ui: &mut Ui, name: &str) -> Id {
    let t = Tokens::get(ui.ctx());
    let text = egui::Label::new(RichText::new(name).color(t.muted)).truncate();
    // Left-aligned, so every row's name starts on one edge.
    let size = vec2(NAME_W, 20.0);
    let layout = Layout::left_to_right(Align::Center);
    let named = ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_min_size(size);
        ui.add(text).id
    });
    named.inner
}

/// Painted and clipped in a fixed box: a widget here could outgrow it.
fn value_box(ui: &mut Ui, shown: &str) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(VALUE_W, 20.0), Sense::hover());
    let font = FontId::proportional(t.text);
    ui.painter_at(rect).text(
        rect.right_center(),
        Align2::RIGHT_CENTER,
        shown,
        font,
        t.ink,
    );
}

/// Label left, slider, then its percent in a fixed box on the right.
fn slider<N: egui::emath::Numeric>(
    ui: &mut Ui,
    label: &str,
    value: &mut N,
    range: std::ops::RangeInclusive<N>,
) {
    ui.horizontal(|ui| {
        let named = name_box(ui, label);
        let gap = ui.spacing().item_spacing.x;
        ui.spacing_mut().slider_width = (ui.available_width() - VALUE_W - gap).max(40.0);
        widgets::labelled(ui, named, |ui| {
            ui.add(egui::Slider::new(value, range).show_value(false));
        });
        value_box(ui, &format!("{:.0}%", value.to_f64()));
    });
}

/// Strength as a share of the whole tint, so all its travel shows.
fn strength(ui: &mut Ui, a: &mut Appearance) {
    let shown = strength_percent(a.tint_strength);
    let mut value = shown;
    slider(ui, "Strength", &mut value, 0..=100);
    if value != shown {
        a.tint_strength = percent_strength(value);
    }
}

fn percent_strength(percent: u8) -> u8 {
    let whole = u16::from(FULL_TINT);
    let strength = (u16::from(percent.min(100)) * whole + 50) / 100;
    u8::try_from(strength).unwrap_or(FULL_TINT)
}

/// mareader's text ink intensity: only a book's text takes it.
fn ink_dial(ui: &mut Ui, ink: f32, out: &mut Commands) {
    let (lo, hi) = TextSettings::INK;
    let shown = ink.round().clamp(lo, hi) as u8;
    let mut value = shown;
    slider(ui, "Text ink", &mut value, lo as u8..=hi as u8);
    if value != shown {
        out.push(Command::SetInkContrast(f32::from(value)));
    }
}

/// The folded colour group's line, as "Light · Sepia 60%".
fn colour_summary(a: &Appearance) -> String {
    if !a.has_tint() {
        return format!("{} · no tint", a.base.label());
    }
    let landmark = HUES.iter().find(|(h, _)| *h == a.tint_hue);
    let hue = landmark.map_or_else(|| format!("{}°", a.tint_hue), |(_, n)| (*n).to_owned());
    let share = strength_percent(a.tint_strength);
    format!("{} · {hue} {share}%", a.base.label())
}

/// The hue strip in a slider's row, its landmarks spread beneath it.
fn hue(ui: &mut Ui, a: &mut Appearance) {
    let mut changed = false;
    let gap = ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        let named = name_box(ui, "Hue");
        let width = ui.available_width() - VALUE_W - gap;
        let strip = widgets::labelled(ui, named, |ui| hue_strip(ui, &mut a.tint_hue, width));
        changed |= strip.changed();
        value_box(ui, &format!("{}°", a.tint_hue));
    });
    ui.horizontal(|ui| {
        ui.add_space(NAME_W + gap);
        let width = ui.available_width() - VALUE_W - gap;
        let n = HUES.len() as f32;
        ui.spacing_mut().item_spacing.x = ((width - n * SWATCH) / (n - 1.0)).max(2.0);
        for (h, name) in HUES {
            if hue_swatch(ui, h, name, a.tint_hue == h).clicked() {
                a.tint_hue = h;
                changed = true;
            }
        }
    });
    if changed && a.tint_strength == 0 {
        a.tint_strength = STRENGTH_ON_HUE;
    }
}

/// The swatch hue matches what the tint filter produces (HSL 60 %, 55 %).
fn hue_colour(h: f32) -> Color32 {
    let (s, l) = (0.6, 0.55);
    let c = (1.0 - (2.0 * l - 1.0_f32).abs()) * s;
    let hp = (h.rem_euclid(360.0)) / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f32| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(byte(r), byte(g), byte(b))
}

/// Click or drag along the hue circle.
fn hue_strip(ui: &mut Ui, hue: &mut u16, width: f32) -> Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(width.max(40.0), 18.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let strip = rect.shrink2(vec2(9.0, 3.0));
    if let Some(p) = resp.interact_pointer_pos() {
        let f = ((p.x - strip.left()) / strip.width()).clamp(0.0, 1.0);
        let h = (f * 359.0).round() as u16;
        if h != *hue {
            *hue = h;
            resp.mark_changed();
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    const STEPS: usize = 36;
    let w = strip.width() / STEPS as f32;
    let painter = ui.painter();
    for i in 0..STEPS {
        let seg = Rect::from_min_size(
            pos2(strip.left() + i as f32 * w, strip.top()),
            vec2(w + 0.5, strip.height()),
        );
        painter.rect_filled(seg, CornerRadius::ZERO, hue_colour(i as f32 * 10.0 + 5.0));
    }
    painter.rect_stroke(
        strip,
        CornerRadius::same(3),
        Stroke::new(1.0, t.line),
        StrokeKind::Outside,
    );
    let x = strip.left() + f32::from(*hue) / 359.0 * strip.width();
    let knob = pos2(x, rect.center().y);
    painter.circle(
        knob,
        8.0,
        hue_colour(f32::from(*hue)),
        Stroke::new(2.5, Color32::WHITE),
    );
    painter.circle_stroke(knob, 9.5, Stroke::new(1.0, t.shadow));
    let value = f64::from(*hue);
    resp.widget_info(|| WidgetInfo::slider(true, value, "Hue"));
    resp
}

/// A landmark hue; a ring marks it picked, so the row never shifts.
fn hue_swatch(ui: &mut Ui, h: u16, name: &str, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SWATCH), Sense::click());
    let painter = ui.painter();
    let (c, r) = (rect.center(), SWATCH / 2.0);
    painter.circle(
        c,
        r - 2.0,
        hue_colour(f32::from(h)),
        Stroke::new(1.0, t.line),
    );
    if on {
        painter.circle_stroke(c, r, Stroke::new(2.0, t.accent));
    } else if resp.hovered() {
        painter.circle_stroke(c, r, Stroke::new(2.0, t.line));
    }
    let label = format!("{name} hue");
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, &label));
    resp.on_hover_text(name)
}

/// Every preset as a small page; a section folds only beside others.
fn presets(ui: &mut Ui, cx: &MenuContext<'_>, a: &mut Appearance, out: &mut Commands) {
    let groups = group_presets(cx.presets);
    let named = groups.len() > 1;
    for group in groups {
        if named {
            let picked = group.presets.iter().find(|p| p.appearance == *a);
            let summary = match picked {
                Some(p) => p.name.clone(),
                None => format!("{} looks", group.presets.len()),
            };
            let key = format!("preset-group/{}", group.name);
            if !fold(ui, &key, group.name, &summary, SUB_INDENT, |_| {}) {
                continue;
            }
        }
        cards(ui, cx, &group.presets, a, out);
    }
}

/// One group's cards in rows; a short last row is centred.
fn cards(
    ui: &mut Ui,
    cx: &MenuContext<'_>,
    presets: &[&Preset],
    a: &mut Appearance,
    out: &mut Commands,
) {
    let (cols, name_size) = fitting_columns(ui, presets);
    let w = card_width(ui.available_width(), cols);
    let h = w * CARD_ASPECT + CARD_NAME;
    for row in presets.chunks(cols) {
        let used = row.len() as f32 * w + (row.len().max(1) - 1) as f32 * CARD_GAP;
        let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        let mut x = band.left() + (band.width() - used) / 2.0;
        for p in row {
            let cell = Rect::from_min_size(pos2(x, band.top()), vec2(w, h));
            let mut child = ui.new_child(UiBuilder::new().max_rect(cell));
            card(&mut child, cx, p, name_size, a, out);
            x += w + CARD_GAP;
        }
    }
}

/// Columns for `n` cards: five, or four when five would orphan one.
fn columns(n: usize) -> usize {
    if n > 5 && n % 5 == 1 { 4 } else { 5 }
}

/// Columns and name size so every name shows whole, five across if any.
fn fitting_columns(ui: &Ui, presets: &[&Preset]) -> (usize, f32) {
    let t = Tokens::get(ui.ctx());
    let base = t.text_small - 1.0;
    let widest = |size: f32| {
        let font = FontId::proportional(size);
        let width = |p: &&Preset| {
            let g = ui
                .painter()
                .layout_no_wrap(p.name.clone(), font.clone(), t.ink);
            g.size().x
        };
        presets.iter().map(width).fold(0.0, f32::max)
    };
    let room = ui.available_width();
    let first = columns(presets.len());
    // Smaller type first, by a point at most; then fewer, wider cards.
    for cols in (3..=first).rev() {
        for size in [base, base - 0.5, base - 1.0] {
            if widest(size) <= name_room(card_width(room, cols)) {
                return (orphanless(cols, presets.len()), size);
            }
        }
    }
    (3, base)
}

/// One card alone on a row looks lost; narrower ones still fit.
fn orphanless(cols: usize, n: usize) -> usize {
    if cols < columns(n) && n > cols && n % cols == 1 {
        cols - 1
    } else {
        cols
    }
}

fn card_width(room: f32, cols: usize) -> f32 {
    (room - CARD_GAP * (cols - 1) as f32) / cols as f32
}

/// A centred name may run into the gaps either side of its card.
fn name_room(card: f32) -> f32 {
    card + CARD_GAP - 2.0
}

/// One preset; picks it on click, and the user's own show a delete.
fn card(
    ui: &mut Ui,
    cx: &MenuContext<'_>,
    p: &Preset,
    name_size: f32,
    a: &mut Appearance,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let selected = p.appearance == *a;
    let w = ui.available_width();
    let size = vec2(w, w * CARD_ASPECT + CARD_NAME);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let page = Rect::from_min_size(rect.min, vec2(w, w * CARD_ASPECT));
    let hot = ui.rect_contains_pointer(rect);
    paint_page(ui, cx.gpu, page, &p.appearance);
    let border = match (selected, hot) {
        (true, _) => Stroke::new(2.0, t.accent),
        (false, true) => Stroke::new(2.0, t.muted),
        (false, false) => Stroke::new(1.0, t.line),
    };
    let r = CornerRadius::same(CARD_RADIUS as u8);
    ui.painter()
        .rect_stroke(page, r, border, StrokeKind::Inside);
    let font = FontId::proportional(name_size);
    let colour = if selected { t.accent } else { t.ink };
    let name = elided(ui, &p.name, font, colour, name_room(w));
    let at = pos2(
        rect.center().x - name.size().x / 2.0,
        rect.bottom() - name.size().y,
    );
    ui.painter().galley(at, name, colour);
    let label = p.name.clone();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, &label));
    let own = cx.user.iter().any(|u| u.id == p.id);
    if own && hot && delete_button(ui, page, p).clicked() {
        out.push(Command::DeletePreset(p.id.clone()));
    } else if resp.on_hover_text(&p.name).clicked() {
        *a = p.appearance;
    }
}

/// A round × on a card's corner, above the card's own click.
fn delete_button(ui: &mut Ui, page: Rect, p: &Preset) -> Response {
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(page.right_top() + vec2(-3.0, 3.0), Vec2::splat(16.0));
    let id = ui.id().with(("delete-preset", &p.id));
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    painter.circle(rect.center(), 8.0, t.surface, Stroke::new(1.0, t.line));
    let tint = if resp.hovered() { t.ink } else { t.muted };
    Icon::Close
        .image(ui, 9.0)
        .tint(tint)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(9.0)));
    let label = format!("Delete {}", p.name);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    resp
}

/// A tiny page in the look's own paper, texture and grain.
fn paint_page(ui: &Ui, gpu: Option<&Gpu>, page: Rect, look: &Appearance) {
    let c = look.ui_tokens();
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
            painter.rect_filled(page, r, rgb(c.paper));
        }
    }
    let inner = page.shrink2(Vec2::new(page.width() * 0.18, page.height() * 0.24));
    for (i, frac) in [1.0, 0.8, 0.6].into_iter().enumerate() {
        let y = inner.top() + i as f32 * inner.height() / 2.0;
        let line =
            Rect::from_min_size(pos2(inner.left(), y - 1.0), vec2(inner.width() * frac, 2.0));
        painter.rect_filled(line, CornerRadius::same(1), rgb(c.ink).gamma_multiply(0.7));
    }
    painter.circle_filled(page.right_top() + vec2(-7.0, 7.0), 3.0, rgb(c.accent));
    if let (Some(gpu), Some(grain)) = (gpu, FilmGrain::of(look)) {
        let offset = crawl_offset(ui.ctx(), grain);
        let card = (page, CARD_RADIUS);
        painter.add(GrainCallback::card(gpu.grain.clone(), card, grain, offset));
    }
}

/// A preset being named; it lives in egui's memory while the form shows.
#[derive(Clone, Default)]
struct Draft {
    open: bool,
    name: String,
    group: String,
    focus: bool,
}

/// Opens the naming form; the header's + calls it.
fn start_draft(ctx: &Context) {
    ctx.data_mut(|m| {
        let d = m.get_temp_mut_or_default::<Draft>(Id::new(DRAFT));
        d.open = true;
        d.focus = true;
    });
}

/// A preset being named: a name, an optional section, Enter or Save.
fn save_form(ui: &mut Ui, user: &[Preset], look: &Appearance, out: &mut Commands) {
    let id = Id::new(DRAFT);
    let mut d = ui.data(|m| m.get_temp::<Draft>(id)).unwrap_or_default();
    if !d.open {
        return;
    }
    draft_form(ui, user, look, &mut d, out);
    ui.data_mut(|m| m.insert_temp(id, d));
}

fn field<'a>(text: &'a mut String, hint: &str) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text)
        .hint_text(hint.to_owned())
        .desired_width(f32::INFINITY)
}

fn draft_form(ui: &mut Ui, user: &[Preset], look: &Appearance, d: &mut Draft, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let frame = egui::Frame::new()
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(8.0);
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let name = ui.add(field(&mut d.name, "Preset name"));
        widgets::name(&name, "Preset name");
        if std::mem::take(&mut d.focus) {
            name.request_focus();
        }
        let group = ui.add(field(&mut d.group, "Section (optional)"));
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
        let primary = widgets::primary_button(ui, None, "Save", t.accent, &t);
        ui.columns(2, |cols| {
            if let [l, r] = cols {
                save = l
                    .add_enabled_ui(named, |ui| {
                        ui.add_sized([ui.available_width(), 24.0], primary)
                    })
                    .inner
                    .clicked();
                let size = [r.available_width(), 24.0];
                cancel = r.add_sized(size, egui::Button::new("Cancel")).clicked();
            }
        });
        if named && (save || enter) {
            out.push(Command::SavePreset {
                name: d.name.trim().to_owned(),
                group: d.group.trim().to_owned(),
                look: *look,
            });
            *d = Draft::default();
        } else if cancel || escape {
            *d = Draft::default();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strength_reads_as_a_share_of_the_whole_tint() {
        assert_eq!(strength_percent(0), 0);
        assert_eq!(strength_percent(FULL_TINT), 100);
        // Past the whole tint nothing changes, so the dial stays full.
        assert_eq!(strength_percent(100), 100);
        assert_eq!(strength_percent(STRENGTH_ON_HUE), 36);
        for percent in (0..=100).step_by(2) {
            assert_eq!(strength_percent(percent_strength(percent)), percent);
        }
    }

    #[test]
    fn the_strip_runs_round_the_hue_circle() {
        assert_eq!(hue_colour(0.0), hue_colour(360.0));
        let red = hue_colour(0.0);
        assert!(red.r() > red.g() && red.r() > red.b());
        let blue = hue_colour(240.0);
        assert!(blue.b() > blue.r() && blue.b() > blue.g());
    }
}
