//! The document pane's own surface: its effects, its content, one pane's
//! state only.

use leptos::prelude::*;

use crate::components::shell::titlebar::floating_document_title::FloatingDocumentTitle;
use crate::components::viewer::controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer::controls::page_indicator::PageIndicator;
use crate::context::ReaderContext;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::features::virtualizers::{ReaderVirtualizers, use_reader_virtualizers};
use reader_core::document::DocStatus;
use reader_core::settings::PageIndicatorStyle;

/// Install every reader effect the pane owns, in its reactive owner.
pub(crate) fn install_pane_effects(
    state: ReaderContext,
    active: Signal<bool>,
) -> ReaderVirtualizers {
    let vs = state.reader;

    // Reader-only event arms: each window listener dies with this scope.
    crate::effects::reader::link_navigation::link_navigation(state, active);
    crate::effects::reader::page_selection::page_selection(state, active);
    crate::effects::reader::selection_tracking::selection_tracking(state, active);
    // The keyboard arm answers only while this pane is the host's active
    // pane.
    crate::effects::reader::shortcuts::shortcuts(
        state.reader,
        move || {
            crate::services::document::open_dialog(state, crate::host::contract::Placement::Here)
        },
        expect_context::<app_ui::components::shell::controller::ShellController>(),
        move || active.try_get_untracked().unwrap_or(false),
    );

    let rv = use_reader_virtualizers(vs, state.pane);

    // The layout prefs resolve page gap and margin into the strips' size
    // models.
    crate::effects::reader::layout_prefs::layout_prefs(
        state,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );

    // The paged text modes' A4 page model upkeep, after the gap effects.
    #[cfg(feature = "reflow")]
    crate::effects::reader::reflow_layout::reflow_layout(state, rv.virtualizer.clone());
    // The reflowable measurement pipeline, beside the layout it feeds.
    #[cfg(feature = "reflow")]
    crate::effects::reader::reflow_measure::install_reflow_measure(state);
    // The Markdown outline follows the same page cut.
    #[cfg(feature = "reflow")]
    crate::effects::reader::reflow_outline::reflow_outline(state);
    // The outline's jump into the stream, beside the outline it moves.
    #[cfg(feature = "reflow")]
    crate::effects::reader::outline_jump::outline_jump(state.reader);

    // What a mode flip owes: anchors, zoom, rasters, the next fit.
    crate::effects::reader::mode_change::mode_change(state);

    let actuator = crate::zoom::actuator::ZoomActuator::new(
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
        vs.dom,
    );
    // Driven once at setup; the effects `drive` installs live with this
    // owner.
    let zoom = crate::zoom::ZoomController::new(actuator);
    zoom.drive(vs);
    // Installed BEFORE reading_progress: effects run in insertion order, so
    // the held jump replays first.
    navigation_sync(vs, rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom sources come last, after the controller that consumes them.
    crate::effects::reader::zoom_watchers::follow_watcher(state, state.ui.sidebar);
    crate::effects::reader::zoom_watchers::fit_watcher(state);
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    reading_progress(state);
    // The blend backdrop's geometry half: the ladder position per scroll
    // tick.
    #[cfg(feature = "pdf")]
    crate::effects::reader::blend_backdrop::blend_backdrop(state);

    crate::effects::reader::first_paint::first_paint_gate(state);

    rv
}

/// The pane's content: the viewer, the first-paint cover, the overlays.
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
    // Continuous text has no page number: the badge is a percentage.
    let stream_live = Signal::derive(move || vs.reflow_streaming());
    let stream_percent = Signal::derive(move || vs.stream_percent());

    let view = view! {
        <div
            node_ref=dom.root_ref()
            // `crate::pane::origin::PANE_ROOT_ATTR`: an event raised inside
            // finds its pane by this.
            data-pane-root=""
            // The open pipeline, tracked: paper and texture rules key off it.
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
            // The first-paint cover, whose timing the gate effect owns.
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
            // Corner page counter, gated on a ready document.
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
            <crate::components::ai::selection_menu::SelectionMenu state=state />
            <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
            <crate::components::dict::hover_card::DictHoverHost state=state />
        </div>
    };

    // The independent-theme paint: the pane root carries the look's tokens
    // while it owns one.
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
            // The engine discovers its pipeline here; notify only on changed
            // bake inputs.
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
