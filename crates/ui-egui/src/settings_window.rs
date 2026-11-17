//! The settings window: a modal with a sidebar of tabs, grouped.

use egui::{
    Align, Button, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea, Sense,
    Stroke, Ui, UiBuilder, Vec2, pos2, vec2,
};
use mareader_appearance::Rgb;
use mareader_settings::{
    AppearanceMenuStyle, Corners, EdgeParking, FloatingLabelStyle, LayoutSettings, LibraryClick,
    PageIndicatorStyle, PaneLook, PaperArea, Settings, StartupFit, Swatch,
};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::modal::Dialog;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;
use mareader_engine::command::Command;

#[cfg(not(target_arch = "wasm32"))]
mod ai;
mod dictionary;
pub use dictionary::{ALL_LANGUAGES, MORE_LANGUAGES};
mod downloads;
mod fonts;
mod highlights;
#[cfg(not(target_arch = "wasm32"))]
mod model_picker;
#[cfg(not(target_arch = "wasm32"))]
mod models;
#[cfg(not(target_arch = "wasm32"))]
mod provider_card;
mod read_aloud;
mod updates;
mod vocabulary;
mod voice_model;
pub use voice_model::accent;

use mareader_engine::scope::{Applies, Scope};
pub use read_aloud::Voices;

/// Its size at most; below that, it takes [`SHARE`] of the window.
pub const MAX_SIZE: Vec2 = vec2(760.0, 960.0);
pub const SHARE: Vec2 = vec2(0.92, 0.86);

/// The window's size in an app window of `screen` points.
pub fn window_size(screen: Vec2) -> Vec2 {
    (screen * SHARE).min(MAX_SIZE)
}
const ROW_H: f32 = 44.0;
/// The sidebar of tabs, and each tab's row in it.
const NAV_W: f32 = 184.0;
const NAV_ROW_H: f32 = 32.0;
/// The voice engines' fold title.
const ENGINES: &str = "Voice engines";
/// How far the engines sit in from the tabs.
const ENGINE_INDENT: f32 = 10.0;
/// The shown tab's name and the close button.
const HEADER_H: f32 = 56.0;
/// Clear space under the page, below the scroll's fade.
const FOOT_H: f32 = 12.0;

use mareader_engine::settings::Tab;

/// Live engine/front-end services read by individual settings tabs.
pub struct Services<'a> {
    pub ai: &'a crate::ai::Ai,
    pub dict: &'a mareader_engine::dict::Service,
    pub vocab: &'a mareader_engine::vocab::Vocab,
    pub voices: &'a Voices<'a>,
    pub updates: &'a mareader_engine::updates::State,
}

#[derive(Clone, Debug, Default)]
pub struct SettingsWindow {
    open: bool,
    tab: Tab,
    /// Opened away from the books: only the tabs that matter there.
    limited: bool,
}

