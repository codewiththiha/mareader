//! The `/reader` route: sidebar + viewer slot + the app title bar, plus the
//! floating doc title, page pill, bottom bar and floating search. The viewer
//! slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches must not edit this
//! file. The shell's layout truth lives in one `ShellController` built here
//! and provided as context; the rail's two mount points (`PushRail` in the
//! flex row, `OverlayRail` above the reader surface) and every chrome
//! component ask it instead of recomputing layout facts.

use leptos::prelude::*;

use crate::components::shell::controller::ShellController;
use crate::components::shell::sidebar::overlay::OverlayRail;
use crate::components::shell::sidebar::push::PushRail;
use crate::components::shell::titlebar::app_title_bar::AppTitleBar;
use crate::components::shell::titlebar::document_title::CenteredDocTitle;
use crate::components::shell::titlebar::floating_document_title::FloatingDocumentTitle;
use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::menus::reader_menu::ReaderMenu;
use crate::components::settings::modal::SettingsModal;
use crate::components::primitives::controls::button::{Button, ButtonVariant};
use app_chrome::hooks::dom::{TOOLBAR_LEADING_ID, VIEWER_SLOT_ID};
use app_chrome::icon::{Icon, IconName};
use app_chrome::tooltip::Tooltip;
use crate::features::reader::rail::ReaderRail;
use crate::components::viewer::controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer::controls::page_indicator::PageIndicator;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::features::reader::use_reader_virtualizers;
use crate::services::document::close_document;
use crate::state::AppState;
use reader_core::settings::PageIndicatorStyle;
use pdf_engine::types::DocStatus;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;

    // The shell's layout brain: one controller for the whole page, provided
    // as context for the title bar, the traffic lights, the floating label
    // and both rail mount points. It owns the open/close slide machine, so
    // the chrome stays aligned with the rail's pixels for the whole length
    // of a slide.
    let shell = ShellController::reader(state);
    provide_context(shell);

    let rv = use_reader_virtualizers(vs);

    // The layout prefs (page gap, page margin) resolve their settings into the
    // strips' size models. Installed BEFORE the reflow layout effect below,
    // which reads the gap they resolve.
    crate::effects::reader::layout_prefs::layout_prefs(
        state,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );

    // The vertical text strip's size model: page units sized to the sum of
    // their blocks, projected into the shared measurement store whenever the
    // format, the mode or the cut moves (and reverted to A4 when any of
    // those stop asking for it). Installed AFTER the gap effects so its
    // relayout reads the gap they just resolved.
    crate::effects::reader::reflow_layout::reflow_layout(state, rv.virtualizer.clone());
    // The Markdown outline follows the same page cut, so it is installed beside
    // it: one re-cut republishes the pages AND moves the chapters.
    crate::effects::reader::reflow_outline::reflow_outline(state);

    // What a mode flip owes: the incoming strip's anchor, the stream's zoom, the
    // outgoing view's rasters, and the fit the next mode owns.
    crate::effects::reader::mode_change::mode_change(state);

    let actuator = crate::zoom::actuator::ZoomActuator::new(rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom controller is created and driven here, and lives exactly as
    // long as this page's reactive owner. Everything downstream only posts
    // commands; nothing else writes a zoom scale or rescales a strip.
    let zoom = crate::zoom::ZoomController::new(actuator);
    zoom.drive(vs);
    // BEFORE reading_progress, and that is a contract rather than a habit.
    // Leptos runs effects in insertion order, so when a zoom transaction
    // closes both wake in the same flush: this one replays its held jump
    // first, and reading progress then persists the page the reader actually
    // asked for instead of the stale dominant the strip still shows.
    navigation_sync(vs, rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // The zoom sources come last, after the controller that consumes them:
    // a container follow on every frame of a sidebar slide or a window drag
    // (each of those two bursts has its own switch, and with its switch off the
    // follow lands the end frame once instead of frame by frame), and a
    // debounced refit when a fit's other inputs move (mode, and the page too —
    // but only while the Auto Resize setting is on).
    crate::effects::reader::zoom_watchers::follow_watcher(state, state.ui.sidebar);
    crate::effects::reader::zoom_watchers::fit_watcher(state);
    crate::effects::reader::auto_scroll::auto_scroll(vs);
    reading_progress(state);
    // The blend backdrop's geometry half: the viewport's ladder position per
    // scroll tick (the engine owns the colours it drives). The backdrop
    // carries no texture — each page's own `::before` paints the gutter (see
    // textures.css BLEND BLEED), so there is nothing here to sync, only the
    // colour position the engine consumes. The SETTINGS half lives at the app
    // root, ahead of the first document open.
    crate::effects::reader::blend_backdrop::blend_backdrop(state);

    let status = state.reader.document.status;
    let is_ready = move || status.get() == DocStatus::Ready;

    let settings_open = RwSignal::new(false);
    // The settings modal is opened from several places (the 3-dash menu's
    // Settings… item, the sidebar header's gear) that sit under different
    // mount points, so the open signal is shared through context rather than
    // threaded as a prop through the rail composition.
    provide_context(settings_open);
    let show_indicator = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator));
    let indicator_style = Signal::derive(move || state.settings.with(|st| st.layout.page_indicator_style));
    let progress_visible = Signal::derive(move || state.settings.with(|st| st.layout.progress_bar));
    // Continuous text reading has no meaningful page number: while the
    // stream is live the badge is a percentage of the document whatever the
    // indicator style says (the style selector stands disabled for exactly
    // as long, so it cannot show a choice that is not being honoured).
    let stream_live = Signal::derive(move || vs.reflow_streaming());
    let stream_percent = Signal::derive(move || vs.stream_percent());

    // Left: sidebar toggle + Library. Title is centered; right is the 3-dash
    // view menu + Appearance.
    //
    // The sidebar toggle's visibility is the controller's rule: overlay mode
    // drops it (the rail opens by brushing the window's left edge and closes
    // from its own header, so a second switch in the bar only competes with
    // both). The Library button stays exactly where it is — the rail floats
    // above the bar and covers it while it is up, which is the rail's job,
    // not this cluster's. The cluster is always mounted so the row keeps its
    // left edge (and `#toolbar-leading`, the measurement anchor the library
    // title uses) wherever the mode puts it. Reader settings have no button
    // of their own here: they open from the 3-dash menu's Settings… item and
    // the sidebar header's gear.
    let left = move || {
        view! {
            <div
                id=TOOLBAR_LEADING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Show when=move || shell.show_sidebar_toggle().get()>
                    <Tooltip text="Toggle sidebar">
                        <Button
                            on_click=move |_| shell.toggle_sidebar()
                            variant=ButtonVariant::Ghost
                            title="Toggle sidebar"
                        >
                            <Icon name=IconName::Sidebar size=18 />
                        </Button>
                    </Tooltip>
                </Show>
                <Show when=move || {
                    matches!(
                        state.reader.document.status.get(),
                        DocStatus::Ready | DocStatus::Opening
                    )
                }>
                    <Tooltip text="Library">
                        <Button
                            on_click=move |_| close_document(state)
                            variant=ButtonVariant::Ghost
                            title="Close this book and return to the library"
                        >
                            <Icon name=IconName::Library size=18 />
                        </Button>
                    </Tooltip>
                </Show>
            </div>
        }
    };
    let center = move || view! { <CenteredDocTitle state=state /> };
    let right = move || {
        view! {
            <ReaderMenu state=state settings_open=settings_open />
            <AppearanceMenu state=state />
        }
    };

    view! {
        <AppTitleBar state=state left=left center=center right=right>
            // overflow-hidden clips the hidden ReaderBottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("blend", move || {
                    // The blend ::after is the PDF paper pipeline (the
                    // document's own paper through the canvas filter). A
                    // text/Markdown page is its OWN paper — the surface
                    // paints --tx-paper (see shell.css) — so the layer
                    // must not run for it, or a second (filtered) backdrop
                    // stacks under the text page.
                    state.settings.with(|st| st.layout.blend_mode)
                        && !state.reader.reflowable()
                })
            >
                <div class="relative flex min-h-0 flex-1">
                    // DOCKED: the rail is a flex sibling of `<main>`, so the
                    // page gives up the width. `PushRail` renders nothing
                    // while the controller says the layout is overlay.
                    <PushRail shell=shell>
                        <ReaderRail state=state shell=shell />
                    </PushRail>
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
                        // The offscreen measure column for text documents:
                        // mounted for as long as one is open, torn down with
                        // it. It renders every block once at scale 1 and
                        // refines the page cut from the DOM's real heights
                        // (see `components::formats::reflow::measure`).
                        <Show when=move || state.reader.reflowable()>
                            <crate::components::formats::reflow::ReflowMeasureColumn app=state />
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
            // OVERLAY: `OverlayRail` mounts OUTSIDE `.reader-bg`, for the
            // reason the modal below spells out. `.reader-bg` is a stacking
            // context at z-index 0, so a rail inside it paints under the
            // title bar's band and hands the band its whole 48px header —
            // the close, search and More buttons live there, and so do the
            // native traffic lights, which the header's 88px gutter reserves
            // for them. Out here its own z-popover outranks the bar, so the
            // rail covers the bar's left corner (the Library button
            // included) and takes the lights with it, and the bar reads as
            // one full-width surface either way. It renders nothing while
            // the controller says the layout is docked.
            <OverlayRail shell=shell>
                <ReaderRail state=state shell=shell />
            </OverlayRail>
            // The settings modal belongs to the window, not to the viewer: as a
            // child of `main` it sat inside `.reader-bg`, which is a stacking
            // context (position:relative + z-index:0), so the title bar's band
            // — a SIBLING of `.reader-bg` at z-bar — painted over the top of an
            // open modal. As a sibling of the page, its own z-popover token
            // outranks the bar, which is what a modal is supposed to do. It
            // also renders after the floating rail, so it wins their shared
            // z-popover token and still covers it.
            <SettingsModal state=state open=settings_open />
        </AppTitleBar>
    }
}
