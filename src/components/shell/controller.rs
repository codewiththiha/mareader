//! The shell's single source of truth for layout state.
//!
//! Before this module existed, the answers to "is the rail overlay or
//! docked?", "does the bar still owe the traffic lights a gutter?", "may the
//! sidebar toggle show?" and "is the rail painted right now?" were recomputed
//! wherever they happened to be needed — `features/reader/page.rs` derived
//! `overlay_sb` and re-spelled `!overlay && mode == None` per mount point,
//! `app_title_bar.rs` rebuilt `rail_painted` / `band_inset` /
//! `lights_gutter` / `bar_gutter` from a chrome context, and the floating
//! label carried its own fallback for the same question. Changing one rule
//! meant finding every spelling of it.
//!
//! Now the page builds ONE [`ShellController`] and provides it as context.
//! Components do not recompute layout facts; they ask the controller:
//!
//! ```text
//! let shell = use_context::<ShellController>().expect(…);
//! shell.is_overlay().get()      // rail floats over the page?
//! shell.rail_present().get()    // rail on screen, close motion included?
//! shell.titlebar_left_gutter()  // px the bar's row must inset for the lights
//! ```
//!
//! The controller also OWNS the open/close bookkeeping (the machine the
//! deleted `SidebarPaint` state machine used to carry) and the remembered
//! last panel, so "reopen what was open" is one call (`open_last_panel`)
//! instead of a second `last_mode` tracker in the page.
//!
//! THE CLOSE MACHINE, TWO GEOMETRIES. The two layouts do not share a motion:
//! the DOCKED rail slides (the aside tweens its width over
//! [`SIDEBAR_SLIDE_MS`]), while the FLOATING rail fades in and out over
//! [`SIDEBAR_FADE_MS`] — a transform slide off the window's edge would
//! travel under the native traffic lights, which can only appear and
//! disappear, so the overlay keeps to what the lights can do and the two
//! read as one unit. Chrome must stay aligned with the pixels for the whole
//! length of either direction: the raw mode flips to `None` on the close
//! click, before the rail is out of the way. `rail_present` is therefore
//! "open OR the close animation is still running" — whatever yields to the
//! rail (the bar's band inset, the traffic lights' host, the floating
//! label's corner) derives from it so it releases when the motion lands,
//! not on frame one of the close.
//!
//! OPEN mounts thumbnail cells immediately so warm bitmaps can paint while
//! the rail is moving. `panel_intro` is the DOCKED open's paint-only marker —
//! a two-frame flag that starts the panel opacity transition without
//! delaying the cell DOM — and is deliberately skipped in overlay: the
//! wrapper's own fade is the reveal there, and a second fade inside it
//! would double-dim the panels. CLOSE is the only timer-gated direction:
//! it keeps the last panel painted through the motion (`collapsing`) and
//! releases the live thumbnail canvases at the same instant the motion
//! lands — one timer that waits out whichever duration the layout's rail
//! is actually running. A reopen inside that window never unmounts, never
//! re-renders, never reallocates.
//!
//! Settings → Animations can freeze the rail's motion (`no_slide`): the
//! docked rail then jumps to its end width, the floating rail appears at
//! full opacity, and the close hold is released on the spot, because
//! nothing is left waiting on a motion that never runs.
//!
//! TWO PAGES, ONE RULEBOOK. The reader builds the controller with
//! [`ShellController::reader`] (rail + titlebar); the library builds it with
//! [`ShellController::titlebar_only`], which answers every rail question
//! with "no rail": the bar keeps the full window width, its 88px gutter and
//! its lights. That keeps the no-rail rules in this file too, instead of an
//! `Option`-shaped fork in every consumer.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::primitives::hooks::use_timeout::use_debounce_for;
use crate::state::{AppState, SidebarMode};
use crate::storage::save_settings;
use pdf_core::settings::Settings;

/// How the rail relates to the page it serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarLayout {
    /// Docked: the rail is a flex sibling of the page, which gives up the
    /// width (the aside tweens `w-72` ↔ `w-0`).
    Push,
    /// Floating: the rail overlays the page from the window's left edge
    /// (a fixed wrapper that fades in and out).
    Overlay,
}

