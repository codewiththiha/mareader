//! The `/reader` route as the SHELL sees it: the title bar with its three
//! clusters, the settings modal, and the reader's surface —
//! [`ReaderRoot`], mounted from the reader crate — between them.
//!
//! This is the half of the old `ReaderPage` that is WINDOW-shaped. The
//! shell owns the open/close bookkeeping ([`close_document`]), the menus,
//! and the modal, so they stay here; the document surface below the bar is
//! the reader crate's, reached as a component with the state slices handed
//! over. The effects that feed the reader are installed HERE, in this one
//! order, because the order is a contract across all of them (zoom before
//! progress, gaps before relayout) and a contract split across a crate
//! boundary is a contract nobody can read in one place.

use leptos::prelude::*;

use app_chrome::controller::ShellController;
use app_chrome::hooks::dom::TOOLBAR_LEADING_ID;
use app_chrome::icon::{Icon, IconName};
use app_chrome::tooltip::Tooltip;
use ui_kit::primitives::controls::button::{Button, ButtonVariant};

use crate::components::menus::appearance_menu::AppearanceMenu;
use crate::components::menus::reader_menu::ReaderMenu;
use crate::components::settings::modal::SettingsModal;
use crate::effects::app::reading_progress::reading_progress;
use crate::services::document::close_document;
use crate::state::{AppState, GlossSave};
use reader_app::effects::auto_scroll::auto_scroll;
use reader_app::effects::blend_backdrop::blend_backdrop;
use reader_app::effects::first_paint::first_paint_gate;
use reader_app::effects::layout_prefs::layout_prefs;
use reader_app::effects::mode_change::mode_change;
use reader_app::effects::navigation_sync::navigation_sync;
use reader_app::effects::reflow_layout::reflow_layout;
use reader_app::effects::reflow_measure::install_reflow_measure;
use reader_app::effects::reflow_outline::reflow_outline;
use reader_app::effects::zoom_watchers::{fit_watcher, follow_watcher};
use reader_app::features::{ReaderRoot, use_reader_virtualizers};
use reader_app::zoom::ZoomController;

