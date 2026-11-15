//! The auto-hiding title bar, kept open by the pin or its popups.

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align, Context, Id, Layout, Order, Rect, Sense, UiBuilder, pos2, vec2};
use mareader_appearance::{Appearance, Preset};
use mareader_library::{ShelfId, View};
use mareader_settings::AppearanceMenuStyle;

use crate::backdrop::Backdrop;
use crate::gpu::{Gpu, Look, Reach};
use crate::icons::Icon;
use crate::keymap;
use crate::library::menu as library_menu;
use crate::library::{Field, crumbs, search};
use crate::motion;
use crate::platform::Platform;
use crate::theme::{self, TITLE_BAR_H, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand, WindowCommand};
use crate::view_menu::{self, MenuContext, Tool};
use crate::widgets::{self, CaptionStyle};
use crate::zoom::ZoomState;
use mareader_engine::command::Independence;
use mareader_engine::library::Crumb;
use mareader_engine::pane::ViewMode;

/// How long the bar stays after the pointer leaves the band.
pub const HIDE_GRACE: f64 = 0.4;
pub const FADE: f32 = 0.2;
/// Appearance flyout inset from the window's right edge.
pub const FLYOUT_MARGIN: f32 = 8.0;
pub const APPEARANCE_POPOVER: &str = "appearance-popover";
pub const DICTIONARY_MENU: &str = "dictionary-page-menu";
const MENU_W: f32 = 248.0;
/// A toolbar button and the gap after it.
const TOOL_W: f32 = 40.0;
/// Width the title keeps before tools fold into the menu.
const TITLE_ROOM: f32 = 160.0;
/// The library's search field: its widest and its narrowest.
const SEARCH_W: f32 = 380.0;
const SEARCH_MIN: f32 = 140.0;

/// What the bar needs from the app this frame.
pub struct BarContext<'a> {
    pub title: &'a str,
    /// Whether this page's bar is pinned. The settings own it.
    pub pinned: bool,
    pub platform: Platform,
    /// The sidebar's width: the band starts after it.
    pub left_inset: f32,
    pub maximized: bool,
    /// No captions or traffic lights in full screen.
    pub fullscreen: bool,
    /// The row starts after the platform's leading room (macOS's lights).
    pub gutter: bool,
    /// The look the popover edits: the active pane's, or the window's.
    pub appearance: &'a Appearance,
    /// Which layout the Appearance popover opens in.
    pub appearance_menu: AppearanceMenuStyle,
    /// Every preset, built-ins first, and the user's own among them.
    pub presets: &'a [Preset],
    pub user_presets: &'a [Preset],
    /// Lets the popover draw preset cards as real pages.
    pub gpu: Option<&'a Gpu>,
    /// What shows behind the bar: glass, a tint or a solid fill.
    pub backdrop: &'a Backdrop,
    /// The route's own controls.
    pub page: BarPage<'a>,
}

/// What sets each route's bar apart.
pub enum BarPage<'a> {
    Reader(ReaderBar),
    Library(LibraryBar<'a>),
    Dictionary(DictionaryBar),
}

/// The dictionary's bar: a way to the library, and back.
pub struct DictionaryBar {
    /// The pressed Dictionary button's name: where it returns to.
    pub back: &'static str,
}

/// Rail, folding tools and the overflow menu act on the panes.
pub struct ReaderBar {
    /// The bar's sidebar toggle; the rail's own header closes it.
    pub sidebar_toggle: bool,
    /// A docked, open sidebar, whose header has Search and Settings.
    pub rail_docked: bool,
    /// The per-pane modes; offered only while the workspace is split.
    pub independence: Option<Independence>,
    /// The active document's zoom, for the menu's zoom row.
    pub zoom: Option<ZoomState>,
    pub mode: Option<ViewMode>,
    /// Whether the active pane auto-scrolls, if it shows a document.
    pub auto_scroll: Option<bool>,
    /// Whether a book reads aloud, once the active pane shows one.
    pub aloud: Option<bool>,
    /// The ink dial, offered while the active pane shows a book.
    pub ink: Option<f32>,
    /// The popover's text dials, offered while the active pane shows a book.
    pub text: Option<mareader_engine::appearance_menu::QuickText>,
    /// The floating Dictionary shows, so its button is pressed.
    pub dictionary: bool,
}