/// How long the DOCKED rail takes to close. The panel paint and the deferred
/// canvas release key off this so they land with the end of the width slide
/// rather than trailing it; the aside's own CSS transition is declared with
/// the matching `duration-300` — keep them in step.
pub(crate) const SIDEBAR_SLIDE_MS: u64 = 300;

/// How long the FLOATING rail's fade takes. The overlay wrapper carries the
/// matching `duration-200`, and the close hold uses this so the rail, its
/// shadow and the native traffic lights all land on the same frame — 200ms
/// is the system-standard window for a fade, which is why it is not simply
/// the slide's duration under a different name.
pub(crate) const SIDEBAR_FADE_MS: u64 = 200;

/// The close hold for a layout's rail: docked waits out the width slide,
/// floating waits out the fade.
fn outro_hold_ms(layout: SidebarLayout) -> u64 {
    match layout {
        SidebarLayout::Push => SIDEBAR_SLIDE_MS,
        SidebarLayout::Overlay => SIDEBAR_FADE_MS,
    }
}

/// The gutter the native traffic lights live in when the bar hosts them:
/// 88px clears the lights (x:20 + ~54px) plus a real gap. Mirrored by the
/// rail header's own `pl-[88px]` chrome row.
const TRAFFIC_LIGHTS_GUTTER_PX: f64 = 88.0;

/// The row's resting left padding once nothing reserves the lights' corner
/// (`pl-3` in the classes this replaced).
const TITLEBAR_REST_PADDING_PX: f64 = 12.0;

/// The single source of truth for shell layout state. Built once per page
/// and provided as context; see the module docs for the question API.
#[derive(Clone, Copy)]
pub struct ShellController {
    /// Which sidebar panel is open. The signal itself belongs to
    /// `AppState::ui` — the controller centralizes the QUESTIONS about it,
    /// not the storage.
    pub sidebar_mode: RwSignal<SidebarMode>,
    /// Pin state for the title bar (persisted through the controller so
    /// both routes share one wiring).
    pub titlebar_pinned: RwSignal<bool>,

    /// Settings write-back + persistence.
    settings: RwSignal<Settings>,
    /// Whether this page mounts a rail at all (reader: yes, library: no).
    has_rail: bool,
    /// Push or Overlay, from Settings → Layout.
    layout: Signal<SidebarLayout>,
    /// Whether the rail's slide tween is frozen (Settings → Animations,
    /// master already applied — `state.reader.viewer.motion`).
    no_slide: Signal<bool>,

    // ---- open/close slide machine ------------------------------------
    /// The panel a reopen should restore (also the panel kept painted
    /// through a close slide).
    last_panel: RwSignal<SidebarMode>,
    /// A close slide is running: keep the last panel painted and the chrome
    /// yielded until it lands.
    collapsing: RwSignal<bool>,
    /// Paint-only fade-in marker; see the module docs.
    intro: RwSignal<bool>,
    /// Whether thumbnail cells may be mounted right now.
    cells_mounted: RwSignal<bool>,
}

impl ShellController {
    /// The reader's shell: rail + titlebar, with the slide machine live.
    /// Must run inside the page's reactive owner (the machine installs an
    /// effect and a debouncer).
    pub fn reader(state: AppState) -> Self {
        Self::build(state, true)
    }

    /// A page with a titlebar but no rail (the library): every rail
    /// question answers "no", so the bar keeps its full width, its gutter
    /// and its lights.
    pub fn titlebar_only(state: AppState) -> Self {
        Self::build(state, false)
    }

