//! Left sidebar with tabs (outline / thumbnails). OWNED BY branch C
//! (panels/sidebar). Search moved out of the sidebar into the floating overlay
//! (Phase 2), so this rail only carries Outline + Thumbs.
//!
//! The `<aside>` is ALWAYS mounted and animates its `width` between 18rem and 0
//! (single-phase slide, no two-phase unmount). The inner content stays fixed at
//! `w-72` so it doesn't collapse mid-transition — `overflow-hidden` on the aside
//! clips it while closed. When collapsed the content is made `inert` so the
//! clipped rail can't be tab-focused / activated.
//!
//! CLOSE / OPEN (appendix 22). The rail labels (Thumbs / Outline) outro by
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

use pdf_viewer::components::atoms::icon::IconName;
use crate::components::molecules::sidebar_item::SidebarItem;
use pdf_viewer::components::outline_panel::OutlinePanel;
use pdf_viewer::components::thumbnails::ThumbnailsPanel;
use crate::core::state::AppState;
use pdf_viewer::state::SidebarMode;

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
fn request_reveal_active() {
    if let Some(win) = web_sys::window() {
        _ = win.dispatch_event(&web_sys::CustomEvent::new("pdfreader:reveal-active").unwrap());
    }
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
pub fn Sidebar(state: AppState) -> impl IntoView {
    let viewer_state = pdf_viewer::state::ViewerState::new(state.doc, state.viewer, state.search, state.sidebar);
    // Last non-None mode, and whether a close slide is still in flight.
    // `last` is what we keep painted during the outro; `collapsing` flips
    // off SIDEBAR_SLIDE_MS after a close so the grid can unmount. A reopen
    // inside that window clears the timer and never unmounts.
    let last_mode = RwSignal::new(SidebarMode::Thumbs);
    let collapsing = RwSignal::new(false);
    let collapse_timer = StoredValue::new_local(None::<TimeoutHandle>);

    Effect::new(move |_| {
        let mode = state.sidebar.get();
        if mode != SidebarMode::None {
            last_mode.set(mode);
            collapsing.set(false);
            if let Some(h) = collapse_timer.get_value() {
                h.clear();
                collapse_timer.set_value(None);
            }
        } else {
            collapsing.set(true);
            if let Some(h) = collapse_timer.get_value() {
                h.clear();
            }
            let handle = set_timeout_with_handle(
                move || collapsing.set(false),
                Duration::from_millis(SIDEBAR_SLIDE_MS),
            )
            .ok();
            collapse_timer.set_value(handle);
        }
    });

    let show_outline = Signal::derive(move || {
        panel_is_shown(
            SidebarMode::Outline,
            state.sidebar.get(),
            collapsing.get(),
            last_mode.get(),
        )
    });
    let show_thumbs = Signal::derive(move || {
        panel_is_shown(
            SidebarMode::Thumbs,
            state.sidebar.get(),
            collapsing.get(),
            last_mode.get(),
        )
    });
    let thumbs_live = Signal::derive(move || {
        thumbs_should_stay_mounted(state.sidebar.get(), collapsing.get(), last_mode.get())
    });
    let is_closed = Signal::derive(move || state.sidebar.get() == SidebarMode::None);

    // Tab rail: re-runs when the active mode changes so the `active` highlight
    // stays in sync.
    let header = move || {
        let mode = state.sidebar.get();
        view! {
            <div class="flex flex-col gap-0.5 border-b border-line p-2">
                <SidebarItem
                    icon=IconName::Thumbs
                    label="Thumbs".to_string()
                    active=mode == SidebarMode::Thumbs
                    on_click=move || {
                        // Re-clicking the ACTIVE tab means "take me to where I
                        // am", not "close". Closing is what the toolbar's
                        // Toggle sidebar button is for, and a reader who has
                        // scrolled the panel away from their position has no
                        // other way to get back to it — they were hunting for
                        // it by hand.
                        if state.sidebar.get() == SidebarMode::Thumbs {
                            request_reveal_active();
                        } else {
                            state.sidebar.set(SidebarMode::Thumbs);
                        }
                    }
                />
                <SidebarItem
                    icon=IconName::Outline
                    label="Outline".to_string()
                    active=mode == SidebarMode::Outline
                    on_click=move || {
                        if state.sidebar.get() == SidebarMode::Outline {
                            request_reveal_active();
                        } else {
                            state.sidebar.set(SidebarMode::Outline);
                        }
                    }
                />
            </div>
        }
    };

    view! {
        <aside
            class="flex shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || state.sidebar.get() != SidebarMode::None)
            class=("w-0", move || state.sidebar.get() == SidebarMode::None)
            class=("border-r-0", move || state.sidebar.get() == SidebarMode::None)
        >
            // pt-12 == TOOLBAR_H. The content row spans the full window height
            // so pages can travel under the glass toolbar, which means this
            // rail starts at y=0 too — without the inset the first tab
            // (Thumbs) renders behind the z-50 header and cannot be clicked.
            <div
                class="flex h-full w-72 min-h-0 flex-col pt-12"
                prop:inert=move || state.sidebar.get() == SidebarMode::None
            >
                {header}
                <div class="relative min-h-0 flex-1">
                    <div
                        class="sidebar-panel absolute inset-0 flex flex-col"
                        class=("invisible", move || !show_outline.get())
                        class=("is-outro", move || is_closed.get())
                    >
                        <OutlinePanel state=viewer_state />
                    </div>
                    <div
                        class="sidebar-panel absolute inset-0 flex flex-col"
                        class=("invisible", move || !show_thumbs.get())
                        class=("is-outro", move || is_closed.get())
                    >
                        <ThumbnailsPanel state=viewer_state live=thumbs_live />
                    </div>
                </div>
            </div>
        </aside>
    }
}

#[cfg(test)]
mod tests {
    use super::{panel_is_shown, thumbs_should_stay_mounted, SIDEBAR_SLIDE_MS};
    use pdf_viewer::state::SidebarMode;

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
