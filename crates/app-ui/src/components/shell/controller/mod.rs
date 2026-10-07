//! The shell's layout state: rail rules, close machine, remembered panel.

use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::use_timeout::use_debounce_for;
use app_state::ChromeState;
use app_state::state::SidebarMode;
use reader_core::settings::Settings;

mod rules;

use rules::{panel_is_shown, sidebar_is_present, thumbnail_cells_are_live};

/// How the rail relates to the page it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarLayout {
    /// Docked: a flex sibling; the page gives up the width.
    Push,
    /// Floating: a fixed overlay from the window's left edge.
    Overlay,
}

/// The DOCKED close slide; the panel release below keys off it.
pub(crate) const SIDEBAR_SLIDE_MS: u64 = 300;

/// The FLOATING rail's fade.
pub(crate) const SIDEBAR_FADE_MS: u64 = 200;

/// The close hold: docked waits the slide, floating the fade.
fn outro_hold_ms(layout: SidebarLayout) -> u64 {
    match layout {
        SidebarLayout::Push => SIDEBAR_SLIDE_MS,
        SidebarLayout::Overlay => SIDEBAR_FADE_MS,
    }
}

/// The traffic-light gutter; the rail header mirrors it.
const TRAFFIC_LIGHTS_GUTTER_PX: f64 = 88.0;

/// The resting left padding once nothing reserves the corner.
const TITLEBAR_REST_PADDING_PX: f64 = 12.0;

/// Which route's chrome this is: the two pages' one difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChromeSurface {
    /// The reader route: a document is open and the shell has a rail.
    #[default]
    Reader,
    /// The library route: the shelf, with no rail and a navigation bar.
    Library,
}

impl ChromeSurface {
    /// Whether this surface's shell has a sidebar rail at all.
    fn has_rail(self) -> bool {
        matches!(self, ChromeSurface::Reader)
    }
}

/// Built once per page and provided as context.
#[derive(Clone, Copy)]
pub struct ShellController {
    /// Which sidebar panel is open; the signal itself lives in `ChromeState`.
    pub sidebar_mode: RwSignal<SidebarMode>,
    /// Pin state for THIS surface's bar, persisted per surface.
    pub titlebar_pinned: RwSignal<bool>,

    /// Settings write-back + persistence.
    settings: RwSignal<Settings>,
    /// Which route's chrome this controller drives.
    surface: ChromeSurface,
    /// Push or Overlay, from Settings → Layout.
    layout: Signal<SidebarLayout>,
    /// Whether the rail's slide tween is frozen (Settings → Animations,
    /// master already applied — `state.reader.sidebar_slide`).
    no_slide: Signal<bool>,

    /// The panel a reopen should restore (also the panel kept painted
    /// through a close slide).
    last_panel: RwSignal<SidebarMode>,
    /// A close slide is running: hold the last panel and the chrome.
    collapsing: RwSignal<bool>,
    /// Paint-only fade-in marker; see the module docs.
    intro: RwSignal<bool>,
    /// Whether thumbnail cells may be mounted right now.
    cells_mounted: RwSignal<bool>,
}

impl ShellController {
    /// The reader's shell: rail and titlebar, slide machine live.
    pub fn reader(state: ChromeState) -> Self {
        Self::build(state, ChromeSurface::Reader)
    }

    /// The library: a titlebar but no rail, every rail question "no".
    pub fn titlebar_only(state: ChromeState) -> Self {
        Self::build(state, ChromeSurface::Library)
    }

    /// Which surface this controller drives.
    pub fn surface(&self) -> ChromeSurface {
        self.surface
    }

    fn build(state: ChromeState, surface: ChromeSurface) -> Self {
        let settings = state.settings;
        let sidebar_mode = state.ui.sidebar;
        // Each surface remembers its own pin in its own settings field.
        let titlebar_pinned = RwSignal::new(match surface {
            ChromeSurface::Reader => settings.with(|s| s.titlebar_pinned),
            ChromeSurface::Library => settings.with(|s| s.library_titlebar_pinned),
        });
        let layout = Signal::derive(move || {
            if settings.with(|st| st.layout.sidebar_overlay) {
                SidebarLayout::Overlay
            } else {
                SidebarLayout::Push
            }
        });
        let no_slide = Signal::derive(move || !state.reader.sidebar_slide.get().sidebar_slide);

        // The close machine; see the module docs for what it holds.
        let last_panel = RwSignal::new(SidebarMode::Thumbs);
        let collapsing = RwSignal::new(false);
        let intro = RwSignal::new(false);
        let cells_mounted = RwSignal::new(false);
        // Whether the previous mode was closed.
        let was_closed = StoredValue::new_local(true);
        // The end of the outro: hold one slide, then release, via a debounce
        // `on_cleanup` clears.
        let outro = use_debounce_for(
            move || Duration::from_millis(outro_hold_ms(layout.get_untracked())),
            move || {
                collapsing.set(false);
                // The engine cache remains; only live canvases are released.
                cells_mounted.set(false);
            },
        );

        Effect::new(move |_| {
            let now = sidebar_mode.get();
            let was = was_closed.get_value();

            // These signals live in the controller's scope; writes are try_.
            if now != SidebarMode::None {
                let _ = last_panel.try_set(now);
                let _ = collapsing.try_set(false);
                outro.cancel();
                if was {
                    // Let cached thumbnails ride the motion. Cold cells keep
                    // their own skeleton until renderThumb completes.
                    let _ = cells_mounted.try_set(true);
                    // The docked open fades panels in; the overlay skips it.
                    if !matches!(layout.get_untracked(), SidebarLayout::Overlay) {
                        let _ = intro.try_set(true);
                    }
                    // Hold the marker a frame, then drop it: two rAFs, not one.
                    request_animation_frame(move || {
                        request_animation_frame(move || {
                            // The rail can be torn down between arm and frame.
                            let _ = intro.try_set(false);
                        });
                    });
                }
                let _ = was_closed.try_set_value(false);
            } else {
                let _ = was_closed.try_set_value(true);
                let _ = intro.try_set(false);
                // The initial closed state has no outro.
                if was || no_slide.get_untracked() {
                    let _ = collapsing.try_set(false);
                    let _ = cells_mounted.try_set(false);
                } else {
                    let _ = collapsing.try_set(true);
                    outro.trigger();
                }
            }
        });

        Self {
            sidebar_mode,
            titlebar_pinned,
            settings,
            surface,
            layout,
            no_slide,
            last_panel,
            collapsing,
            intro,
            cells_mounted,
        }
    }

