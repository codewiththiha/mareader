//! The document pane's own surface: the reader effects it installs and the
//! content it renders inside the host's workspace slot.
//!
//! This is the per-pane half of what the old `ReaderPage` did. The other
//! half — the title bar, the rail's mount points, the settings modal's
//! placement, the shell controller, the backdrop — is the host's
//! (`crate::host::view`). Everything here acts on ONE pane's state: its
//! document, its virtualizers, its zoom, its overlays. Nothing here names
//! the host.

use leptos::prelude::*;

use crate::components::shell::titlebar::floating_document_title::FloatingDocumentTitle;
use crate::components::viewer::controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer::controls::page_indicator::PageIndicator;
use crate::context::ReaderContext;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::features::virtualizers::{ReaderVirtualizers, use_reader_virtualizers};
use pdf_engine::types::DocStatus;
use reader_core::settings::PageIndicatorStyle;

/// Install every reader effect the pane owns, in its reactive owner, and
/// build its virtualizers. Called once, at mount, inside the pane's owner:
/// everything created here dies with the pane's dispose.
///
/// The ORDER is load-bearing and unchanged from the page this came out of
/// (see each comment).
pub(crate) fn install_pane_effects(
    state: ReaderContext,
    active: Signal<bool>,
) -> ReaderVirtualizers {
    let vs = state.reader;

    // Reader-only event arms: they act on this pane's state only (page
    // navigation, page selection, the AI selection anchor), so they are
    // installed in the pane's owner and die with it — each arm's window
    // listener unregisters with this scope.
    // The events arrive on the window for every pane alike; each arm keeps
    // only its own (`crate::pane::origin`).
    crate::effects::reader::link_navigation::link_navigation(state, active);
    crate::effects::reader::page_selection::page_selection(state, active);
    crate::effects::reader::selection_tracking::selection_tracking(state, active);
    // The keyboard arm (page navigation, zoom steps, the sidebar toggles,
    // Cmd/Ctrl+O) answers only while this pane is the host's active pane:
    // the gate reads the host's one focus authority, never a copy of it.
    // What it asks of the workspace goes through the HOST: the picked file
    // through the host's open command (`open_dialog` ends in `ctx.open`),
    // Escape's rail close through the host's shell controller (provided by
    // the host in the session scope this pane's owner descends from).
    crate::effects::reader::shortcuts::shortcuts(
        state.reader,
        move || {
            crate::services::document::open_dialog(state, crate::host::contract::Placement::Here)
        },
        expect_context::<app_ui::components::shell::controller::ShellController>(),
        move || active.try_get_untracked().unwrap_or(false),
    );

    let rv = use_reader_virtualizers(vs, state.pane);

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
    // cut moves, and reverted when they stop asking for it. Installed AFTER
    // the gap effects so its relayout reads the gap they just resolved.
    crate::effects::reader::reflow_layout::reflow_layout(state, rv.virtualizer.clone());
    // The reflowable measurement pipeline, beside the layout it feeds.
    crate::effects::reader::reflow_measure::install_reflow_measure(state);
    // The Markdown outline follows the same page cut: one re-cut republishes
    // the pages AND moves the chapters.
    crate::effects::reader::reflow_outline::reflow_outline(state);

    // What a mode flip owes: the incoming strip's anchor, the stream's zoom, the
    // outgoing view's rasters, and the fit the next mode owns.
    crate::effects::reader::mode_change::mode_change(state);

    let actuator = crate::zoom::actuator::ZoomActuator::new(
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
        vs.dom,
    );
    // Driven once at setup. What outlives the controller are the effects
    // `drive` installs, which live as long as this pane's owner. Everything
    // downstream only posts commands; nothing else writes a zoom scale or
    // rescales a strip.
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
    // container follow on every frame of a sidebar slide or window drag, and
    // a debounced refit when a fit's other inputs move.
    crate::effects::reader::zoom_watchers::follow_watcher(state, state.ui.sidebar);
    crate::effects::reader::zoom_watchers::fit_watcher(state);
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    reading_progress(state);
    // The blend backdrop's geometry half: the viewport's ladder position per
    // scroll tick (the engine owns the colours it drives). The SETTINGS half
    // was installed with the pane, ahead of its first document open.
    crate::effects::reader::blend_backdrop::blend_backdrop(state);

    crate::effects::reader::first_paint::first_paint_gate(state);

    rv
}