#[component]
pub fn ReaderRoute(state: AppState) -> impl IntoView {
    let vs = state.reader;

    // The shell's layout brain: one controller for the whole page, provided as
    // context for the title bar, the traffic lights, the floating label and
    // both rail mount points. It owns the open/close slide machine, so the
    // chrome stays aligned with the rail's pixels for the whole slide.
    let shell = ShellController::reader(state.settings, state.ui.sidebar, state.reader.viewer.motion.into());
    provide_context(shell);

    // The reader's persistence door: gloss marks save into the shell's
    // storage, keyed the way the open flow's load reads them back. Provided
    // before the reader surface mounts, so the gloss controller below finds
    // it in context.
    provide_context::<GlossSave>(Callback::new(
        move |(key, marks): (String, Vec<ai_core::gloss::GlossMark>)| {
            crate::storage::persist_gloss(&key, &marks);
        },
    ));

    let rv = use_reader_virtualizers(vs);

    // ── The reader's effects, in the one order that works ──────────────

    // The layout prefs (page gap, page margin) resolve their settings into the
    // strips' size models. Installed BEFORE the reflow layout effect below,
    // which reads the gap they resolve.
    layout_prefs(
        vs,
        state.settings,
        rv.virtualizer.clone(),
        rv.h_virtualizer.clone(),
    );

    // The paged text modes' A4 page model upkeep: page-unit sizes projected
    // into the shared measurement store whenever the format, the mode or the
    // cut moves, and reverted when they stop asking for it (the vertical
    // reading mode is the stream's, which virtualizes blocks directly).
    // Installed AFTER the gap effects so its relayout reads the gap they just
    // resolved.
    reflow_layout(vs, rv.virtualizer.clone());
    // The reflowable measurement pipeline: the pipe the stream's and the
    // page hosts' block measurements flow through into the page cut, and the
    // re-estimate that follows the typography and the width dials. Installed
    // beside the layout it feeds.
    install_reflow_measure(vs);
    // The Markdown outline follows the same page cut, so it is installed beside
    // it: one re-cut republishes the pages AND moves the chapters.
    reflow_outline(vs);

    // What a mode flip owes: the incoming strip's anchor, the stream's zoom, the
    // outgoing view's rasters, and the fit the next mode owns.
    mode_change(vs, state.settings);

    let actuator = reader_app::zoom::actuator::ZoomActuator::new(rv.virtualizer.clone(), rv.h_virtualizer.clone());
    // Driven once at setup. The controller itself is dropped when setup
    // returns; what outlives it are the effects `drive` installs, which live as
    // long as this page's reactive owner. Everything downstream only posts
    // commands; nothing else writes a zoom scale or rescales a strip.
    let zoom = ZoomController::new(actuator);
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
    follow_watcher(vs, state.ui.sidebar);
    fit_watcher(vs, state.settings);
    auto_scroll(vs);
    reading_progress(state);
    // The blend backdrop's geometry half: the viewport's ladder position per
    // scroll tick (the engine owns the colours it drives). The backdrop
    // carries no texture — each page's own ::before paints the gutter
    // (textures.css, BLEND BLEED) — so there is nothing here to sync, only the
    // colour position the engine consumes. The SETTINGS half lives at the app
    // root, ahead of the first document open.
    blend_backdrop(vs, state.settings);

    first_paint_gate(vs);

    let settings_open = RwSignal::new(false);
    // The settings modal is opened from several places (the 3-dash menu's
    // Settings… item, the sidebar header's gear) that sit under different
    // mount points, so the open signal is shared through context rather than
    // threaded as a prop through the rail composition.
    provide_context(settings_open);

    // Left: sidebar toggle + Library; title centered; right: the 3-dash view
    // menu + Appearance.
    //
    // The sidebar toggle's visibility is the controller's rule: overlay mode
    // drops it (the rail opens by brushing the window's left edge and closes
    // from its own header, so a second switch in the bar only competes with
    // both). The Library button stays put — the rail floats above the bar and
    // covers it while up, which is the rail's job. The cluster is always
    // mounted so the row keeps its left edge (and `#toolbar-leading`, the
    // measurement anchor the library title uses) wherever the mode puts it.
    // Reader settings have no button of their own: they open from the 3-dash
    // menu's Settings item and the sidebar header's gear.
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
                        pdf_engine::types::DocStatus::Ready | pdf_engine::types::DocStatus::Opening
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
    let center = move || {
        view! {
            <crate::components::shell::titlebar::document_title::CenteredDocTitle state=state />
        }
    };
    let right = move || {
        view! {
            <ReaderMenu state=state settings_open=settings_open />
            <AppearanceMenu state=state />
        }
    };

    view! {
        <crate::components::shell::titlebar::app_title_bar::AppTitleBar state=state left=left center=center right=right>
            // The document surface, from the reader crate: the rail, the
            // viewer slot, and everything that floats over the document.
            // The floating rail renders as its last element — outside
            // `.reader-bg`'s stacking context — and the modal below wins
            // their shared z-popover token by coming after it.
            <ReaderRoot
                reader=vs
                settings=state.settings
                covers=state.library.covers
                shell=shell
                virtualizer_view=rv.virtualizer_view
                h_virtualizer_view=rv.h_virtualizer_view
            />
            // The settings modal belongs to the window, not to the viewer: as a
            // child of `main` it sat inside `.reader-bg`, which is a stacking
            // context (position:relative + z-index:0), so the title bar's band
            // — a SIBLING of `.reader-bg` at z-bar — painted over the top of an
            // open modal. As a sibling of the page, its own z-popover token
            // outranks the bar, which is what a modal is supposed to do. It
            // also renders after the floating rail, so it wins their shared
            // z-popover token and still covers it.
            <SettingsModal state=state open=settings_open />
        </crate::components::shell::titlebar::app_title_bar::AppTitleBar>
    }
}