impl SettingsWindow {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens on `tab`, or where it was left; `limited` lists fewer tabs.
    pub fn open(&mut self, tab: Option<Tab>, limited: bool) {
        self.open = true;
        self.limited = limited;
        if let Some(t) = tab {
            self.tab = t;
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// The tab on screen: one not listed falls back to the first listed.
    pub fn tab(&self) -> Tab {
        if self.lists(self.tab) {
            self.tab
        } else if self.limited {
            Tab::ReadAloud
        } else {
            Tab::Appearance
        }
    }

    /// Whether the sidebar names `tab` this time.
    fn lists(&self, tab: Tab) -> bool {
        tab.available() && (!self.limited || tab.beyond_books())
    }

    /// Edits a copy of `current`; any change goes out as one command.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        current: &Settings,
        cx: &Scope,
        services: Services<'_>,
        out: &mut Commands,
    ) {
        if !self.open {
            return;
        }
        let models = services.voices.shelf.ready();
        // A removed model's tab goes; its engine's settings stay.
        if let Tab::Model(m) = self.tab
            && !models.iter().any(|r| r.id == m.id)
        {
            self.tab = Tab::ReadAloud;
        }
        let t = Tokens::get(ctx);
        let size = window_size(ctx.content_rect().size());
        let mut draft = current.clone();
        let mut close = false;
        let modal = Dialog::new("settings-window").margin(0).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal_top(|ui| {
                self.nav(ui, size.y, &models, &current.read_aloud.engine);
                let (line, _) = ui.allocate_exact_size(vec2(1.0, size.y), Sense::hover());
                let edge = Stroke::new(1.0, t.line);
                ui.painter().vline(line.center().x, line.y_range(), edge);
                ui.vertical(|ui| {
                    ui.set_width(size.x - NAV_W - 1.0);
                    let tab = self.tab();
                    close = header(ui, tab);
                    // Short of the rounded corner, fade and all.
                    let room = ui.available_height() - FOOT_H;
                    let body = ScrollArea::vertical()
                        .id_salt(("settings-body", tab.label()))
                        .max_height(room)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            let margin = Margin {
                                left: 24,
                                right: 24,
                                top: 0,
                                bottom: 24,
                            };
                            Frame::new().inner_margin(margin).show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                tab_body(ui, tab, &mut draft, cx, &services, out);
                            });
                        });
                    widgets::scroll_fades(ui, &body, t.surface);
                });
            });
        });
        if close || modal.should_close() {
            out.push(Sent::Ui(UiCommand::CloseSettings));
        }
        if draft != *current {
            out.push(Sent::Engine(Command::SetSettings(Box::new(draft))));
        }
    }

    /// The sidebar: every tab named, under its group's heading.
    fn nav(
        &mut self,
        ui: &mut Ui,
        height: f32,
        models: &[&'static mareader_speech::Model],
        engine: &str,
    ) {
        let t = Tokens::get(ui.ctx());
        let (rect, _) = ui.allocate_exact_size(vec2(NAV_W, height), Sense::hover());
        let r = t.radius_lg as u8;
        let corners = CornerRadius {
            nw: r,
            sw: r,
            ne: 0,
            se: 0,
        };
        ui.painter()
            .rect_filled(rect, corners, t.hover.gamma_multiply(0.5));
        let inner = rect.shrink2(vec2(10.0, 12.0));
        let layout = Layout::top_down(Align::Min);
        let mut ui = ui.new_child(UiBuilder::new().max_rect(inner).layout(layout));
        ui.spacing_mut().item_spacing.y = 2.0;
        let shown = self.tab();
        let mut group = None;
        let tabs: Vec<Tab> = Tab::ALL.into_iter().filter(|&t| self.lists(t)).collect();
        for tab in tabs {
            if group != Some(tab.group()) {
                ui.add_space(if group.is_some() { 14.0 } else { 6.0 });
                nav_heading(&mut ui, tab.group());
                group = Some(tab.group());
            }
            if nav_row(&mut ui, tab, tab == shown).clicked() {
                self.tab = tab;
            }
            // The downloaded models fold under Read aloud.
            if tab == Tab::ReadAloud && !models.is_empty() {
                self.engines(&mut ui, models, engine, shown);
            }
        }
    }

    /// The engines' fold; shut, it keeps the engine in use and the one shown.
    fn engines(
        &mut self,
        ui: &mut Ui,
        models: &[&'static mareader_speech::Model],
        engine: &str,
        shown: Tab,
    ) {
        ui.add_space(4.0);
        let id = egui::Id::new(ENGINES);
        let fold = crate::fold::title(ui, id, ENGINES, Some(models.len()));
        let rows = models.iter().map(|m| {
            let tab = Tab::Model(m);
            (tab, m.id == engine || tab == shown)
        });
        crate::fold::rows(
            ui,
            fold,
            (NAV_ROW_H, ENGINE_INDENT),
            rows,
            |ui, tab, live| {
                let r = nav_row(ui, tab, live && tab == shown);
                if live && r.clicked() {
                    self.tab = tab;
                }
            },
        );
    }
}

/// A small muted heading over a group of tabs.
fn nav_heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let text = RichText::new(text)
            .font(theme::medium(t.text_small))
            .color(t.muted);
        ui.label(text);
    });
    ui.add_space(4.0);
}

/// A tab in the sidebar: its icon and name, filled while shown.
fn nav_row(ui: &mut Ui, tab: Tab, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), NAV_ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let label = tab.label();
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    let fill = if on {
        t.selected
    } else if r.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let icon = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(16.0));
    let tint = if on { t.accent } else { t.muted };
    // Stable row names preserve runs while earlier groups fold.
    let _act = anim::announce(ui, Act::Click);
    ui.push_id(("settings-nav", label), |ui| {
        anim::paint(ui, icon, tab.icon(), tint, label, &r);
    });
    let font = if on {
        theme::medium(t.text)
    } else {
        egui::FontId::proportional(t.text)
    };
    let at = pos2(rect.left() + 38.0, rect.center().y);
    ui.painter()
        .text(at, egui::Align2::LEFT_CENTER, label, font, t.ink);
    r
}

/// The shown tab's name over its page, and the close button; true on close.
fn header(ui: &mut Ui, tab: Tab) -> bool {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), HEADER_H);
    ui.allocate_ui_with_layout(size, Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_height(HEADER_H);
        ui.add_space(12.0);
        let close = widgets::icon_button(ui, Icon::Close, "Close settings", false).clicked();
        // Painted, not a label: the sidebar already names the tab.
        let at = pos2(ui.max_rect().left() + 24.0, ui.max_rect().center().y);
        let font = theme::medium(18.0);
        ui.painter()
            .text(at, egui::Align2::LEFT_CENTER, tab.label(), font, t.ink);
        close
    })
    .inner
}