/// The pane's content for the host's workspace slot: the viewer, the
/// first-paint cover, and the per-document overlays (floating title, page
/// pill, bottom bar, find bar, selection pill, gloss popover). The host
/// places this inside its entry for the pane; the wrapper is the PANE's
/// root — sized to the bounds the host handed the pane (filling the entry
/// until the first measurement), the element every pane-owned lookup is
/// scoped to (`crate::pane::dom`), and the box every overlay's `absolute`
/// resolves against.
///
/// A pointer or keyboard focus landing inside the pane asks the host's
/// focus authority to make it active (`request_focus`); the authority
/// decides. Bubbling listeners: a control that swallows its own pointerdown
/// sits on a pane whose surface took the pointer first.
pub(crate) fn pane_content(
    state: ReaderContext,
    rv: ReaderVirtualizers,
    request_focus: Callback<()>,
) -> impl IntoView {
    let vs = state.reader;
    let dom = vs.dom;
    let measured = move || crate::pane::dom::measured(dom.bounds());
    let status = state.reader.document.status;
    let is_ready = move || status.get() == DocStatus::Ready;
    let show_indicator = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator));
    let indicator_style =
        Signal::derive(move || state.settings.with(|st| st.layout.page_indicator_style));
    let progress_visible = Signal::derive(move || state.settings.with(|st| st.layout.progress_bar));
    // Continuous text reading has no meaningful page number: while the stream
    // is live the badge is a percentage of the document whatever the indicator
    // style says.
    let stream_live = Signal::derive(move || vs.reflow_streaming());
    let stream_percent = Signal::derive(move || vs.stream_percent());

    let view = view! {
        <div
            node_ref=dom.root_ref()
            // `crate::pane::origin::PANE_ROOT_ATTR`: an event raised inside
            // finds its pane by this.
            data-pane-root=""
            // The open document's pipeline, tracked: the per-pane paper and
            // texture rules key off it (styles/components/shell.css,
            // styles/textures.css). The format lives on the pane root, not
            // on `:root` — one workspace can show several formats at once,
            // and the paper is a PANE fact. Reactive on purpose: an in-place
            // open swaps the pipeline under the same pane.
            data-format=move || match vs.document.format.get() {
                reader_core::format::Format::Pdf => "pdf",
                reader_core::format::Format::Text => "text",
                reader_core::format::Format::Markdown => "markdown",
            }
            class="absolute left-0 top-0"
            style:width=move || measured().map_or("100%".to_string(), |(w, _)| format!("{w}px"))
            style:height=move || measured().map_or("100%".to_string(), |(_, h)| format!("{h}px"))
            on:pointerdown=move |_| {
                request_focus.try_run(());
            }
            on:focusin=move |_| {
                request_focus.try_run(());
            }
        >
            <Show when=is_ready>
                <crate::components::viewer::Viewer
                    state=vs
                    virtualizer=rv.virtualizer_view.get_value()
                    h_virtualizer=rv.h_virtualizer_view.get_value()
                    progress_visible=progress_visible
                />
            </Show>
            // The first-paint cover (the gate effect owns its timing): an
            // opaque sheet of the paper the reader is about to paint, over
            // everything the pane stacks, until the reading surface has landed
            // on the resume point.
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
                    <app_ui::components::primitives::feedback::CenteredLoader />
                </div>
            </Show>
            <FloatingDocumentTitle state=state />
            // Corner page counter, gated on a ready document; the indicator
            // itself is reusable UI with no knowledge of the reader context.
            <Show when=move || is_ready() && show_indicator.get()>
                <div class=format!(
                    "pointer-events-none absolute bottom-3 right-3 {}",
                    app_chrome::layers::CONTROLS
                )>
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
            <ReaderBottomBar reader=vs />
            <crate::components::search::floating_search::FloatingSearch
                state=vs
                virtualizer=rv.virtualizer_view
            />
            <crate::components::ai::selection_pill::SelectionPill state=state />
            <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
        </div>
    };

    // The independent-theme paint: while the host's appearance boundary
    // hands this pane a look of its own, the pane root carries the look's
    // tokens (base + tint + texture — grain stays global and inherits);
    // with `None` the tokens are removed and the pane resolves to the
    // window theme again. The ink dial is a global text setting, so the
    // paint tracks it alongside the look. The root exists by the time this
    // effect first runs (the view above built it).
    let last_raster = StoredValue::new_local(None::<(String, String, String)>);
    Effect::new(move |_| {
        let look = vs.viewer.look.get();
        let ink = state.settings.with(|s| s.text.ink_contrast);
        let global = state.settings.with(|s| s.appearance);
        let signature_of = |a: reader_core::appearance::Appearance| {
            (
                a.canvas_filter(),
                a.canvas_blend().to_string(),
                a.base.as_str().to_string(),
            )
        };
        let global_signature = signature_of(global);
        let signature = signature_of(look.unwrap_or(global));
        let previous = last_raster.try_get_value().flatten();
        let changed = previous
            .as_ref()
            .is_some_and(|previous| previous != &signature)
            || (previous.is_none() && look.is_some() && signature != global_signature);
        last_raster.set_value(Some(signature));
        if let Some(el) = dom.root() {
            match look {
                Some(a) => app_ui::theme_paint::paint_pane_appearance(el, a, ink),
                None => app_ui::theme_paint::clear_pane_appearance(el),
            }
            // The PDF engine now discovers its pipeline at this pane root.
            // Notify it only after the new scoped tokens land, and only when
            // bake inputs changed: texture, grain, ink, and same-look seeding
            // are CSS-only and must not redraw full PDF page surfaces.
            if changed
                && vs.document.format.get_untracked() == reader_core::format::Format::Pdf
                && !app_ui::appearance::is_scrubbing()
            {
                app_ui::appearance::raster::refresh_after_pane_paint();
            }
        }
    });

    view
}
