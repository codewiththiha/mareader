//! Left sidebar with panels (outline / thumbnails). The coordinator owns the
//! open/close state machine and composes the chrome row, the book-identity
//! row, the two stacked panels, and the bottom rail (see the sibling modules).
//!
//! The `<aside>` is ALWAYS mounted and animates its `width` between 18rem and 0
//! (single-phase slide, no two-phase unmount). The inner content stays fixed at
//! `w-72` so it doesn't collapse mid-transition — `overflow-hidden` on the aside
//! clips it while closed. When collapsed the content is made `inert` so the
//! clipped rail can't be tab-focused / activated.
//!
//! CLOSE / OPEN. The rail labels (Thumbs / Outline) outro by
//! being clipped as the aside shrinks — a 300ms ease-in-out the reader
//! already has. The panels used to go `visibility:hidden` on the FIRST frame
//! of that slide, so the thumbnail grid popped off while the labels slid
//! away. Worse, toggling visibility on a stack of `filter` + `mix-blend-mode`
//! canvases makes WKWebView allocate a fresh compositor layer per thumb;
//! close-and-reopen-quickly stacked those layers into a RAM spike.
//!
//! So the last-open panel STAYS PAINTED for the whole slide (it fades with
//! `.sidebar-panel.is-outro` and clips with the aside). Only after the 300ms
//! does the grid unmount, which `cancelThumb`s every live canvas and drops
//! the backing stores. A reopen inside that window never unmounts, never
//! re-renders, never reallocates.
//!
//! OPEN is the expensive direction: the width transition reflows the
//! `flex-1` viewer every frame for 300ms while the thumbnail grid mounts
//! its whole first window (≈20–30 pdf.js rasterisations) and the fit
//! effect rescales every page. `SidebarPaint::opening` marks that first
//! 300ms window so the page can defer the cell mounts until the layout
//! has settled — the skeleton shows for the slide, then the grid mounts.
//! On later opens the engine's thumb cache is warm (`starts_cached`), so
//! the gate costs nothing while still keeping the slide jank-free.
//! A close that beats that opening gate must not arm the close hold: no cells
//! existed to preserve. `cells_mounted` guards that path, so toggle spam never
//! mounts canvases during a close that interrupted the opening delay.
//!
//! Tab switches (Thumbs ↔ Outline) still use `invisible` on the inactive
//! panel so the virtualization window stays engine-bound and a switch back
//! is instant. `hidden` (`display:none`) is still forbidden: its height
//! collapse would re-evict the window and re-render every thumb.
//!
//! Gotcha: each reactive `class=("name", cond)` toggle becomes one
//! `classList.add("name")` call — a space-separated token throws a swallowed
//! SyntaxError and the class is silently never applied. Keep every conditional
//! class to a SINGLE token (hence `w-0` and `border-r-0` as separate toggles).


use std::time::Duration;

use leptos::prelude::*;

use leptos::children::ViewFn;

use crate::state::SidebarMode;

/// Must match the aside's `duration-300` width slide. The panel fade and
/// the deferred canvas release both key off this so the three outros land
/// together and a quick reopen cannot beat the unmount.
pub(crate) const SIDEBAR_SLIDE_MS: u64 = 300;

/// Ask the visible panel to scroll to wherever the reader currently is.
///
/// A CustomEvent rather than a signal in `AppState`: this is a one-shot
/// gesture with no state to hold, and a counter signal would need to be read
/// by both panels and could not distinguish "asked twice" from "asked once"
/// without extra bookkeeping. Same mechanism the PDF link layer uses.
pub(crate) fn request_reveal_active() {
    let Some(win) = web_sys::window() else { return };
    let Ok(event) = web_sys::CustomEvent::new("pdfreader:reveal-active") else {
        return;
    };
    _ = win.dispatch_event(&event);
}

/// Whether `panel` should stay painted this frame.
///
/// Open: only the active panel. Closing: the panel that was showing, for
/// the whole slide, so it can fade and clip with the rail labels instead
/// of popping off on frame one.
pub(crate) fn panel_is_shown(
    panel: SidebarMode,
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    mode == panel || (mode == SidebarMode::None && collapsing && last == panel)
}

/// Whether the rail is still painted and therefore still owns title-bar
/// chrome space. This stays true through the close slide after raw `mode`
/// has changed to `None`.
pub(crate) fn sidebar_is_present(mode: SidebarMode, collapsing: bool) -> bool {
    mode != SidebarMode::None || collapsing
}