    fn build(state: AppState, has_rail: bool) -> Self {
        let settings = state.settings;
        let sidebar_mode = state.ui.sidebar;
        let titlebar_pinned = RwSignal::new(settings.with(|s| s.titlebar_pinned));
        let layout = Signal::derive(move || {
            if settings.with(|st| st.layout.sidebar_overlay) {
                SidebarLayout::Overlay
            } else {
                SidebarLayout::Push
            }
        });
        let no_slide =
            Signal::derive(move || !state.reader.viewer.motion.get().sidebar_slide);

        // The close machine, verbatim from the old `sidebar_paint` apart from
        // the hold's duration: see the module docs for what each direction
        // holds and releases.
        let last_panel = RwSignal::new(SidebarMode::Thumbs);
        let collapsing = RwSignal::new(false);
        let intro = RwSignal::new(false);
        let cells_mounted = RwSignal::new(false);
        // Whether the previous mode was closed. Tab changes do not re-run
        // the open path, while a real None → panel transition does.
        let was_closed = StoredValue::new_local(true);
        // The end of the outro: hold the panel and its canvases for one
        // slide, then release. A debounce rather than a hand-rolled handle,
        // because `on_cleanup` then clears a fire that is still pending —
        // which a stored handle only did if the NEXT close arrived first,
        // leaving a reader that was gone writing to signals that were.
        // Re-arming postpones the release instead of queueing a second one,
        // which is what a burst of toggles should do. The WAIT is read per
        // trigger, untracked, so one timer serves both geometries: a close in
        // the docked layout holds for the width slide, a close in the overlay
        // layout holds for the fade — and the lights under the floating rail
        // release on the frame it finishes disappearing.
        let outro = use_debounce_for(
            move || Duration::from_millis(outro_hold_ms(layout.get_untracked())),
            move || {
                collapsing.set(false);
                // The engine cache remains; only live DOM canvases are released,
                // so a later open can synchronously blit.
                cells_mounted.set(false);
            },
        );

        Effect::new(move |_| {
            let now = sidebar_mode.get();
            let was = was_closed.get_value();

            if now != SidebarMode::None {
                last_panel.set(now);
                collapsing.set(false);
                outro.cancel();
                if was {
                    // Let cached thumbnails ride the motion. Cold cells keep
                    // their own skeleton until renderThumb completes.
                    cells_mounted.set(true);
                    // The docked open fades the panels in alongside the width
                    // slide. The overlay open skips the marker: its wrapper
                    // fades the whole rail in, and a panel fade inside that
                    // fade would land at half the opacity of either.
                    if !matches!(layout.get_untracked(), SidebarLayout::Overlay) {
                        intro.set(true);
                    }
                    // Keep the marker through one COMMITTED frame, then
                    // remove it so the CSS opacity transition runs alongside
                    // the rail. Two rAFs, not one: the first callback fires
                    // BEFORE the frame with `intro` painted has been
                    // composited, so clearing there would change the class
                    // in the same paint the marker appeared in — no
                    // transition. The second callback runs strictly after
                    // that frame is on screen, which is the earliest point
                    // the fade can actually animate from.
                    request_animation_frame(move || {
                        request_animation_frame(move || intro.set(false));
                    });
                }
                was_closed.set_value(false);
            } else {
                was_closed.set_value(true);
                intro.set(false);
                // The initial closed state has no outro. Every panel → None
                // transition holds cells and chrome for the actual motion —
                // and with the tween frozen there IS no motion to wait out,
                // so holding them would leave the title bar's inset
                // released a timer late by a rail that is already gone.
                if was || no_slide.get_untracked() {
                    collapsing.set(false);
                    cells_mounted.set(false);
                } else {
                    collapsing.set(true);
                    outro.trigger();
                }
            }
        });

        Self {
            sidebar_mode,
            titlebar_pinned,
            settings,
            has_rail,
            layout,
            no_slide,
            last_panel,
            collapsing,
            intro,
            cells_mounted,
        }
    }

    // ---- the question API ---------------------------------------------
    // Every rule about how the shell lays out lives in one of these methods
    // — a consumer that recomputes one of them by hand is a bug.