fn tab_body(
    ui: &mut Ui,
    tab: Tab,
    s: &mut Settings,
    cx: &Scope,
    live: &Services<'_>,
    out: &mut Commands,
) {
    match tab {
        Tab::Appearance => appearance(ui, s, cx, out),
        Tab::Layout => layout(ui, s, cx),
        Tab::ReadAloud => read_aloud::tab(ui, &mut s.read_aloud, live.voices, out),
        Tab::Model(m) => voice_model::tab(ui, &mut s.read_aloud, m, live.voices, out),
        Tab::Motion => motion(ui, s, cx),
        Tab::Highlights => {
            let marks = highlights::Marks {
                ai: cx.explains && Tab::Ai.available(),
                shading: *live.vocab.state(mareader_cefr::Part::Dataset)
                    == mareader_cefr::State::Ready,
            };
            highlights::tab(ui, &mut s.study.highlights, marks, out);
        }
        Tab::Vocabulary => {
            let heard = (live.vocab, live.voices.sounds, live.voices.english.as_ref());
            vocabulary::tab(ui, (&mut s.study, &mut s.dictionary), heard, out);
        }
        Tab::Dictionary => dictionary::tab(ui, &mut s.dictionary, live.dict, out),
        #[cfg(not(target_arch = "wasm32"))]
        Tab::Ai => {
            let looks = (&mut s.study.highlights, &mut s.study.gloss);
            ai::tab(ui, &mut s.ai, looks, live.ai.board(), out);
        }
        #[cfg(not(target_arch = "wasm32"))]
        Tab::Models => {
            let in_use = |id| live.ai.in_use(id);
            let live_m = live.ai.board().map(|board| models::Live {
                board,
                in_use: &in_use,
            });
            models::models_tab(ui, &mut s.ai, live_m, out);
        }
        #[cfg(target_arch = "wasm32")]
        Tab::Ai | Tab::Models => pending(ui, "AI answers need the desktop app."),
        Tab::Updates => updates::tab(ui, &mut s.updates, live.updates, out),
        Tab::Panes => panes(ui, s, cx),
    }
    let _ = live.ai;
}

fn layout(ui: &mut Ui, s: &mut Settings, cx: &Scope) {
    rows(ui, cx, Applies::Always, "Reader chrome", |r| {
        let c = &mut s.chrome;
        r.always(|ui| {
            let hint = "Pin the title bar so it never hides";
            switch_row(ui, "Keep title bar shown", hint, &mut c.titlebar_pinned);
        });
        r.always(|ui| {
            let hint = "The sidebar floats over pages and hides when the pointer leaves";
            switch_row(ui, "Overlay sidebar", hint, &mut c.sidebar_overlay);
        });
        r.always(|ui| {
            let hint = "Blur the page behind the title bar and page bar; off makes both solid";
            switch_row(ui, "Frosted glass", hint, &mut c.glass);
        });
    });
    rows(ui, cx, Applies::Always, "Page position", |r| {
        let l = &mut s.layout;
        r.always(|ui| {
            let hint = "A page counter in the corner of each pane";
            switch_row(ui, "Page indicator", hint, &mut l.page_indicator);
        });
        if l.page_indicator {
            r.always(|ui| {
                row(ui, "Indicator shows", "", |ui| {
                    let style = &mut l.page_indicator_style;
                    ui.selectable_value(style, PageIndicatorStyle::Percentage, "Percent");
                    ui.selectable_value(style, PageIndicatorStyle::PageNumber, "Pages");
                });
            });
        }
        r.always(|ui| {
            let hint = "A thin line along each pane's bottom edge";
            switch_row(ui, "Progress bar", hint, &mut l.progress_bar);
        });
    });
    rows(ui, cx, Applies::Always, "Page label", |r| {
        label_rows(r, &mut s.layout)
    });
    rows(ui, cx, Applies::Always, "Pages", |r| {
        page_rows(r, &mut s.layout)
    });
}

/// The floating document label; its options show once it's on.
fn label_rows(r: &mut Rows<'_>, l: &mut LayoutSettings) {
    r.always(|ui| {
        let hint = "The document's name in each pane's corner while the title bar hides";
        switch_row(ui, "Floating label", hint, &mut l.floating_label);
    });
    if !l.floating_label {
        return;
    }
    r.always(|ui| {
        row(ui, "Label shows", "", |ui| {
            let style = &mut l.floating_label_style;
            ui.selectable_value(style, FloatingLabelStyle::Chapter, "Chapter");
            ui.selectable_value(style, FloatingLabelStyle::FileName, "File name");
        });
    });
    r.always(|ui| {
        let hint = "Keep it while the title bar shows, however narrow the side";
        switch_row(ui, "Always show label", hint, &mut l.floating_label_persist);
    });
    if l.floating_label_persist {
        return;
    }
    r.always(|ui| {
        let hint = "How much of the room beside the page the label may take";
        row(ui, "Label width limit", hint, |ui| {
            let pct = &mut l.floating_label_max_pct;
            let show = |v: f32| format!("{v:.0}%");
            widgets::stepper(ui, pct, 10.0..=100.0, 10.0, show, "label width");
        });
    });
}

