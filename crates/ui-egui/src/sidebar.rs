//! One sidebar rail per window, docked or floating; it serves the focused pane.

mod edge;
mod hover;

use egui::emath::easing;
use egui::{
    Align, Context, CornerRadius, Id, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, Vec2,
    pos2, vec2,
};
use serde::{Deserialize, Serialize};

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::motion;
use crate::platform::Platform;
use crate::theme::{self, TITLE_BAR_H, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand, WindowCommand};
use crate::widgets;
use crate::z_order::{self, Tier};

pub const WIDTH: f32 = 288.0;
/// The rail's slide, docked or floating, in seconds.
pub const SLIDE: f32 = 0.3;
/// How long a floating rail waits for the pointer to come back.
pub const HOVER_GRACE: f64 = 0.25;
/// The floating rail's reveal strip at the window's left edge.
pub const EDGE: f32 = 6.0;
/// How long the edge waits while the pointer is in the title bar.
pub const EDGE_DELAY: f64 = 0.5;
const SWITCHER_H: f32 = 54.0;

/// Draws a panel; false shows the placeholder. The flag: reveal the
/// reading place.
pub type PanelBody<'a> = dyn FnMut(&mut Ui, Panel, bool, &mut Commands) -> bool + 'a;

/// What the rail shows; the focused pane supplies the content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Panel {
    #[default]
    Thumbnails,
    Outline,
    Library,
    /// The book's highlights and their notes.
    Notes,
}

impl Panel {
    /// The switcher's buttons, in order.
    pub const SWITCHER: [Panel; 4] = [
        Panel::Thumbnails,
        Panel::Outline,
        Panel::Notes,
        Panel::Library,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Panel::Thumbnails => "Thumbnails",
            Panel::Outline => "Outline",
            Panel::Library => "Library",
            Panel::Notes => "Notes",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [
            Panel::Thumbnails,
            Panel::Outline,
            Panel::Library,
            Panel::Notes,
        ]
        .into_iter()
        .find(|p| p.label().eq_ignore_ascii_case(s))
    }

    fn icon(self) -> Icon {
        match self {
            Panel::Thumbnails => Icon::Thumbnails,
            Panel::Outline => Icon::Outline,
            Panel::Library => Icon::Library,
            Panel::Notes => Icon::Note,
        }
    }

    /// Placeholder copy until the panel's milestone lands.
    fn pending(self) -> &'static str {
        match self {
            Panel::Thumbnails => "Open a PDF to browse its pages here.",
            Panel::Outline => "Open a book to see its chapters here.",
            Panel::Library => "Your library as a tree, with the open tabs, arrives in M5.",
            Panel::Notes => "Open a book to see its highlights and notes here.",
        }
    }
}

/// What the rail needs from the app this frame.
pub struct RailContext {
    /// Settings → Animations → Sidebar slide; off shows end frames only.
    pub slide: bool,
    pub platform: Platform,
    /// The title bar's hover band ends here; the edge waits above it.
    pub bar_bottom: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Sidebar {
    open: bool,
    /// An explicit toggle holds the rail beside the page.
    docked: bool,
    /// Keep the docked layer until its closing slide has finished.
    dock_exit: bool,
    /// An explicit close suppresses hover until the edge is left.
    hover_blocked: bool,
    /// The panel shown, kept while closed so a reopen returns to it.
    panel: Panel,
    /// Floating only: the pointer has reached the rail since it opened.
    armed: bool,
    left_at: Option<f64>,
    /// When the pointer reached the edge strip inside the title bar.
    edge_at: Option<f64>,
    /// The panel should bring the reading place into view.
    recentre: bool,
    /// Last frame's rail edge left of its open place; `None` when not drawn.
    offset: Option<f32>,
}

impl Sidebar {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Whether it is taking space, including its closing slide.
    pub fn is_docked(&self) -> bool {
        self.docked || self.dock_exit
    }

    /// The rail's shift last frame, `-WIDTH` to 0; `None` if not drawn.
    pub fn offset(&self) -> Option<f32> {
        self.offset
    }

    pub fn panel(&self) -> Panel {
        self.panel
    }

    /// Opens persistently on `panel`, or on the last one shown.
    pub fn open(&mut self, panel: Option<Panel>) {
        if let Some(p) = panel {
            self.recentre |= p != self.panel;
            self.panel = p;
        }
        if !self.open {
            self.recentre = true;
        }
        self.open = true;
        self.docked = true;
        self.dock_exit = false;
        self.hover_blocked = false;
        self.armed = false;
        self.left_at = None;
        self.edge_at = None;
    }

    pub fn close(&mut self) {
        self.dock_exit |= self.docked;
        self.docked = false;
        self.open = false;
        self.hover_blocked = true;
        self.armed = false;
        self.left_at = None;
        self.edge_at = None;
    }