    /// A panel is open (Outline or Thumbs).
    pub fn is_sidebar_open(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.sidebar_mode.get() != SidebarMode::None)
    }

    /// The rail floats over the page instead of docking into it. Only a
    /// page with a rail can be in overlay mode; the library's answer is
    /// always no.
    pub fn is_overlay(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.has_rail && matches!(this.layout.get(), SidebarLayout::Overlay))
    }

    /// The rail is on screen: open, or its close motion is still running —
    /// in Push OR Overlay mode. The floating label and the native traffic
    /// lights key off this directly: a rail of either kind covers the
    /// window's top-left corner, so the label gets out of the way and the
    /// lights are hosted by the rail's own header gutter.
    pub fn rail_present(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            this.has_rail && sidebar_is_present(this.sidebar_mode.get(), this.collapsing.get())
        })
    }

    /// May the titlebar's sidebar toggle show? Overlay mode drops it: the
    /// rail opens by brushing the window's left edge and closes from its
    /// own header, so a second switch in the bar only competes with both.
    pub fn show_sidebar_toggle(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            !this.is_overlay().get() && this.sidebar_mode.get() == SidebarMode::None
        })
    }

    /// May the overlay rail's edge-hover strip show? Only while the overlay
    /// rail is fully closed — an open rail covers the strip's pixels.
    pub fn hover_strip_active(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || {
            this.is_overlay().get() && this.sidebar_mode.get() == SidebarMode::None
        })
    }

    /// Does the bar's hover band yield its left edge? Only a DOCKED rail
    /// takes the band's edge: an overlay rail floats ABOVE the bar and
    /// covers its corner, so the band keeps the full window width and
    /// reads as one bar either way.
    pub fn band_inset(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.rail_present().get() && !this.is_overlay().get())
    }

    /// Does the bar's row reserve the 88px traffic-light gutter? Off when a
    /// docked rail has taken that corner over, and off in overlay mode —
    /// where there are no lights in the bar to clear, so the leading
    /// control moves left into the space they would have occupied.
    pub fn lights_gutter(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || !this.is_overlay().get() && !this.rail_present().get())
    }

    /// Could the bar host the lights AT ALL in this layout mode, regardless
    /// of what is covering it? Not the same question as `lights_gutter`:
    /// overlay mode answers no — the bar keeps its full width and its
    /// leading control sits in the space the lights would have taken, so a
    /// hover must not put them back on top of it. The rail still hosts
    /// them from its own header while it is up, which is `rail_present`'s
    /// job and not this signal's.
    pub fn bar_gutter(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || !this.is_overlay().get())
    }

    /// The bar row's left padding in px: the traffic-light gutter while the
    /// bar owes the lights one, the resting padding once the corner belongs
    /// to something else.
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

    /// The rail's motion is frozen (Settings → Animations): the docked
    /// width slide and the floating fade both collapse to their end frames.
    /// Read TRACKED by the rail wrappers (the class has to move in the frame
    /// the switch does) and untracked by the machine.
    pub fn no_slide(&self) -> Signal<bool> {
        self.no_slide
    }

    // ---- panel paint (consumed by the rail's panel hosts) --------------

    /// Whether `panel` should stay painted this frame. Open: only the
    /// active panel. Closing: the panel that was showing, for the whole
    /// slide, so it can fade and clip with the rail labels instead of
    /// popping off on frame one.
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

    /// The raw mode is closed — the panels' outro flag, so their fade lands
    /// with the rail's clip rather than after it.
    pub fn panel_outro(&self) -> Signal<bool> {
        let this = *self;
        Signal::derive(move || this.sidebar_mode.get() == SidebarMode::None)
    }

    /// Paint-only fade-in marker for the panel hosts; see the module docs.
    pub fn panel_intro(&self) -> Signal<bool> {
        self.intro.into()
    }

    /// Final mount gate for thumbnail cells: mounted by a real open, and
    /// held through the outro so a quick reopen is free.
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

    // ---- actions --------------------------------------------------------

    /// Toggle from the titlebar's switch: open the default panel (Thumbs)
    /// when closed, close whatever is open.
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

    /// Pin the title bar and persist the choice (both routes share this).
    pub fn set_titlebar_pinned(&self, pinned: bool) {
        self.titlebar_pinned.set(pinned);
        self.settings.update(|s| s.titlebar_pinned = pinned);
        if let Err(e) = save_settings(&self.settings.with(|s| s.clone())) {
            e.report();
        }
    }
}

/// Whether the rail is still painted and therefore still owns title-bar
/// chrome space. Stays true through the close slide after the raw mode has
/// changed to `None`.
fn sidebar_is_present(mode: SidebarMode, collapsing: bool) -> bool {
    mode != SidebarMode::None || collapsing
}

/// Whether `panel` should stay painted this frame (see
/// [`ShellController::panel_shown`]).
fn panel_is_shown(panel: SidebarMode, mode: SidebarMode, collapsing: bool, last: SidebarMode) -> bool {
    mode == panel || (mode == SidebarMode::None && collapsing && last == panel)
}

/// Final mount gate for thumbnail cells. Even if the panel state would
/// normally preserve cells through an outro, there is nothing to preserve
/// when a close arrives before the opening delay created any cells.
fn thumbnail_cells_are_live(
    cells_mounted: bool,
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    cells_mounted && thumbs_should_stay_mounted(mode, collapsing, last)
}