/// How pages sit: the fit they open at, their spacing and shadow.
fn page_rows(r: &mut Rows<'_>, l: &mut LayoutSettings) {
    let sideways = r.cx.sideways;
    r.always(|ui| {
        row(ui, "Default fit", "How a document opens", |ui| {
            ui.selectable_value(&mut l.startup_fit, StartupFit::Width, "Width");
            ui.selectable_value(&mut l.startup_fit, StartupFit::Page, "Page");
        });
    });
    // A book's vertical stream has neither gaps nor shadows.
    r.add(Applies::Pages, |ui| {
        let hint = "Pages in the scrolling views touch";
        switch_row(ui, "No gap", hint, &mut l.no_gap);
    });
    r.always(|ui| {
        let hint = if sideways {
            "Pages scrolling sideways have no margin"
        } else {
            "Space beside the pages"
        };
        row(ui, "Page margin", hint, |ui| {
            ui.add_enabled_ui(!sideways, |ui| {
                let show = |v: f32| {
                    if v <= 0.0 {
                        "Off".to_owned()
                    } else {
                        format!("{v:.0} px")
                    }
                };
                let margin = &mut l.page_margin;
                widgets::stepper(ui, margin, 0.0..=64.0, 4.0, show, "page margin");
            });
        });
    });
    // A book's pages are all one size.
    r.add(Applies::NotBook, |ui| {
        let hint = "A zoom set by hand shrinks with the pane, like a fit, never past itself";
        switch_row(ui, "Scale my zoom with the pane", hint, &mut l.auto_resize);
    });
    r.add(Applies::Pages, |ui| {
        switch_row(ui, "Page shadow", "", &mut l.page_shadow);
    });
}

fn motion(ui: &mut Ui, s: &mut Settings, cx: &Scope) {
    let m = &mut s.motion;
    rows(ui, cx, Applies::Always, "All motion", |r| {
        r.always(|ui| {
            let hint = "Everything that moves. Off, a change lands as its end frame.";
            switch_row(ui, "Animations", hint, &mut m.enabled);
        });
    });
    if !m.enabled {
        note(ui, "Turn Animations on to choose each motion.");
        return;
    }
    rows(ui, cx, Applies::Always, "Each motion", |r| {
        r.always(|ui| {
            let hint = "Icons move when controls are reached or pressed";
            switch_row(ui, "Icon animations", hint, &mut m.icon_animations);
        });
        r.always(|ui| {
            let hint = "The sidebar slides in and out, docked or floating";
            switch_row(ui, "Sidebar animation", hint, &mut m.sidebar_slide);
        });
        r.always(|ui| {
            let hint = "Re-fit the page while the window is dragged; off, once the drag ends";
            switch_row(ui, "Canvas follows window", hint, &mut m.canvas_resize);
        });
        r.always(|ui| {
            let hint = "Ease a zoom to its new scale; off, it lands on the first frame";
            switch_row(ui, "Zoom in and out", hint, &mut m.zoom);
        });
        r.always(|ui| {
            let hint = "Glide to a page, and with the arrow and page keys; off, land there";
            switch_row(ui, "Scroll to page", hint, &mut m.scroll_jumps);
        });
    });
    note(ui, "Anything switched off still changes, in one frame.");
}

/// The row that swaps this window for the Appearance popover.
pub(super) fn colours_row(ui: &mut Ui, out: &mut Commands) {
    let hint = "Opens over the page, so each change shows as you make it";
    row(ui, "Mode, tint, texture and presets", hint, |ui| {
        let t = Tokens::get(ui.ctx());
        let text = RichText::new("Open Appearance").color(t.accent);
        if ui.add(Button::new(text).fill(t.accent_soft)).clicked() {
            out.push(Sent::Ui(UiCommand::CloseSettings));
            out.push(Sent::Ui(UiCommand::ShowAppearance));
        }
    });
}

/// Chooses between the classic Appearance list and the tabbed Dial deck.
pub(super) fn appearance_menu_row(ui: &mut Ui, style: &mut AppearanceMenuStyle) {
    let hint = "Compact tabs with a colour wheel and rotary dials, or the classic list";
    row(ui, "Appearance menu", hint, |ui| {
        // Right to left: Classic lands on the left, Dial deck on the right.
        for v in [AppearanceMenuStyle::DialDeck, AppearanceMenuStyle::Classic] {
            ui.selectable_value(style, v, v.label());
        }
    });
}

/// Colours stay in the popover, where the page shows each change.
fn appearance(ui: &mut Ui, s: &mut Settings, cx: &Scope, out: &mut Commands) {
    // A book's colours close its own groups, beside its ink.
    let book = cx.applies(Applies::Book);
    if !book {
        rows(ui, cx, Applies::Always, "Colours", |r| {
            r.always(|ui| colours_row(ui, out));
            r.always(|ui| appearance_menu_row(ui, &mut s.chrome.appearance_menu));
        });
    }
    if book {
        fonts::tab(ui, &mut s.text, &mut s.chrome.appearance_menu, cx, out);
    } else if cx.applies(Applies::AiPages) {
        fonts::ai_tab(ui, &mut s.text);
    } else {
        section(ui, "Text");
        let text = "Open an EPUB, Markdown or text file to set its type. \
            A PDF takes type while one of its AI pages shows.";
        note(ui, text);
    }
    // Read from page rasters, so only PDFs have it.
    rows(ui, cx, Applies::NotBook, "Paper", |r| paper(r, s));
}