    // Every layout rule lives in one of these methods.

    /// A panel is open (Outline or Thumbs).
    pub fn is_sidebar_open(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.sidebar_mode.get() != SidebarMode::None)
    }

    /// The rail floats over the page; the library answers no.
    pub fn is_overlay(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            this.surface.has_rail() && matches!(this.layout.get(), SidebarLayout::Overlay)
        })
    }

    /// The rail is on screen: open, or its close motion still running.
    pub fn rail_present(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            this.surface.has_rail()
                && sidebar_is_present(this.sidebar_mode.get(), this.collapsing.get())
        })
    }

    /// May the titlebar's sidebar toggle show? Overlay mode drops it.
    pub fn show_sidebar_toggle(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            !this.is_overlay().get() && this.sidebar_mode.get() == SidebarMode::None
        })
    }

    /// May the edge-hover strip show? Only while the rail is closed.
    pub fn hover_strip_active(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            this.is_overlay().get() && this.sidebar_mode.get() == SidebarMode::None
        })
    }

    /// Does the bar's hover band yield its left edge? Docked rails only.
    pub fn band_inset(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.rail_present().get() && !this.is_overlay().get())
    }

    /// Does the row reserve the light gutter? Not under a docked rail.
    fn lights_gutter(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            app_chrome::platform::is_macos()
                && !this.is_overlay().get()
                && !this.rail_present().get()
        })
    }

    /// Could the bar host the lights at all here? Overlay answers no.
    pub fn bar_gutter(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || app_chrome::platform::is_macos() && !this.is_overlay().get())
    }

    /// The bar row's left padding: the light gutter, or the resting one.
    pub fn titlebar_left_gutter(&self) -> Signal<f64> {
        let this = *self;
        Signal::derive(move || {
            if this.lights_gutter().get() {
                TRAFFIC_LIGHTS_GUTTER_PX
            } else {
                TITLEBAR_REST_PADDING_PX
            }
        })
    }

    /// The rail's motion is frozen (Settings → Animations): end frames only.
    pub fn no_slide(&self) -> Signal<bool> {
        self.no_slide
    }

    /// Whether `panel` should stay painted this frame.
    pub fn panel_shown(&self, panel: SidebarMode) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            panel_is_shown(
                panel,
                this.sidebar_mode.get(),
                this.collapsing.get(),
                this.last_panel.get(),
            )
        })
    }

    /// Whether `panel` is the active one (the switcher's pressed state).
    pub fn panel_active(&self, panel: SidebarMode) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.sidebar_mode.get() == panel)
    }

    /// The raw mode is closed: the panels' outro flag.
    pub fn panel_outro(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.sidebar_mode.get() == SidebarMode::None)
    }

    /// Paint-only fade-in marker for the panel hosts; see the module docs.
    pub fn panel_intro(&self) -> Signal<bool> {
        self.intro.into()
    }

    /// Final mount gate: mounted by a real open, held through the outro.
    pub fn thumbs_live(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            thumbnail_cells_are_live(
                this.cells_mounted.get(),
                this.sidebar_mode.get(),
                this.collapsing.get(),
                this.last_panel.get(),
            )
        })
    }

    /// Toggle from the titlebar: open the default panel, else close.
    pub fn toggle_sidebar(&self) {
        if self.sidebar_mode.get() == SidebarMode::None {
            self.sidebar_mode.set(SidebarMode::Thumbs);
        } else {
            self.sidebar_mode.set(SidebarMode::None);
        }
    }

    /// Reopen the panel a close last left behind (the overlay rail's
    /// edge-hover hand-off).
    pub fn open_last_panel(&self) {
        self.sidebar_mode.set(self.last_panel.get());
    }

    /// Close the rail (the mode flips now; chrome follows `rail_present`
    /// through the close motion).
    pub fn close_sidebar(&self) {
        self.sidebar_mode.set(SidebarMode::None);
    }

    /// Pin this surface's bar, through the debounced settings effect.
    pub fn set_titlebar_pinned(&self, pinned: bool) {
        self.titlebar_pinned.set(pinned);
        self.settings.update(|s| match self.surface {
            ChromeSurface::Reader => s.titlebar_pinned = pinned,
            ChromeSurface::Library => s.library_titlebar_pinned = pinned,
        });
    }
}