/// Chrome-facing view of the open/close slide. The page that runs
/// [`sidebar_paint`] provides this; the title bar above the sidebar reads it
/// so its chrome (left inset, native traffic lights) follows the paint, not
/// the raw mode.
#[derive(Clone, Copy)]
pub struct SidebarChromeCtx {
    /// Sidebar open, or its close slide still running.
    pub present: Signal<bool>,
}

/// Paint flags derived from the open/close slide. The page composes
/// the panel hosts with these; the shell itself only owns the aside.
pub struct SidebarPaint {
    pub show_outline: Signal<bool>,
    pub show_thumbs: Signal<bool>,
    pub thumbs_live: Signal<bool>,
    pub is_closed: Signal<bool>,
    pub outline_active: Signal<bool>,
    pub thumbs_active: Signal<bool>,
    /// True for the first SIDEBAR_SLIDE_MS after the sidebar opens.
    /// Gate expensive mounts (thumbnail cells) on this so they don't
    /// compete with the width animation.
    pub opening: Signal<bool>,
    /// Opacity-intro class for the panel hosts. This mirrors `opening`: the
    /// wrapper is transparent during the gate, then fades in when cells mount.
    pub intro: Signal<bool>,
    /// The sidebar still occupies chrome space: open OR mid-close-slide.
    /// Title-bar inset and native traffic lights derive from this so they
    /// release when the slide lands, not on the first frame of the close.
    pub present: Signal<bool>,
}

/// Drive the open/close slide bookkeeping and return the paint flags.
///
/// `collapsing` gates the CLOSE slide (keep the last panel painted while
/// the aside shrinks). `opening` is its mirror for the OPEN slide: it marks
/// the 300 ms window in which the width transition is reflowing the viewer
/// every frame, so the page can defer the thumbnail `<For>` window — 20–30
/// `render_thumb` rasterisations — until the layout has settled.
pub fn sidebar_paint(mode: RwSignal<SidebarMode>) -> SidebarPaint {
    let last_mode = RwSignal::new(SidebarMode::Thumbs);
    let collapsing = RwSignal::new(false);
    let opening = RwSignal::new(false);
    // True only while thumbnail cells actually exist. Besides preventing a
    // pre-gate close from mounting them during its outro, this preserves the
    // existing quick-reopen path: cells remain live until the close timer
    // finishes and a reopen inside that window cancels the release.
    let cells_mounted = RwSignal::new(false);
    let collapse_timer = StoredValue::new_local(None::<TimeoutHandle>);
    let opening_timer = StoredValue::new_local(None::<TimeoutHandle>);
    // Whether the sidebar was closed the last time `mode` changed. An open
    // transition (None → panel) only counts as "opening" when it actually
    // came from closed; a Thumbs ↔ Outline tab switch must not re-arm the
    // gate or every tab switch would flash the grid off for 300 ms.
    let was_closed = StoredValue::new_local(true);

    Effect::new(move |_| {
        let now = mode.get();
        let was = was_closed.get_value();

        if now != SidebarMode::None {
            last_mode.set(now);
            collapsing.set(false);
            if let Some(h) = collapse_timer.get_value() {
                h.clear();
                collapse_timer.set_value(None);
            }

            // Mark the opening window so the page can hold back the
            // thumbnail cells while the width animation is running. A quick
            // reopen during a real close still has live cells, so it skips the
            // gate and keeps the old canvases instead of remounting them.
            if was {
                if let Some(h) = opening_timer.get_value() {
                    h.clear();
                    opening_timer.set_value(None);
                }
                if cells_mounted.get_untracked() {
                    opening.set(false);
                } else {
                    opening.set(true);
                    match set_timeout_with_handle(
                        move || {
                            opening.set(false);
                            cells_mounted.set(true);
                        },
                        Duration::from_millis(SIDEBAR_SLIDE_MS),
                    ) {
                        Ok(h) => opening_timer.set_value(Some(h)),
                        // No timer available (should not happen): fall back to
                        // the pre-fix behaviour — mount immediately — rather
                        // than gating the thumbs off forever.
                        Err(_) => {
                            opening.set(false);
                            cells_mounted.set(true);
                        }
                    }
                }
            }
            was_closed.set_value(false);
        } else {
            was_closed.set_value(true);
            opening.set(false);
            // A close before the opening gate clears cancels that pending
            // mount. Without this clear the stale callback would claim cells
            // existed after the sidebar was already closed.
            if let Some(h) = opening_timer.get_value() {
                h.clear();
                opening_timer.set_value(None);
            }

            // Do not treat the initial closed state as a close transition.
            // Every actual panel → None change, however, slides the rail for
            // the full duration — even if the opening gate had not mounted
            // thumbnail cells yet — so chrome must use the same hold.
            if was {
                collapsing.set(false);
            } else {
                collapsing.set(true);
                if let Some(h) = collapse_timer.get_value() {
                    h.clear();
                }
                let handle = set_timeout_with_handle(
                    move || {
                        collapsing.set(false);
                        // This is a no-op for a pre-gate close, and releases
                        // live thumbnail canvases after an ordinary close.
                        cells_mounted.set(false);
                    },
                    Duration::from_millis(SIDEBAR_SLIDE_MS),
                )
                .ok();
                collapse_timer.set_value(handle);
            }
        }
    });

    SidebarPaint {
        show_outline: Signal::derive(move || {
            panel_is_shown(SidebarMode::Outline, mode.get(), collapsing.get(), last_mode.get())
        }),
        show_thumbs: Signal::derive(move || {
            panel_is_shown(SidebarMode::Thumbs, mode.get(), collapsing.get(), last_mode.get())
        }),
        thumbs_live: Signal::derive(move || {
            thumbnail_cells_are_live(
                cells_mounted.get(),
                mode.get(),
                collapsing.get(),
                last_mode.get(),
            )
        }),
        is_closed: Signal::derive(move || mode.get() == SidebarMode::None),
        outline_active: Signal::derive(move || mode.get() == SidebarMode::Outline),
        thumbs_active: Signal::derive(move || mode.get() == SidebarMode::Thumbs),
        opening: opening.into(),
        intro: opening.into(),
        present: Signal::derive(move || sidebar_is_present(mode.get(), collapsing.get())),
    }
}