fn paper(r: &mut Rows<'_>, s: &mut Settings) {
    let p = &mut s.paper;
    r.always(|ui| {
        let hint = "The background takes the page's own paper colour";
        switch_row(ui, "Blend mode", hint, &mut p.blend);
    });
    r.always(|ui| {
        row(ui, "Detection", "Where the paper colour is read", |ui| {
            ui.add_enabled_ui(p.blend, |ui| {
                // Right to left: the last option is drawn first.
                let options = [
                    (PaperArea::Edges, "Edges"),
                    (PaperArea::WholePage, "Whole page"),
                ];
                for (area, label) in options {
                    ui.selectable_value(&mut p.area, area, label);
                }
            });
        });
    });
}

/// Touching panes hide corners and shadows, so picking one opens a gap.
const EDGE_HINT: &str = "Shows between panes; picking it opens a gap if they touch";

fn pane_look(r: &mut Rows<'_>, w: &mut PaneLook) {
    r.always(|ui| {
        row(ui, "Outline thickness", "Zero hides the outline", |ui| {
            ui.spacing_mut().slider_width = 140.0;
            ui.add(
                egui::Slider::new(&mut w.outline_width, 0.0..=8.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" px"),
            );
        });
    });
    r.always(|ui| {
        row(ui, "Outline colour", "The focused pane's outline", |ui| {
            outline_colours(ui, &mut w.outline_color, &mut w.outline_custom);
        });
    });
    r.always(|ui| {
        row(ui, "Space around and between panes", "", |ui| {
            ui.spacing_mut().slider_width = 140.0;
            ui.add(
                egui::Slider::new(&mut w.gap, 0.0..=24.0)
                    .step_by(1.0)
                    .fixed_decimals(0)
                    .suffix(" px"),
            );
        });
    });
    r.always(|ui| {
        row(ui, "Corners", EDGE_HINT, |ui| {
            // Right to left: the last option is drawn first.
            for (corners, label) in [(Corners::Rounded, "Rounded"), (Corners::Square, "Square")] {
                if ui
                    .add(Button::selectable(w.corners == corners, label))
                    .clicked()
                {
                    w.corners = corners;
                    if w.rounded() {
                        w.make_room();
                    }
                }
            }
        });
    });
    r.always(|ui| {
        let was = w.shadow;
        switch_row(ui, "Pane drop shadow", EDGE_HINT, &mut w.shadow);
        if w.shadow && !was {
            w.make_room();
        }
    });
}

/// Laid out right to left: the custom picker, then the palette.
fn outline_colours(ui: &mut Ui, choice: &mut Swatch, custom: &mut Rgb) {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.x = 6.0;
    let mut srgb = [custom.0, custom.1, custom.2];
    ui.spacing_mut().interact_size = Vec2::splat(20.0);
    let picker = egui::color_picker::color_edit_button_srgb(ui, &mut srgb);
    if picker.clicked() || srgb != [custom.0, custom.1, custom.2] {
        *custom = Rgb(srgb[0], srgb[1], srgb[2]);
        *choice = Swatch::Custom;
    }
    widgets::tip(&picker, "Custom", None);
    for c in Swatch::PALETTE.into_iter().rev() {
        let (rect, r) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
        let fill = c.rgb(*custom).map_or(t.accent, theme::color);
        let on = *choice == c;
        ui.painter().circle_filled(rect.center(), 9.0, fill);
        if on {
            ui.painter()
                .circle_stroke(rect.center(), 10.5, Stroke::new(1.5, t.ink));
        }
        let label = c.label();
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
        widgets::tip(&r, label, None);
        if r.clicked() {
            *choice = c;
        }
    }
}

/// The Library panel's click choices, in the tab's order.
const LIBRARY_CLICKS: [(LibraryClick, &str, &str); 3] = [
    (
        LibraryClick::Replace,
        "Open in focused pane",
        "The book replaces what the focused pane shows.",
    ),
    (
        LibraryClick::Split,
        "Open as a new split",
        "The book opens in a new pane beside the focused one.",
    ),
    (
        LibraryClick::DragOnly,
        "Do nothing",
        "Books open by dragging them onto a pane, or with Enter.",
    ),
];