/// The shelf has no rail, panes or page: View options and Appearance.
pub struct LibraryBar<'a> {
    pub view: &'a View,
    /// The grid's automatic column count, for the menu's stepper.
    pub auto_columns: u8,
    /// Panes wait behind the page; the pressed Library button returns.
    pub can_return: bool,
    /// The way up from the open shelf; empty at the top level.
    pub trail: Vec<Crumb>,
    pub renaming: Option<ShelfId>,
    /// The search field, centred where the reader's title sits.
    pub search: Field,
    /// The AI is organizing: the menu can't start another run.
    pub organizing: bool,
    /// How far a stopped run got: the menu offers to resume it.
    pub paused: Option<String>,
}

/// Foldable tools, rightmost first; Settings and the pin live in the menu.
fn tools() -> [Tool; 1] {
    [Tool {
        icon: Icon::Search,
        label: "Search",
        command: Sent::Ui(UiCommand::ShowSearch),
    }]
}

/// Whether Dictionary keeps its button, then how many tools fit.
fn fitting(bar: &BarContext<'_>, reader: &ReaderBar, row_width: f32) -> (bool, usize) {
    let lead = if reader.sidebar_toggle { 2.0 } else { 1.0 } * TOOL_W;
    let captions = match captions(bar) {
        Some(CaptionStyle::Windows) => 3.0 * 46.0 + 4.0,
        Some(CaptionStyle::Gnome) => 3.0 * 24.0 + 2.0 * 8.0 + 4.0,
        None => 0.0,
    };
    // Appearance and the menu never fold.
    let fixed = 2.0 * TOOL_W + 8.0;
    let room = row_width - lead - captions - fixed - TITLE_ROOM;
    // Dictionary, beside Library, folds last.
    let dictionary = room >= TOOL_W;
    let room = if dictionary { room - TOOL_W } else { room };
    let fit = ((room / TOOL_W).floor().max(0.0) as usize).min(tools().len());
    (dictionary, fit)
}

/// The reader's Dictionary, as the menu lists it once folded.
fn dictionary_tool() -> Tool {
    Tool {
        icon: Icon::Dictionary,
        label: "Dictionary",
        command: Sent::Ui(UiCommand::ShowDictionary),
    }
}

fn captions(bar: &BarContext<'_>) -> Option<CaptionStyle> {
    bar.platform.captions().filter(|_| !bar.fullscreen)
}

#[derive(Clone, Debug, Default)]
pub struct TitleBar {
    /// Never shows, hover or pin (distraction-free reading).
    pub chromeless: bool,
    /// Kept shown while set, as the find bar does, once shown.
    pub hold: bool,
    /// A page's own control in the band, which never calls the bar.
    pub quiet: Option<Rect>,
    shown: bool,
    left_at: Option<f64>,
    opacity: f32,
}

impl TitleBar {
    /// Shows the bar as if hovered; it hides after the usual grace.
    pub fn reveal(&mut self) {
        self.shown = true;
    }