/// Final mount gate for thumbnail cells. Even if the panel state would
/// normally preserve cells through an outro, there is nothing to preserve
/// when a close arrives before the opening delay created any cells.
pub(crate) fn thumbnail_cells_are_live(
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
pub(crate) fn thumbs_should_stay_mounted(
    mode: SidebarMode,
    collapsing: bool,
    last: SidebarMode,
) -> bool {
    match mode {
        SidebarMode::Thumbs | SidebarMode::Outline => true,
        SidebarMode::None => collapsing && last == SidebarMode::Thumbs,
    }
}

#[component]
pub fn Sidebar(
    mode: RwSignal<SidebarMode>,
    #[prop(into)] header: ViewFn,
    #[prop(optional, into)] info_row: Option<ViewFn>,
    #[prop(into)] panels: ViewFn,
    #[prop(optional, into)] footer: Option<ViewFn>,
) -> impl IntoView {
    view! {
        <aside
            class="sidebar-aside flex shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || matches!(mode.get(), SidebarMode::Thumbs | SidebarMode::Outline))
            class=("w-0", move || mode.get() == SidebarMode::None)
            class=("border-r-0", move || mode.get() == SidebarMode::None)
        >
            <div
                class="flex h-full w-72 min-h-0 flex-col"
                prop:inert=move || mode.get() == SidebarMode::None
            >
                {header.run()}
                {info_row.map(|row| row.run())}
                <div class="relative min-h-0 flex-1">
                    {panels.run()}
                </div>
                {footer.map(|row| row.run())}
            </div>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::{
        panel_is_shown, sidebar_is_present, thumbnail_cells_are_live, thumbs_should_stay_mounted,
        SIDEBAR_SLIDE_MS,
    };
    use crate::state::SidebarMode;

    #[test]
    fn the_slide_matches_the_css_duration() {
        // The fade, the width tween and the deferred unmount must share one
        // number. Drift here is how the thumbs popped off before the labels.
        assert_eq!(SIDEBAR_SLIDE_MS, 300);
    }

    #[test]
    fn a_close_keeps_the_open_panel_painted_until_the_slide_ends() {
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
    fn chrome_space_is_held_until_the_close_slide_lands() {
        assert!(sidebar_is_present(SidebarMode::Thumbs, false));
        // Frame one of a close: raw mode is None, but the aside is still
        // sliding and title-bar chrome must remain aligned with it.
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
    fn a_pre_gate_close_cannot_make_thumbnail_cells_live() {
        // The old close path made `collapsing` true even though the opening
        // delay had not mounted anything. That state must not create cells.
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