fn panes(ui: &mut Ui, s: &mut Settings, cx: &Scope) {
    let w = &mut s.workspace;
    section(ui, "Library panel");
    group(ui, |ui| {
        for (i, (choice, title, detail)) in LIBRARY_CLICKS.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, w.library_click == choice) {
                w.library_click = choice;
            }
        }
    });
    let page = "The Library page follows this too; with Do nothing it opens in the focused pane.";
    note(ui, page);
    section(ui, "Floating windows at the edge");
    group(ui, |ui| {
        for (i, (choice, title, detail)) in [
            (EdgeParking::Off, "Off", "Keep floating windows on screen"),
            (
                EdgeParking::Sliver,
                "Window edge",
                "Leave part of the window visible",
            ),
            (
                EdgeParking::Tab,
                "Edge tab",
                "Show a labeled tab at the edge",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, w.edge_parking == choice) {
                w.edge_parking = choice;
            }
        }
    });
    let title = "Split pane appearance";
    rows(ui, cx, Applies::Split, title, |r| pane_look(r, &mut w.look));
    rows(ui, cx, Applies::Always, "Pane themes", |r| {
        r.always(|ui| {
            let hint = "While split, each pane keeps its own colour; grain stays shared";
            let label = "Independent theme for each pane";
            switch_row(ui, label, hint, &mut w.independent_themes);
        });
        // Only means something once each pane has its own theme.
        if w.independent_themes {
            r.always(|ui| {
                let hint = "Off: each pane keeps its own mode too";
                let label = "Light, Dark and Dim change every pane";
                switch_row(ui, label, hint, &mut w.shared_base_mode);
            });
        }
        r.always(|ui| {
            let hint = "While split, each pane keeps its own texture and dials";
            let label = "Independent page texture for each pane";
            switch_row(ui, label, hint, &mut w.independent_textures);
        });
    });
    // Text settings reach books and AI pages; other PDFs don't need it.
    rows(ui, cx, Applies::SplitWithText, "Pane text", |r| {
        r.always(|ui| {
            let hint = "While split, the Fonts tab changes only the focused pane";
            let label = "Independent text for each pane";
            switch_row(ui, label, hint, &mut w.independent_text);
        });
    });
}

/// A titled group where `applies`, its rows each where they apply.
fn rows(ui: &mut Ui, cx: &Scope, applies: Applies, title: &str, add: impl FnOnce(&mut Rows<'_>)) {
    if !cx.applies(applies) {
        return;
    }
    section(ui, title);
    group(ui, |ui| {
        add(&mut Rows {
            ui,
            cx: *cx,
            drawn: 0,
        });
    });
}

/// A group's rows; dividers fall only between rows drawn.
struct Rows<'a> {
    ui: &'a mut Ui,
    cx: Scope,
    drawn: usize,
}

impl Rows<'_> {
    fn add(&mut self, applies: Applies, row: impl FnOnce(&mut Ui)) {
        if !self.cx.applies(applies) {
            return;
        }
        if self.drawn > 0 {
            divider(self.ui);
        }
        row(self.ui);
        self.drawn += 1;
    }

    fn always(&mut self, row: impl FnOnce(&mut Ui)) {
        self.add(Applies::Always, row);
    }
}

fn section(ui: &mut Ui, title: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(14.0);
    ui.label(
        RichText::new(title)
            .font(theme::medium(t.text_small))
            .color(t.muted),
    );
    ui.add_space(6.0);
}

/// A filled, bordered block of rows.
fn group(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ui.ctx());
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .inner_margin(Margin::symmetric(14, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui);
        });
}

fn divider(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t.line));
}

/// Label on the left (its hint as a tooltip), control on the right.
fn row(ui: &mut Ui, label: &str, hint: &str, control: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(ROW_H);
            let r = ui.label(label);
            if !hint.is_empty() {
                widgets::tip(&r, hint, None);
            }
            widgets::labelled(ui, r.id, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), control);
            });
        },
    );
}

fn switch_row(ui: &mut Ui, label: &str, hint: &str, on: &mut bool) {
    row(ui, label, hint, |ui| {
        widgets::switch(ui, on, label);
    });
}

/// A row that shows the rows under it; true while open.
fn disclosure(ui: &mut Ui, label: &str, hint: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let id = ui.id().with(("disclosure", label));
    let mut open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let size = vec2(ui.available_width(), ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, label));
    if !hint.is_empty() {
        widgets::tip(&r, hint, None);
    }
    if r.clicked() {
        open = !open;
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    let ink = if r.hovered() { t.ink } else { t.muted };
    let font = egui::FontId::proportional(t.text);
    let left = pos2(rect.left(), rect.center().y);
    ui.painter()
        .text(left, egui::Align2::LEFT_CENTER, label, font, ink);
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let mark = Rect::from_center_size(pos2(rect.right() - 9.0, rect.center().y), Vec2::splat(16.0));
    chevron.image(ui, 16.0).tint(ink).paint_at(ui, mark);
    open
}

/// The fold asked to show itself next time it is drawn.
const REVEAL: &str = "settings-fold-reveal";
/// How long a revealed fold glows.
const GLOW: f64 = 1.6;

/// Asks the [`fold_row`] `id` to open, come into view and glow once.
pub fn reveal(ctx: &egui::Context, id: egui::Id) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(REVEAL), id));
}

/// The fold of a model's voices in `language`, as `fr-FR`, on its tab.
pub fn voice_fold(model: &mareader_speech::Model, language: &str) -> egui::Id {
    voice_model::fold_id(model, language)
}

/// Whether the [`fold_row`] `id` is open.
pub fn fold_open(ctx: &egui::Context, id: egui::Id) -> bool {
    ctx.data(|d| d.get_temp::<bool>(id)) == Some(true)
}