    /// How much of the bar showed last frame, 0 to 1; native buttons follow it.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn visible(&self, pinned: bool) -> bool {
        !self.chromeless && (pinned || self.shown)
    }

    /// Where a page under the bar starts: below it only while pinned.
    pub fn page_top(&self, ctx: &Context, page: mareader_settings::Bar, pinned: bool) -> f32 {
        let top = ctx.content_rect().top();
        let under = pinned && !self.chromeless;
        let id = Id::new(("titlebar-page-top", page));
        top + ctx.animate_bool_with_time(id, under, motion::secs(ctx, FADE)) * TITLE_BAR_H
    }

    /// Where the hover band ends; nothing interactive should sit above it.
    pub fn band_bottom(&self, ctx: &Context) -> f32 {
        let top = ctx.content_rect().top();
        if self.chromeless {
            top
        } else {
            top + TITLE_BAR_H
        }
    }

    fn update_hover(&mut self, ctx: &Context, band: Rect) {
        let now = ctx.input(|i| i.time);
        let own = egui::LayerId::new(Order::Foreground, Id::new("titlebar"));
        let at = crate::widgets::bare_pointer(ctx, own);
        let calm = at.is_some_and(|p| self.quiet.is_some_and(|q| q.contains(p)));
        let inside = !calm && at.is_some_and(|p| band.contains(p));
        let held = self.hold
            || egui::Popup::is_any_open(ctx)
            || (self.shown && ctx.input(|i| i.pointer.any_down()));
        if inside {
            self.shown = true;
            self.left_at = None;
        } else if calm && !held {
            // On the page's own control the bar steps aside at once.
            self.shown = false;
            self.left_at = None;
        } else if self.shown && !held {
            let waited = now - *self.left_at.get_or_insert(now);
            if waited >= HIDE_GRACE {
                self.shown = false;
                self.left_at = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(HIDE_GRACE - waited));
            }
        }
    }

    pub fn show(&mut self, ctx: &Context, bar: &BarContext<'_>, out: &mut Commands) {
        self.opacity = 0.0;
        if self.chromeless {
            return;
        }
        let screen = ctx.content_rect();
        let band = Rect::from_min_max(
            pos2(screen.left() + bar.left_inset, screen.top()),
            pos2(screen.right(), screen.top() + TITLE_BAR_H),
        );
        self.update_hover(ctx, band);
        let visible = self.visible(bar.pinned);
        let fade =
            ctx.animate_bool_with_time(Id::new("titlebar"), visible, motion::secs(ctx, FADE));
        self.opacity = fade;
        if fade <= 0.0 {
            return;
        }
        // Our own fade below; egui's would stack on it.
        egui::Area::new(Id::new("titlebar"))
            .fade_in(false)
            .order(Order::Foreground)
            .fixed_pos(band.min)
            .constrain(false)
            .interactable(visible)
            .show(ctx, |ui| {
                ui.set_opacity(fade);
                ui.set_min_size(band.size());
                let look = Look {
                    slot: 0,
                    reach: Reach::Below,
                    opacity: fade,
                };
                bar.backdrop.paint(ui, band, look);
                // Registered before the buttons so they win the pointer.
                let drag = ui.interact(band, Id::new("titlebar-drag"), Sense::CLICK | Sense::DRAG);
                if visible && drag.double_clicked() {
                    out.push(Sent::Ui(UiCommand::Window(WindowCommand::ToggleMaximize)));
                } else if visible && drag.drag_started() {
                    out.push(Sent::Ui(UiCommand::Window(WindowCommand::StartDrag)));
                }
                let live = visible;
                let mut row = band;
                // Over a rail or in overlay mode, the bar hosts no lights.
                row.min.x += if bar.gutter {
                    bar.platform.leading_inset()
                } else {
                    8.0
                };
                if captions(bar) != Some(CaptionStyle::Windows) {
                    row.max.x -= 8.0;
                }
                let builder = UiBuilder::new()
                    .max_rect(row)
                    .layout(Layout::left_to_right(Align::Center));
                ui.scope_builder(builder, |ui| self.contents(ui, bar, live, out));
            });
    }

    fn contents(
        &mut self,
        ui: &mut egui::Ui,
        bar: &BarContext<'_>,
        live: bool,
        out: &mut Commands,
    ) {
        // Window furniture animates before a click can move or close it.
        let _act = crate::icons::anim::announce(ui, crate::icons::anim::Act::Hover);
        let pinned = bar.pinned;
        let width = ui.max_rect().width();
        let ends = Id::new("titlebar-trail-start");
        let end = ui.data(|d| d.get_temp::<f32>(ends));
        let ((lead_end, lead), (trail_start, trail)) =
            egui::Sides::new().height(TITLE_BAR_H).spacing(8.0).show(
                ui,
                |ui| leading(ui, bar, end, width),
                |ui| trailing(ui, bar, pinned, width),
            );
        ui.data_mut(|d| d.insert_temp(ends, trail_start));
        let mut field = Commands::new();
        match &bar.page {
            BarPage::Library(library) => {
                search_field(ui, &library.search, (lead_end, trail_start), &mut field)
            }
            BarPage::Reader(_) | BarPage::Dictionary(_) => {
                title(ui, bar.title, lead_end, trail_start)
            }
        }
        for c in lead.into_iter().chain(trail).chain(field).filter(|_| live) {
            out.push(c);
        }
    }
}

fn clicked(r: egui::Response, c: Sent) -> Option<Sent> {
    r.clicked().then_some(c)
}