    pub fn toggle(&mut self) {
        if self.docked {
            self.close();
        } else {
            self.open(None);
        }
    }

    /// How much of the rail shows, 0 to `WIDTH`, eased.
    fn slide(&self, ctx: &Context, cx: &RailContext, id: &str) -> f32 {
        let secs = if cx.slide {
            motion::secs(ctx, SLIDE)
        } else {
            0.0
        };
        let t = ctx.animate_bool_with_time_and_easing(
            Id::new(id),
            self.open,
            secs,
            easing::cubic_in_out,
        );
        (WIDTH * t).round()
    }

    /// Takes a sliding strip from the left of `ui`; returns its width.
    pub fn show_docked(
        &mut self,
        ui: &mut Ui,
        cx: &RailContext,
        body: &mut PanelBody<'_>,
        out: &mut Commands,
    ) -> f32 {
        let width = self.slide(ui.ctx(), cx, "sidebar-slide");
        self.offset = (width >= 1.0).then_some(width - WIDTH);
        if width < 1.0 {
            if self.dock_exit {
                self.dock_exit = false;
                ui.ctx().request_repaint();
            }
            return 0.0;
        }
        egui::Panel::left("sidebar")
            .exact_size(width)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let strip = ui.max_rect();
                // The rail keeps its width and slides out under the strip.
                let rail = Rect::from_min_size(
                    pos2(strip.right() - WIDTH, strip.top()),
                    vec2(WIDTH, strip.height()),
                );
                let mut child = ui.new_child(UiBuilder::new().max_rect(rail).id_salt("rail"));
                child.shrink_clip_rect(strip);
                self.contents(&mut child, cx, body, out);
                self.edge(ui, strip, out);
            });
        width
    }

    /// Slides over the panes from the left edge; returns the width it covers.
    pub fn show_floating(
        &mut self,
        ctx: &Context,
        cx: &RailContext,
        body: &mut PanelBody<'_>,
        out: &mut Commands,
    ) -> f32 {
        let screen = ctx.content_rect();
        let home = Rect::from_min_size(screen.min, vec2(WIDTH, screen.height()));
        self.track_hover(ctx, cx, home, screen);
        let width = self.slide(ctx, cx, "sidebar-slide");
        self.offset = (width >= 1.0).then_some(width - WIDTH);
        if width < 1.0 {
            return 0.0;
        }
        let rail = home.translate(vec2(width - WIDTH, 0.0));
        // egui fades new areas in by default; this rail only slides.
        z_order::area(ctx, Tier::Sidebar, Id::new("sidebar-float"))
            .fade_in(false)
            .fixed_pos(rail.min)
            .constrain(false)
            .interactable(self.open)
            .show(ctx, |ui| {
                ui.set_min_size(rail.size());
                let t = Tokens::get(ctx);
                let shadow = egui::Shadow {
                    offset: [0, 8],
                    blur: 32,
                    spread: 0,
                    color: t.shadow,
                };
                ui.painter().add(shadow.as_shape(rail, CornerRadius::ZERO));
                let mut child = ui.new_child(UiBuilder::new().max_rect(rail).id_salt("rail"));
                child.set_clip_rect(rail);
                self.contents(&mut child, cx, body, out);
                self.edge(ui, rail, out);
            });
        width
    }

    /// Header, panel and switcher, laid out in `ui.max_rect()`.
    fn contents(
        &mut self,
        ui: &mut Ui,
        cx: &RailContext,
        body: &mut PanelBody<'_>,
        out: &mut Commands,
    ) {
        let t = Tokens::get(ui.ctx());
        let rail = ui.max_rect();
        ui.painter()
            .rect_filled(rail, CornerRadius::ZERO, t.surface);
        let (header, rest) = rail.split_top_bottom_at_y(rail.top() + TITLE_BAR_H);
        let (panel, switcher) = rest.split_top_bottom_at_y(rest.bottom() - SWITCHER_H);
        self.header(ui, header, cx, out);
        let recentre = std::mem::take(&mut self.recentre);
        let builder = UiBuilder::new()
            .max_rect(panel)
            .layout(Layout::top_down(Align::Min));
        let drew = ui
            .scope_builder(builder, |ui| {
                ui.shrink_clip_rect(panel);
                body(ui, self.panel, recentre, out)
            })
            .inner;
        if !drew {
            // Kept for the document still to come.
            self.recentre |= recentre;
            placeholder(ui, panel, self.panel);
        }
        ui.painter()
            .hline(switcher.x_range(), switcher.top(), Stroke::new(1.0, t.line));
        self.switcher(ui, switcher);
    }

    fn header(&self, ui: &mut Ui, rect: Rect, cx: &RailContext, out: &mut Commands) {
        // Registered before the buttons so they win the pointer.
        let drag = ui.interact(rect, Id::new("sidebar-drag"), Sense::CLICK | Sense::DRAG);
        if drag.double_clicked() {
            out.push(Sent::Ui(UiCommand::Window(WindowCommand::ToggleMaximize)));
        } else if drag.drag_started() {
            out.push(Sent::Ui(UiCommand::Window(WindowCommand::StartDrag)));
        }
        let mut row = rect;
        row.min.x += cx.platform.leading_inset();
        row.max.x -= 8.0;
        let builder = UiBuilder::new()
            .max_rect(row)
            .layout(Layout::left_to_right(Align::Center));
        ui.scope_builder(builder, |ui| {
            // The docked rail belongs to title-bar hover motion.
            let _act = anim::announce(ui, Act::Hover);
            ui.spacing_mut().item_spacing.x = 4.0;
            let sidebar_button = if self.docked {
                (
                    Icon::SidebarClose,
                    "Close sidebar",
                    Sent::Ui(UiCommand::CloseSidebar),
                )
            } else {
                (
                    Icon::Sidebar,
                    "Keep sidebar open",
                    Sent::Ui(UiCommand::ToggleSidebar),
                )
            };
            let buttons = [
                sidebar_button,
                (Icon::Search, "Search", Sent::Ui(UiCommand::ShowSearch)),
                (
                    Icon::Settings,
                    "Settings",
                    Sent::Ui(UiCommand::ShowSettings),
                ),
            ];
            for (icon, label, command) in buttons {
                let keys = crate::keymap::hint(cx.platform, &command);
                let r = widgets::icon_button_keyed(ui, icon, label, false, keys.as_deref());
                if r.clicked() {
                    out.push(command);
                }
            }
        });
    }

    /// The panels, each named under its icon.
    fn switcher(&mut self, ui: &mut Ui, rect: Rect) {
        let t = Tokens::get(ui.ctx());
        let _act = anim::announce(ui, Act::Click);
        let slot = rect.width() / Panel::SWITCHER.len() as f32;
        for (i, panel) in Panel::SWITCHER.into_iter().enumerate() {
            let centre = pos2(rect.left() + slot * (i as f32 + 0.5), rect.center().y);
            let at = Rect::from_center_size(centre, vec2(slot - 8.0, SWITCHER_H - 10.0));
            let on = panel == self.panel;
            let label = panel.label();
            let r = ui.interact(at, Id::new(("sidebar-switch", label)), Sense::click());
            r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
            if r.hovered() && !on {
                ui.painter().rect_filled(at, t.radius, t.hover);
            }
            let ink = if on {
                t.accent
            } else if r.hovered() {
                t.ink
            } else {
                t.muted
            };
            let icon = Rect::from_center_size(at.center() - vec2(0.0, 8.0), Vec2::splat(16.0));
            anim::paint(ui, icon, panel.icon(), ink, label, &r);
            let font = if on {
                theme::medium(11.0)
            } else {
                egui::FontId::proportional(11.0)
            };
            let name = at.center() + vec2(0.0, 10.0);
            ui.painter()
                .text(name, egui::Align2::CENTER_CENTER, label, font, ink);
            if r.clicked() && on {
                self.recentre = true;
            } else if r.clicked() {
                self.panel = panel;
                self.recentre = true;
            }
        }
    }
}