/// Two lines folding the rows below; true while open.
/// Folded until opened, or [`reveal`]ed.
fn fold_row(ui: &mut Ui, id: egui::Id, (title, detail): (&str, &str)) -> bool {
    let t = Tokens::get(ui.ctx());
    let kept = ui.data(|d| d.get_temp::<bool>(id));
    let mut open = kept.unwrap_or(false);
    let size = vec2(ui.available_width(), 52.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, open, title));
    let now = ui.input(|i| i.time);
    let asked = egui::Id::new(REVEAL);
    if ui.data(|d| d.get_temp::<egui::Id>(asked)) == Some(id) {
        open = true;
        ui.data_mut(|d| {
            d.remove::<egui::Id>(asked);
            d.insert_temp(id.with("glow"), now);
        });
        r.scroll_to_me(None);
    }
    if r.clicked() {
        open = !open;
    }
    if r.clicked() || kept != Some(open) {
        ui.data_mut(|d| d.insert_temp(id, open));
    }
    glow(ui, id, rect, now);
    let mid = rect.center().y;
    let ink = if r.hovered() { t.accent } else { t.ink };
    let text = Rect::from_min_max(rect.min, pos2(rect.right() - 26.0, rect.bottom()));
    let painter = ui.painter().with_clip_rect(text);
    let font = egui::FontId::proportional(t.text);
    let top = pos2(rect.left(), mid - 9.0);
    painter.text(top, egui::Align2::LEFT_CENTER, title, font, ink);
    let small = egui::FontId::proportional(t.text_small);
    let below = pos2(rect.left(), mid + 9.0);
    painter.text(below, egui::Align2::LEFT_CENTER, detail, small, t.muted);
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    let mark = Rect::from_center_size(pos2(rect.right() - 9.0, mid), Vec2::splat(16.0));
    chevron.image(ui, 16.0).tint(t.muted).paint_at(ui, mark);
    open
}

/// A revealed fold's glow: the accent's tint, pulsing twice as it fades.
fn glow(ui: &Ui, id: egui::Id, rect: Rect, now: f64) {
    let Some(from) = ui.data(|d| d.get_temp::<f64>(id.with("glow"))) else {
        return;
    };
    let age = now - from;
    if !(0.0..GLOW).contains(&age) {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let fade = 1.0 - (age / GLOW) as f32;
    let pulse = 0.6 + 0.4 * (age * std::f64::consts::TAU * 1.25).cos() as f32;
    let strength = (fade * pulse).clamp(0.0, 1.0);
    let spot = rect.expand2(vec2(6.0, 0.0));
    let fill = t.accent_soft.gamma_multiply(strength);
    let edge = Stroke::new(1.5, t.accent.gamma_multiply(strength));
    (ui.painter()).rect(spot, t.radius, fill, edge, egui::StrokeKind::Inside);
    ui.ctx().request_repaint();
}

/// The rows under an open [`fold_row`], set in a step.
fn folded(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .inner_margin(Margin {
            left: 14,
            ..Margin::ZERO
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// A titled choice with its detail and a radio mark; true when picked.
fn choice_row(ui: &mut Ui, title: &str, detail: &str, on: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 52.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, title));
    let painter = ui.painter();
    let left = rect.left();
    let font = egui::FontId::proportional(t.text);
    let top = pos2(left, rect.center().y - 9.0);
    painter.text(top, egui::Align2::LEFT_CENTER, title, font, t.ink);
    let small = egui::FontId::proportional(t.text_small);
    let below = pos2(left, rect.center().y + 9.0);
    painter.text(below, egui::Align2::LEFT_CENTER, detail, small, t.muted);
    let at = pos2(rect.right() - 9.0, rect.center().y);
    mark(painter, &t, at, on, resp.hovered());
    resp.clicked() && !on
}

/// The settings radio ring, filled when `on`.
fn mark(painter: &egui::Painter, t: &Tokens, at: egui::Pos2, on: bool, hot: bool) {
    // Off reads like an off switch: its track, not the faint line.
    let ring = if on || hot { t.accent } else { t.track };
    painter.circle_stroke(at, 8.0, Stroke::new(1.5, ring));
    if on {
        painter.circle_filled(at, 4.5, t.accent);
    }
}

/// Each of `all` as a selectable button, laid right to left in a row.
fn choices<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> &'static str,
) {
    for &choice in all.iter().rev() {
        let button = egui::Button::selectable(*value == choice, label(choice));
        if ui.add(button).clicked() {
            *value = choice;
        }
    }
}

/// A lone settings radio named `name`; true when newly picked.
fn radio(ui: &mut Ui, on: bool, name: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, name));
    mark(ui.painter(), &t, rect.center(), on, resp.hovered());
    resp.clicked() && !on
}

/// A [`detail_row_tip`] picked anywhere; no radio without `on`.
fn pick_row(
    ui: &mut Ui,
    lines: (&str, &str),
    tip: &str,
    on: Option<bool>,
    control: impl FnOnce(&mut Ui),
) -> bool {
    let rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 52.0));
    // Laid first, so the controls on top keep their own clicks.
    let id = ui.id().with(("pick_row", lines.0));
    let whole = on.map(|_| ui.interact(rect, id, Sense::click()));
    let mut picked = whole.is_some_and(|w| w.clicked());
    flagged_row(ui, lines, tip, None, |ui| {
        if let Some(on) = on {
            picked |= radio(ui, on, lines.0);
        }
        control(ui);
    });
    picked && on == Some(false)
}