/// Whether the thumbnail grid should keep its cells mounted.
///
/// Mounted while Thumbs is showing, while Outline is showing (so a tab
/// switch does not re-render every thumb), and while the Thumbs panel is
/// mid-outro. Dropped only once a close from Thumbs has finished — that
/// is what releases the live canvases without a quick-reopen spike.
fn thumbs_should_stay_mounted(mode: SidebarMode, collapsing: bool, last: SidebarMode) -> bool {
    match mode {
        SidebarMode::Thumbs | SidebarMode::Outline => true,
        SidebarMode::None => collapsing && last == SidebarMode::Thumbs,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        panel_is_shown, sidebar_is_present, thumbnail_cells_are_live, thumbs_should_stay_mounted,
        SIDEBAR_FADE_MS, SIDEBAR_SLIDE_MS,
    };
    use crate::state::SidebarMode;

    #[test]
    fn each_motion_matches_its_css_duration() {
        // The rail wrappers carry the matching transition utilities (the
        // docked aside's `duration-300` width tween, the floating wrapper's
        // `duration-200` opacity fade), and the panel paint plus the deferred
        // canvas release both key off these constants, so the outros land
        // with the end of the motion rather than trailing it. Rename either
        // side and this test is the tripwire.
        assert_eq!(SIDEBAR_SLIDE_MS, 300);
        assert_eq!(SIDEBAR_FADE_MS, 200);
    }

    #[test]
    fn a_close_keeps_the_open_panel_painted_until_the_motion_ends() {
        // Frame one of a Thumbs close: still painted, so it can fade/clip.
        assert!(panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        assert!(!panel_is_shown(
            SidebarMode::Outline,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        // After the slide: both hidden.
        assert!(!panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::None,
            false,
            SidebarMode::Thumbs
        ));
    }

    #[test]
    fn chrome_space_is_held_until_the_close_motion_lands() {
        assert!(sidebar_is_present(SidebarMode::Thumbs, false));
        // Frame one of a close: raw mode is None, but the rail is still
        // sliding or fading and title-bar chrome must remain aligned with it.
        assert!(sidebar_is_present(SidebarMode::None, true));
        assert!(!sidebar_is_present(SidebarMode::None, false));
    }

    #[test]
    fn a_tab_switch_shows_only_the_active_panel() {
        assert!(panel_is_shown(
            SidebarMode::Outline,
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
        assert!(!panel_is_shown(
            SidebarMode::Thumbs,
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
    }

    #[test]
    fn thumbnail_cells_require_a_real_mount() {
        // The helper remains defensive: an outro state alone never creates
        // cells; the open transition is what mounts them.
        assert!(!thumbnail_cells_are_live(
            false,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs,
        ));
        // Once cells genuinely exist, the ordinary open and outro paths keep
        // working as before.
        assert!(thumbnail_cells_are_live(
            true,
            SidebarMode::Thumbs,
            false,
            SidebarMode::Thumbs,
        ));
        assert!(thumbnail_cells_are_live(
            true,
            SidebarMode::None,
            true,
            SidebarMode::Thumbs,
        ));
    }

    #[test]
    fn thumbs_stay_mounted_across_a_tab_switch_but_not_a_finished_close() {
        // Instant Thumbs ↔ Outline: keep the canvases.
        assert!(thumbs_should_stay_mounted(
            SidebarMode::Outline,
            false,
            SidebarMode::Outline
        ));
        assert!(thumbs_should_stay_mounted(
            SidebarMode::Thumbs,
            false,
            SidebarMode::Thumbs
        ));
        // Mid-outro from Thumbs: keep them so a quick reopen is free.
        assert!(thumbs_should_stay_mounted(
            SidebarMode::None,
            true,
            SidebarMode::Thumbs
        ));
        // Slide finished, or we closed from Outline: drop the live canvases.
        assert!(!thumbs_should_stay_mounted(
            SidebarMode::None,
            false,
            SidebarMode::Thumbs
        ));
        assert!(!thumbs_should_stay_mounted(
            SidebarMode::None,
            true,
            SidebarMode::Outline
        ));
    }
}
