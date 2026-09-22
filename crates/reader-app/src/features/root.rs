//! The reader root: everything below the title bar — the rail's two mount
//! points, the viewer slot and the floating surfaces over it — and the effect
//! installations that make them agree.
//!
//! This is the composition the shell mounts for its reader route, and the one
//! a reader of its own would mount for itself: it takes the open document's
//! state, the persisted settings, the library's cover map and the shell's
//! layout rulebook, and nothing else. There is no window in here — no bar, no
//! route, no modal — and the one thing it owes the world outside a document
//! (the reading position, which the library persists) leaves through the
//! `record_progress` callback rather than through a dependency.
//!
//! The effect order below is a contract, not a habit. Read the comments at
//! each installation before moving one.

use leptos::prelude::*;

use app_chrome::controller::ShellController;
use app_chrome::hooks::dom::VIEWER_SLOT_ID;
use library_core::covers::CoverMap;
use pdf_engine::types::DocStatus;
use reader_core::settings::{PageIndicatorStyle, Settings};

use crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover;
use crate::components::ai::selection_pill::SelectionPill;
use crate::components::rail::overlay::OverlayRail;
use crate::components::rail::push::PushRail;
use crate::components::search::floating_search::FloatingSearch;
use crate::components::viewer::Viewer;
use crate::components::viewer::controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer::controls::page_indicator::PageIndicator;
use crate::components::viewer::floating_title::FloatingDocumentTitle;
use crate::effects::navigation_sync::navigation_sync;
use crate::features::rail::ReaderRail;
use crate::features::use_reader_virtualizers;
use crate::state::ReaderState;