/// A row named on two lines, its controls at the right.
fn detail_row(ui: &mut Ui, title: &str, detail: &str, control: impl FnOnce(&mut Ui)) {
    detail_row_tip(ui, (title, detail), "", control);
}

/// A [`detail_row`] whose lines tell more on hover.
fn detail_row_tip(
    ui: &mut Ui,
    (title, detail): (&str, &str),
    tip: &str,
    control: impl FnOnce(&mut Ui),
) {
    flagged_row(ui, (title, detail), tip, None, control);
}

/// A [`detail_row_tip`] whose detail can wear an icon in its colour.
fn flagged_row(
    ui: &mut Ui,
    (title, detail): (&str, &str),
    tip: &str,
    flag: Option<(Icon, Color32)>,
    control: impl FnOnce(&mut Ui),
) {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), 52.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    // Controls first: the lines take the room they leave, cut short.
    let name = ui.id().with(("detail_row", title));
    let builder = UiBuilder::new()
        .max_rect(rect)
        .layout(Layout::right_to_left(Align::Center));
    let mut right = ui.new_child(builder);
    widgets::labelled(&mut right, name, control);
    let room = (rect.width() - right.min_rect().width() - 16.0).max(40.0);
    let fit = |text: RichText, room: f32| {
        let wrap = Some(egui::TextWrapMode::Truncate);
        egui::WidgetText::from(text).into_galley(ui, wrap, room, egui::TextStyle::Body)
    };
    // A flag's icon leads the detail, the text after it.
    const MARK: f32 = 13.0;
    let indent = if flag.is_some() { MARK + 5.0 } else { 0.0 };
    let tone = flag.map_or(t.muted, |(_, c)| c);
    let top = fit(RichText::new(title).size(t.text).color(t.ink), room);
    let low = RichText::new(detail).size(t.text_small).color(tone);
    let low = fit(low, room - indent);
    let gap = 2.0;
    let y = rect.center().y - (top.size().y + gap + low.size().y) / 2.0;
    let at = pos2(rect.left(), y);
    let top_rect = Rect::from_min_size(at, top.size());
    let low_at = pos2(rect.left() + indent, top_rect.bottom() + gap);
    let cut = top.elided || low.elided;
    if let Some((icon, color)) = flag {
        let mid = pos2(rect.left() + MARK / 2.0, low_at.y + low.size().y / 2.0);
        let mark = Rect::from_center_size(mid, Vec2::splat(MARK));
        icon.image(ui, MARK).tint(color).paint_at(ui, mark);
    }
    let painter = ui.painter();
    painter.galley(at, top, t.ink);
    painter.galley(low_at, low, tone);
    let named = ui.interact(top_rect, name, Sense::hover());
    named.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
    let told = match (cut, tip.is_empty()) {
        (_, false) => tip.to_owned(),
        (true, true) => format!("{title}\n{detail}"),
        (false, true) => return,
    };
    let lines = Rect::from_min_max(at, pos2(rect.left() + room, rect.bottom()));
    let whole = ui.interact(lines, name.with("whole"), Sense::hover());
    whole.on_hover_text(told);
}

/// Honest copy for settings whose feature has not landed yet.
#[cfg(target_arch = "wasm32")]
fn pending(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        ui.add(Icon::Clock.image(ui, 13.0).tint(t.muted));
        ui.label(RichText::new(text).size(t.text_small).color(t.muted));
    });
}

fn note(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(8.0);
    ui.label(RichText::new(text).size(t.text_small).color(t.muted));
}

/// A tab's icon: presentation, so it stays above the line.
pub(crate) trait TabIcon {
    fn icon(self) -> Icon;
}

impl TabIcon for Tab {
    fn icon(self) -> Icon {
        match self {
            Tab::Appearance => Icon::Appearance,
            Tab::Layout => Icon::Layout,
            Tab::ReadAloud => Icon::ReadAloud,
            Tab::Highlights => Icon::Highlight,
            Tab::Vocabulary => Icon::Levels,
            Tab::Dictionary => Icon::Dictionary,
            Tab::Ai => Icon::Ai,
            Tab::Models => Icon::Chip,
            Tab::Panes => Icon::SplitRight,
            Tab::Motion => Icon::Motion,
            Tab::Updates => Icon::Download,
            Tab::Model(model) => Icon::engine(model.id).unwrap_or(Icon::VoiceModel),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_grows_with_the_app_past_mareader_s_640() {
        let tall = window_size(vec2(1600.0, 1100.0));
        assert!(tall.y > 900.0 && tall.x > 700.0, "{tall:?}");
        assert_eq!(window_size(vec2(5000.0, 5000.0)), MAX_SIZE);
        let small = window_size(vec2(500.0, 400.0));
        assert!(small.x < 500.0 && small.y < 400.0, "{small:?}: it fits");
    }
}
