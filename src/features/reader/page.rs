//! Document-owned effects and surfaces. Window chrome lives in workspace-wasm.
use leptos::prelude::*;

use crate::components::shell::titlebar::floating_document_title::FloatingDocumentTitle;
use app_chrome::hooks::dom::VIEWER_SLOT_ID;
use crate::components::viewer::controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer::controls::page_indicator::PageIndicator;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::features::reader::use_reader_virtualizers;
use crate::state::AppState;
use reader_core::settings::PageIndicatorStyle;
use pdf_engine::types::DocStatus;

#[component]
pub fn ReaderSurface(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;

    let rv = use_reader_virtualizers(vs);

    // The layout prefs (page gap, page margin) resolve their settings into the
    // strips' size models. Installed BEFORE the reflow layout effect below,
    // which reads the gap they resolve.
    crate::effects::reader::layout_prefs::layout_prefs(
        state,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );

    // The paged text modes' A4 page model upkeep: page-unit sizes projected
    // into the shared measurement store whenever the format, the mode or the
    // cut moves, and reverted when they stop asking for it (the vertical
    // reading mode is the stream's, which virtualizes blocks directly).
    // Installed AFTER the gap effects so its relayout reads the gap they just
    // resolved.
    crate::effects::reader::reflow_layout::reflow_layout(state, rv.virtualizer.clone());
    // The reflowable measurement pipeline: the pipe the stream's and the
    // page hosts' block measurements flow through into the page cut, and the
    // re-estimate that follows the typography and the width dials. Installed
    // beside the layout it feeds.
    crate::effects::reader::reflow_measure::install_reflow_measure(state);
    // The Markdown outline follows the same page cut, so it is installed beside
    // it: one re-cut republishes the pages AND moves the chapters.
    crate::effects::reader::reflow_outline::reflow_outline(state);

    // What a mode flip owes: the incoming strip's anchor, the stream's zoom, the
    // outgoing view's rasters, and the fit the next mode owns.
    crate::effects::reader::mode_change::mode_change(state);

    let actuator = crate::zoom::actuator::ZoomActuator::new(rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // Driven once at setup. The controller itself is dropped when setup
    // returns; what outlives it are the effects `drive` installs, which live as
    // long as this page's reactive owner. Everything downstream only posts
    // commands; nothing else writes a zoom scale or rescales a strip.
    let zoom = crate::zoom::ZoomController::new(actuator);
    zoom.drive(vs);
    // Installed BEFORE reading_progress, and that is a contract rather than a
    // habit: Leptos runs effects in insertion order, so when a zoom
    // transaction closes both wake in the same flush — this one replays its
    // held jump first, and reading progress then persists the page the reader
    // actually asked for instead of the stale dominant the strip still
    // shows.
    navigation_sync(vs, rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom sources come last, after the controller that consumes them: a
    // container follow on every frame of a sidebar slide or window drag (each
    // burst has its own switch; with it off the follow lands the end frame
    // once instead of frame by frame), and a debounced refit when a fit's
    // other inputs move (mode, and the page too — only while Auto Resize is
    // on).
    crate::effects::reader::zoom_watchers::follow_watcher(state, state.ui.sidebar);
    crate::effects::reader::zoom_watchers::fit_watcher(state);
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    reading_progress(state);
    // The blend backdrop's geometry half: the viewport's ladder position per
    // scroll tick (the engine owns the colours it drives). The backdrop
    // carries no texture — each page's own ::before paints the gutter
    // (textures.css, BLEND BLEED) — so there is nothing here to sync, only the
    // colour position the engine consumes. The SETTINGS half lives at the app
    // root, ahead of the first document open.
    crate::effects::reader::blend_backdrop::blend_backdrop(state);

    crate::effects::reader::first_paint::first_paint_gate(state);

    let status = state.reader.document.status;
    let is_ready = move || status.get() == DocStatus::Ready;

    let show_indicator = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator));
    let indicator_style = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator_style));
    let progress_visible = Signal::derive(move || state.settings.with(|st| st.layout.progress_bar));
    // Continuous text reading has no meaningful page number: while the stream
    // is live the badge is a percentage of the document whatever the indicator
    // style says — and the style selector stands disabled for exactly as long,
    // so it cannot show a choice that is not being honoured.
    let stream_live = Signal::derive(move || vs.reflow_streaming());
    let stream_percent = Signal::derive(move || vs.stream_percent());

    view! {
            // overflow-hidden clips the hidden ReaderBottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("blend", move || {
                    // The blend class swaps the backdrop AND the page hosts
                    // onto the engine's one computed paper colour
                    // (styles/components/shell.css, styles/page_host.css),
                    // killing the fractional-edge rim a second paper colour
                    // under the canvas used to show. A text/Markdown page is
                    // its OWN paper (the surface paints --tx-paper), so the
                    // class must not run for it or a second paper colour
                    // stacks under the text page.
                    state.settings.with(|st| st.layout.blend_mode)
                        && !state.reader.reflowable()
                })
            >
                <div class="relative flex min-h-0 flex-1">
                    <main
                        id=VIEWER_SLOT_ID
                        class="relative min-w-0 flex-1 overflow-hidden"
                        class=("no-page-shadow", move || !state.settings.with(|st| st.layout.page_shadow))
                    >
                        <Show when=is_ready>
                            <crate::components::viewer::Viewer
                                state=vs
                                virtualizer=rv.virtualizer_view.get_value()
                                h_virtualizer=rv.h_virtualizer_view.get_value()
                                progress_visible=progress_visible
                            />
                        </Show>
                        // The first-paint cover (the gate effects above own
                        // its timing): an opaque sheet of the paper the reader
                        // is about to paint, over everything the viewer slot
                        // stacks, until the reading surface has landed on the
                        // resume point. Lifting is seamless in every theme
                        // because it wears the same paper token as the surface
                        // underneath.
                        <Show when=move || is_ready() && !state.reader.viewer.first_paint.get()>
                            <div
                                class=format!(
                                    "absolute inset-0 {} flex items-center justify-center",
                                    app_chrome::layers::DRAG_OVERLAY
                                )
                                style=move || format!(
                                    "background:{}",
                                    if state.reader.reflowable() {
                                        "var(--tx-paper)"
                                    } else {
                                        "var(--color-paper)"
                                    }
                                )
                            >
                                <crate::components::primitives::feedback::CenteredLoader />
                            </div>
                        </Show>
                        <FloatingDocumentTitle state=state />
                        // Corner page counter, gated on a ready document and
                        // positioned by the page; the indicator itself is
                        // reusable UI with no knowledge of AppState.
                        <Show when=move || is_ready() && show_indicator.get()>
                            <div class=format!("pointer-events-none absolute bottom-3 right-3 {}", app_chrome::layers::CONTROLS)>
                                <PageIndicator
                                    current=Signal::derive(move || {
                                        if stream_live.get() {
                                            stream_percent.get()
                                        } else {
                                            vs.viewer.page.get()
                                        }
                                    })
                                    total=Signal::derive(move || {
                                        if stream_live.get() {
                                            100
                                        } else {
                                            vs.document.num_pages.get()
                                        }
                                    })
                                    style=Signal::derive(move || {
                                        if stream_live.get() {
                                            PageIndicatorStyle::Percentage
                                        } else {
                                            indicator_style.get()
                                        }
                                    })
                                    hidden=Signal::derive(move || state.reader.gloss.selection_active.get())
                                />
                            </div>
                        </Show>
                        <ReaderBottomBar
                            reader=vs
                        />
                        <crate::components::search::floating_search::FloatingSearch
                            state=vs
                            virtualizer=rv.virtualizer_view
                        />
                        <crate::components::ai::selection_pill::SelectionPill state=state />
                        <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
                    </main>
                </div>
            </div>
    }
}