/// Its right edge and clicks; `end` is last frame's trailing edge.
fn leading(
    ui: &mut egui::Ui,
    bar: &BarContext<'_>,
    end: Option<f32>,
    width: f32,
) -> (f32, Commands) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut out = Commands::new();
    let keys = keymap::hint(bar.platform, &Sent::Ui(UiCommand::ShowDictionary));
    let dictionary = |ui: &mut egui::Ui, label: &str, on: bool| {
        let r = widgets::icon_button_keyed(ui, Icon::Dictionary, label, on, keys.as_deref());
        clicked(r, Sent::Ui(UiCommand::ShowDictionary))
    };
    match &bar.page {
        BarPage::Reader(reader) => {
            if reader.sidebar_toggle {
                let r = widgets::icon_button(ui, Icon::Sidebar, "Toggle sidebar", false);
                out.extend(clicked(r, Sent::Ui(UiCommand::ToggleSidebar)));
            }
            let r = widgets::icon_button(ui, Icon::Library, "Library", false);
            out.extend(clicked(r, Sent::Ui(UiCommand::ShowLibrary)));
            if fitting(bar, reader, width).0 {
                out.extend(dictionary(ui, "Dictionary", reader.dictionary));
            }
        }
        BarPage::Dictionary(page) => {
            let r = widgets::icon_button(ui, Icon::Library, "Library", false);
            out.extend(clicked(r, Sent::Ui(UiCommand::ShowLibrary)));
            // Pressed, it undoes itself, as the library's does.
            out.extend(dictionary(ui, page.back, true));
        }
        BarPage::Library(library) => {
            // Pressed, it undoes itself; with nothing open there's no way back.
            if library.can_return {
                let r = widgets::icon_button(ui, Icon::Library, "Back to reading", true);
                out.extend(clicked(r, Sent::Ui(UiCommand::ShowLibrary)));
            }
            out.extend(dictionary(ui, "Dictionary", false));
            // The crumbs leave the search field its narrowest width.
            let room = end.map(|end| end - ui.cursor().min.x - SEARCH_MIN - 24.0);
            crumbs::show(ui, &library.trail, library.renaming, room, &mut out);
        }
    }
    (ui.min_rect().right(), out)
}

/// Laid out right to left. Returns its left edge and what was clicked.
fn trailing(ui: &mut egui::Ui, bar: &BarContext<'_>, pinned: bool, width: f32) -> (f32, Commands) {
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut out = Commands::new();
    if let Some(style) = captions(bar) {
        let (w, gap) = match style {
            CaptionStyle::Windows => (3.0 * 46.0, 0.0),
            CaptionStyle::Gnome => (3.0 * 24.0 + 2.0 * 8.0, 8.0),
        };
        let size = vec2(w, ui.available_height());
        let caption =
            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                widgets::captions(ui, style, bar.maximized)
            });
        out.extend(caption.inner.map(|w| Sent::Ui(UiCommand::Window(w))));
        ui.add_space(4.0);
    }
    let tools = tools();
    // A docked sidebar's header already holds the tools.
    let tools: &[Tool] = match &bar.page {
        BarPage::Reader(reader) if !reader.rail_docked => &tools,
        _ => &[],
    };
    let (dictionary, fit) = match &bar.page {
        BarPage::Reader(reader) => fitting(bar, reader, width),
        BarPage::Library(_) | BarPage::Dictionary(_) => (true, 0),
    };
    let (shown, folded) = tools.split_at(fit.min(tools.len()));
    let mut folded = folded.to_vec();
    if !dictionary {
        folded.push(dictionary_tool());
    }
    // The end slot is the route's menu; Appearance stays put.
    match &bar.page {
        BarPage::Reader(reader) => {
            let menu = MenuContext {
                folded: &folded,
                fullscreen: bar.fullscreen,
                platform: bar.platform,
                zoom: reader.zoom,
                mode: reader.mode,
                auto_scroll: reader.auto_scroll,
                aloud: reader.aloud,
                pinned,
            };
            view_menu::button(ui, &menu, &mut out);
        }
        BarPage::Library(library) => {
            library_menu::button(ui, library, pinned, bar.platform, &mut out);
        }
        // The page's own choices sit on it; the menu keeps the pin.
        BarPage::Dictionary(_) => dictionary_menu(ui, pinned, bar.platform, &mut out),
    }
    appearance_button(ui, bar, &mut out);
    if !matches!(bar.page, BarPage::Reader(_)) {
        // The reader's sits in its menu; these pages have no other way.
        let keys = keymap::hint(bar.platform, &Sent::Ui(UiCommand::ShowSettings));
        let r = widgets::icon_button_keyed(ui, Icon::Settings, "Settings", false, keys.as_deref());
        out.extend(clicked(r, Sent::Ui(UiCommand::ShowSettings)));
        return (ui.min_rect().left(), out);
    }
    for tool in shown {
        let keys = keymap::hint(bar.platform, &tool.command);
        let r = widgets::icon_button_keyed(ui, tool.icon, tool.label, false, keys.as_deref());
        out.extend(clicked(r, tool.command.clone()));
    }
    (ui.min_rect().left(), out)
}