#[component]
pub fn ReaderRoot(
    /// The open document's reactive truth.
    reader: ReaderState,
    /// The persisted settings: the viewer's knobs are read from here and the
    /// reader's own rows write back to it. Loading and saving them is the
    /// shell's.
    settings: RwSignal<Settings>,
    /// The library's cover map, for the rail's identity row.
    covers: RwSignal<CoverMap>,
    /// The shell's layout truth, built by whoever mounted this root so both
    /// rail mount points and every chrome component ask ONE controller.
    shell: ShellController,
    /// The shell's reading-position write-back, run at its documented point
    /// in the effect order below: after `navigation_sync`, so a zoom
    /// transaction that closes replays its held jump before the page anyone
    /// persists is read. The reader cannot do this itself — the resume point
    /// lands in the library's rows.
    record_progress: Callback<()>,
) -> impl IntoView {
    let rv = use_reader_virtualizers(reader);

    // The layout prefs (page gap, page margin) resolve their settings into the
    // strips' size models. Installed BEFORE the reflow layout effect below,
    // which reads the gap they resolve.
    crate::effects::layout_prefs::layout_prefs(
        reader,
        settings,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );

    // The paged text modes' A4 page model upkeep: page-unit sizes projected
    // into the shared measurement store whenever the format, the mode or the
    // cut moves, and reverted when they stop asking for it (the vertical
    // reading mode is the stream's, which virtualizes blocks directly).
    // Installed AFTER the gap effects so its relayout reads the gap they just
    // resolved.
    crate::effects::reflow_layout::reflow_layout(reader, rv.virtualizer.clone());
    // The reflowable measurement pipeline: the pipe the stream's and the
    // page hosts' block measurements flow through into the page cut, and the
    // re-estimate that follows the typography and the width dials. Installed
    // beside the layout it feeds.
    crate::effects::reflow_measure::install_reflow_measure(reader);
    // The Markdown outline follows the same page cut, so it is installed beside
    // it: one re-cut republishes the pages AND moves the chapters.
    crate::effects::reflow_outline::reflow_outline(reader);

    // What a mode flip owes: the incoming strip's anchor, the stream's zoom, the
    // outgoing view's rasters, and the fit the next mode owns.
    crate::effects::mode_change::mode_change(reader, settings);

    let actuator =
        crate::zoom::actuator::ZoomActuator::new(rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // Driven once at setup. The controller itself is dropped when setup
    // returns; what outlives it are the effects `drive` installs, which live as
    // long as this root's reactive owner. Everything downstream only posts
    // commands; nothing else writes a zoom scale or rescales a strip.
    let zoom = crate::zoom::ZoomController::new(actuator);
    zoom.drive(reader);
    // Installed BEFORE the reading-position write-back, and that is a contract
    // rather than a habit: Leptos runs effects in insertion order, so when a
    // zoom transaction closes both wake in the same flush — this one replays
    // its held jump first, and the write-back then persists the page the
    // reader actually asked for instead of the stale dominant the strip still
    // shows.
    navigation_sync(reader, rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom sources come last, after the controller that consumes them: a
    // container follow on every frame of a rail slide or window drag (each
    // burst has its own switch; with it off the follow lands the end frame
    // once instead of frame by frame), and a debounced refit when a fit's
    // other inputs move (mode, and the page too — only while Auto Resize is
    // on).
    crate::effects::zoom_watchers::follow_watcher(reader, shell.sidebar_mode);
    crate::effects::zoom_watchers::fit_watcher(reader, settings);
    crate::effects::auto_scroll::auto_scroll(reader);
    record_progress.run(());
    // The blend backdrop's geometry half: the viewport's ladder position per
    // scroll tick (the engine owns the colours it drives). The backdrop
    // carries no texture — each page's own ::before paints the gutter
    // (textures.css, BLEND BLEED) — so there is nothing here to sync, only the
    // colour position the engine consumes. The SETTINGS half lives at the app
    // root, ahead of the first document open.
    crate::effects::blend_backdrop::blend_backdrop(reader, settings);

    crate::effects::first_paint::first_paint_gate(reader);

    let status = reader.document.status;
    let is_ready = move || status.get() == DocStatus::Ready;

    let show_indicator = Signal::derive(move || settings.with(|st| st.layout.page_indicator));
    let indicator_style = Signal::derive(move || settings.with(|st| st.layout.page_indicator_style));
    let progress_visible = Signal::derive(move || settings.with(|st| st.layout.progress_bar));
    // Continuous text reading has no meaningful page number: while the stream
    // is live the badge is a percentage of the document whatever the indicator
    // style says — and the style selector stands disabled for exactly as long,
    // so it cannot show a choice that is not being honoured.
    let stream_live = Signal::derive(move || reader.reflow_streaming());
    let stream_percent = Signal::derive(move || reader.stream_percent());

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
                settings.with(|st| st.layout.blend_mode) && !reader.reflowable()
            })
        >
            <div class="relative flex min-h-0 flex-1">
                // DOCKED: the rail is a flex sibling of `<main>`, so the
                // page gives up the width. `PushRail` renders nothing
                // while the controller says the layout is overlay.
                <PushRail shell=shell>
                    <ReaderRail reader covers shell=shell />
                </PushRail>
                <main
                    id=VIEWER_SLOT_ID
                    class="relative min-w-0 flex-1 overflow-hidden"
                    class=("no-page-shadow", move || !settings.with(|st| st.layout.page_shadow))
                >
                    <Show when=is_ready>
                        <Viewer
                            state=reader
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
                    <Show when=move || is_ready() && !reader.viewer.first_paint.get()>
                        <div
                            class=format!(
                                "absolute inset-0 {} flex items-center justify-center",
                                app_chrome::layers::DRAG_OVERLAY
                            )
                            style=move || format!(
                                "background:{}",
                                if reader.reflowable() {
                                    "var(--tx-paper)"
                                } else {
                                    "var(--color-paper)"
                                }
                            )
                        >
                            <ui_kit::feedback::CenteredLoader />
                        </div>
                    </Show>
                    <FloatingDocumentTitle reader settings />
                    // Corner page counter, gated on a ready document and
                    // positioned by the page; the indicator itself is
                    // reusable UI with no knowledge of ReaderState.
                    <Show when=move || is_ready() && show_indicator.get()>
                        <div class=format!("pointer-events-none absolute bottom-3 right-3 {}", app_chrome::layers::CONTROLS)>
                            <PageIndicator
                                current=Signal::derive(move || {
                                    if stream_live.get() {
                                        stream_percent.get()
                                    } else {
                                        reader.viewer.page.get()
                                    }
                                })
                                total=Signal::derive(move || {
                                    if stream_live.get() {
                                        100
                                    } else {
                                        reader.document.num_pages.get()
                                    }
                                })
                                style=Signal::derive(move || {
                                    if stream_live.get() {
                                        PageIndicatorStyle::Percentage
                                    } else {
                                        indicator_style.get()
                                    }
                                })
                                hidden=Signal::derive(move || reader.gloss.selection_active.get())
                            />
                        </div>
                    </Show>
                    <ReaderBottomBar reader />
                    <FloatingSearch
                        state=reader
                        virtualizer=rv.virtualizer_view
                    />
                    <SelectionPill state=reader />
                    <GlossAiPopover reader settings />
                </main>
            </div>
        </div>
        // OVERLAY: `OverlayRail` mounts OUTSIDE `.reader-bg`, which is a
        // stacking context at z-index 0 — a rail inside it would paint
        // under the title bar's band and hand the band its whole 48px
        // header (close, search, More, and the native traffic lights the
        // header's 88px gutter reserves). Out here its own z-popover
        // outranks the bar, so the rail covers the bar's left corner
        // (Library button included) and takes the lights with it, and the
        // bar reads as one full-width surface either way. Renders nothing
        // while the controller says the layout is docked.
        <OverlayRail shell=shell>
            <ReaderRail reader covers shell=shell />
        </OverlayRail>
    }
}