fn placeholder(ui: &mut Ui, rect: Rect, panel: Panel) {
    let t = Tokens::get(ui.ctx());
    let area = rect.shrink2(vec2(28.0, 0.0));
    let builder = UiBuilder::new()
        .max_rect(area)
        .layout(Layout::top_down(Align::Center));
    ui.scope_builder(builder, |ui| {
        ui.add_space(area.height() * 0.3);
        ui.add(panel.icon().image(ui, 24.0).tint(t.muted));
        ui.add_space(6.0);
        ui.label(
            RichText::new(panel.label())
                .font(theme::medium(14.0))
                .color(t.ink),
        );
        ui.label(
            RichText::new(panel.pending())
                .size(t.text_small)
                .color(t.muted),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::{Panel, Sidebar};

    #[test]
    fn a_reopen_returns_to_the_last_panel() {
        let mut s = Sidebar::default();
        s.open(Some(Panel::Outline));
        s.toggle();
        assert!(!s.is_open());
        s.toggle();
        assert!(s.is_open());
        assert_eq!(s.panel(), Panel::Outline);
    }

    #[test]
    fn opening_or_switching_panels_asks_for_the_reading_place() {
        let mut s = Sidebar::default();
        s.open(Some(Panel::Outline));
        assert!(std::mem::take(&mut s.recentre));
        s.open(Some(Panel::Outline));
        assert!(!s.recentre);
        s.open(Some(Panel::Thumbnails));
        assert!(s.recentre);
    }

    #[test]
    fn panels_parse_from_their_labels() {
        for p in Panel::SWITCHER {
            assert_eq!(Panel::parse(&p.label().to_lowercase()), Some(p));
        }
        assert_eq!(Panel::parse("nope"), None);
    }
}
