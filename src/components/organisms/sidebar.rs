//! Left sidebar with panels (outline / thumbnails). OWNED BY branch C
//! (panels/sidebar). Search moved out of the sidebar into the floating overlay
//! (Phase 2); the panel toggles moved to a bottom icon-only rail (redesign).
//! The sidebar OWNS the titlebar chrome while it is open: a always-visible
//! 48px chrome row (traffic-light inset + close toggle + search + ⋯) and a
//! book-identity row (cover + title + author). They are not hover-gated — the
//! native traffic lights follow `sidebar_open || bar_visible`, so an open
//! sidebar keeps the lights on its own.
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

use pdf_engine::types::DocStatus;
use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_viewer::components::outline_panel::OutlinePanel;
use pdf_viewer::components::thumbnails::ThumbnailsPanel;
use crate::components::molecules::more_menu::MoreMenu;
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

/// Icon-only bottom-rail toggle. Active state is a rounded filled chip,
/// exactly like the reference's bookmark button.
#[component]
fn RailToggle(
    icon: IconName,
    title: &'static str,
    active: Signal<bool>,
    on_click: impl Fn() + 'static,
) -> impl IntoView {
    view! {
        <button
            type="button"
            title=title
            aria-label=title
            aria-pressed=move || active.get().to_string()
            on:click=move |_| on_click()
            class="inline-flex h-9 w-14 items-center justify-center rounded-lg transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            // Repo rule: each conditional class carries ONE token. A
            // space-separated value here throws a swallowed SyntaxError and
            // the highlight silently never applies.
            class=("bg-line", move || active.get())
            class=("text-ink", move || active.get())
            class=("text-muted", move || !active.get())
            class=("hover:text-ink", move || !active.get())
        >
            <Icon name=icon size=16 />
        </button>
    }
}

#[component]
pub fn Sidebar(state: AppState) -> impl IntoView {
    let viewer_state = state.viewer_state();
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

    // Bottom-rail active states: re-run when the mode changes so the rounded
    // chip highlight stays in sync.
    let outline_active = Signal::derive(move || state.sidebar.get() == SidebarMode::Outline);
    let thumbs_active = Signal::derive(move || state.sidebar.get() == SidebarMode::Thumbs);

    view! {
        <aside
            class="flex shrink-0 flex-col overflow-hidden border-r border-line bg-surface transition-[width] duration-300 ease-in-out"
            class=("w-72", move || state.sidebar.get() != SidebarMode::None)
            class=("w-0", move || state.sidebar.get() == SidebarMode::None)
            class=("border-r-0", move || state.sidebar.get() == SidebarMode::None)
        >
            // The content row spans the full window height. The chrome row at
            // the top carries the 48px traffic-light inset; the book-identity
            // row sits below it. Both are always visible while the sidebar is
            // open (not hover-gated) — the sidebar is the lights' home.
            <div
                class="flex h-full w-72 min-h-0 flex-col"
                prop:inert=move || state.sidebar.get() == SidebarMode::None
            >
                // ── Row 1: sidebar chrome. Always visible while open. The
                // native traffic lights overlay the left ~76px; the filled
                // panel glyph marks "sidebar is on". A drag region so the
                // window stays grabable from the sidebar.
                <div
                    class="flex h-12 shrink-0 items-center gap-1 pl-[88px] pr-2"
                    data-tauri-drag-region="true"
                >
                    <Tooltip text="Close sidebar".to_string()>
                        <Button
                            on_click=move |_| state.sidebar.set(SidebarMode::None)
                            kind=ButtonKind::Ghost
                            icon=IconName::SidebarOpen
                            title="Close sidebar".to_string()
                        />
                    </Tooltip>
                    // Floating-search toggle: raw button so pointerdown can
                    // stop propagation (the floating bar's outside-click
                    // dismiss listens on window pointerdown, which would
                    // otherwise close the bar and let the click re-open it —
                    // a one-way toggle).
                    <button
                        type="button"
                        data-search-chrome="true"
                        title="Search (Cmd/Ctrl+F)"
                        on:pointerdown=move |ev| ev.stop_propagation()
                        on:click=move |_| {
                            let vs = state.viewer_state();
                            if state.search.visible.get() {
                                pdf_viewer::effects::search_effects::dismiss_search(vs);
                            } else {
                                pdf_viewer::effects::search_effects::resume_search(vs);
                            }
                        }
                        class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        <Icon name=IconName::Search size=18 />
                    </button>
                    <MoreMenu state=state />
                </div>

                // ── Row 2: book identity (cover + title + author + info).
                // Always visible while open and a document is Ready.
                <Show when=move || state.doc.status.get() == DocStatus::Ready>
                    <div
                        class="flex items-center gap-3 border-b border-line px-3 pb-3"
                        data-tauri-drag-region="true"
                    >
                        {move || {
                            let path = state.doc.path.get().unwrap_or_default();
                            match state.covers.get().get(&path).cloned() {
                                Some(c) => view! {
                                    <img
                                        class="h-12 w-10 rounded-sm border border-line/60 object-cover"
                                        src=c.data_url
                                        alt="Cover"
                                        loading="lazy"
                                    />
                                }
                                .into_any(),
                                None => view! {
                                    <div class="flex h-12 w-10 items-center justify-center rounded-sm border border-line bg-surface">
                                        <Icon name=IconName::Open size=14 />
                                    </div>
                                }
                                .into_any(),
                            }
                        }}
                        <div class="min-w-0 flex-1" data-tauri-drag-region="true">
                            <p
                                class="truncate text-sm font-semibold text-ink"
                                data-tauri-drag-region="true"
                            >
                                {move || pdf_core::filename::display_name(
                                    state.doc.title.get().as_deref(),
                                    state.doc.path.get().as_deref(),
                                )
                                .unwrap_or_else(|| "No document".to_string())}
                            </p>
                            <p class="truncate text-xs text-muted">
                                {move || state.doc.author.get().unwrap_or_default()}
                            </p>
                        </div>
                        // Info: native tooltip with the full path is enough.
                        <span
                            title=move || state.doc.path.get().unwrap_or_default()
                            class="text-muted"
                        >
                            <Icon name=IconName::More size=14 />
                        </span>
                    </div>
                </Show>

                // ── Panels: stacked absolutely, invisible/is-outro toggles
                // drive which is painted (see module docs).
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

                // ── Bottom rail: icons-only horizontal panel toggles.
                <div class="flex shrink-0 items-center justify-around gap-1 border-t border-line p-1.5">
                    <RailToggle
                        icon=IconName::Thumbs
                        title="Thumbnails"
                        active=thumbs_active
                        on_click=move || {
                            // Re-clicking the ACTIVE tab means "take me to where
                            // I am", not "close".
                            if state.sidebar.get() == SidebarMode::Thumbs {
                                request_reveal_active();
                            } else {
                                state.sidebar.set(SidebarMode::Thumbs);
                            }
                        }
                    />
                    <RailToggle
                        icon=IconName::Outline
                        title="Outline"
                        active=outline_active
                        on_click=move || {
                            if state.sidebar.get() == SidebarMode::Outline {
                                request_reveal_active();
                            } else {
                                state.sidebar.set(SidebarMode::Outline);
                            }
                        }
                    />
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