/// The dictionary page's menu: the pin and the shortcuts.
fn dictionary_menu(ui: &mut egui::Ui, pinned: bool, platform: Platform, out: &mut Commands) {
    let id = Id::new(DICTIONARY_MENU);
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::Menu, "Dictionary options", open);
    egui::Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .align(egui::RectAlign::BOTTOM_END)
        .gap(6.0)
        .width(MENU_W)
        .show(|ui| {
            ui.set_width(MENU_W);
            ui.spacing_mut().item_spacing.y = 2.0;
            view_menu::pin_row(ui, pinned, out);
            view_menu::shortcuts_row(ui, platform, out);
        });
}

/// Toggles the Appearance popover, which edits the look live.
fn appearance_button(ui: &mut egui::Ui, bar: &BarContext<'_>, out: &mut Commands) {
    let id = Id::new(APPEARANCE_POPOVER);
    let open = egui::Popup::is_id_open(ui.ctx(), id);
    let r = widgets::icon_button(ui, Icon::Appearance, "Appearance", open);
    // The flyout hugs the bar and window edge without casting a shadow.
    let screen = ui.ctx().content_rect();
    let hang = pos2(screen.right() - FLYOUT_MARGIN, screen.top() + TITLE_BAR_H);
    let mut frame = egui::Frame::popup(ui.style());
    frame.shadow = egui::Shadow::NONE;
    egui::Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .at_position(hang)
        .align(egui::RectAlign::BOTTOM_END)
        // An edge flyout never changes sides.
        .align_alternatives(&[])
        .gap(0.0)
        .frame(frame)
        .show(|ui| {
            let reader = match &bar.page {
                BarPage::Reader(reader) => Some(reader),
                BarPage::Library(_) | BarPage::Dictionary(_) => None,
            };
            let cx = crate::appearance_menu::MenuContext {
                current: bar.appearance,
                independence: reader.and_then(|r| r.independence),
                presets: bar.presets,
                user: bar.user_presets,
                gpu: bar.gpu,
                ink: reader.and_then(|r| r.ink),
                text: reader.and_then(|r| r.text),
                // The shelf has no page to texture.
                texture: reader.is_some(),
            };
            match bar.appearance_menu {
                AppearanceMenuStyle::Classic => crate::appearance_menu::show(ui, &cx, out),
                AppearanceMenuStyle::DialDeck => crate::dial_deck::show(ui, &cx, out),
            }
        });
    if !open && !egui::Popup::is_id_open(ui.ctx(), id) {
        crate::appearance_menu::forget_draft(ui.ctx());
        crate::dial_deck::forget_draft(ui.ctx());
    }
}

/// Centred on the window like a title, narrowed to the gap.
fn search_field(
    ui: &mut egui::Ui,
    field: &Field,
    (lead_end, trail_start): (f32, f32),
    out: &mut Commands,
) {
    const PAD: f32 = 12.0;
    let gap = trail_start - lead_end - 2.0 * PAD;
    if gap < SEARCH_MIN {
        return;
    }
    let w = gap.min(SEARCH_W);
    let centred = ui.ctx().content_rect().center().x - w / 2.0;
    let x = centred.clamp(lead_end + PAD, trail_start - PAD - w);
    let y = ui.max_rect().center().y;
    let rect = Rect::from_min_size(pos2(x, y - 15.0), vec2(w, 30.0));
    search::field(ui, rect, field, out);
}

/// Centred on the window when it fits, else in the gap, truncated.
fn title(ui: &egui::Ui, text: &str, lead_end: f32, trail_start: f32) {
    const PAD: f32 = 12.0;
    let gap = (trail_start - lead_end - 2.0 * PAD).max(0.0);
    if gap < 48.0 || text.is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let mut job = LayoutJob::simple_singleline(text.to_owned(), theme::medium(13.0), t.ink);
    job.wrap = TextWrapping::truncate_at_width(gap);
    let galley = ui.painter().layout_job(job);
    let rect = ui.max_rect();
    let w = galley.size().x;
    let centred = ui.ctx().content_rect().center().x - w / 2.0;
    let x = centred.clamp(lead_end + PAD, (trail_start - PAD - w).max(lead_end + PAD));
    let y = rect.center().y - galley.size().y / 2.0;
    let size = galley.size();
    ui.painter().galley(pos2(x, y), galley, t.ink);
    // Named in full for screen readers; hover only, so drags pass through.
    let at = Rect::from_min_size(pos2(x, y), size);
    let node = ui.interact(at, Id::new("titlebar-title"), Sense::hover());
    node.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
}
