//! The `/reader` route: sidebar + viewer slot + the app title bar, plus the
//! floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use leptos::html;
use leptos::prelude::*;


use pdf_engine::types::DocStatus;
use pdf_core::layout::{DocumentLayout, PAGE_GAP, ViewMode};
use crate::components::chrome::adaptive_toolbar::AdaptiveToolbar;
use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::tooltip::Tooltip;
use crate::components::panels::book_info::BookInfo;
use crate::components::panels::outline_host::SidebarOutline;
use crate::components::panels::panel_switcher::PanelSwitcher;
use crate::components::panels::sidebar_header::SidebarHeader;
use crate::components::panels::Sidebar;
use crate::components::panels::sidebar_shell::{
    request_reveal_active, sidebar_paint, SidebarChromeCtx,
};
use crate::components::panels::thumbnail_host::SidebarThumbs;
use crate::components::chrome::document_title::FloatingDocumentTitle;
use crate::components::chrome::app_title_bar::AppTitleBar;
use crate::components::chrome::document_title::DocumentTitle;
use crate::components::reader_controls::page_indicator::PageIndicator;
use crate::components::reader_controls::bottom_bar::ReaderBottomBar;
use crate::services::document::{close_document, open_dialog};
use crate::state::AppState;
use crate::effects::reader::reading_progress::reading_progress;
use crate::effects::reader::fit_mode::fit_effect;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::zoom::zoom_system;
use crate::state::SidebarMode;
use super::toolbar_entries::reader_toolbar_entries;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;
    // One cached column layout for the session. Borrow the heights so a
    // zoom frame doesn't clone the whole vector just to rebuild prefix sums.
    let layout = Memo::new(move |_| {
        vs.document
            .metrics
            .css_heights
            .with(|heights| DocumentLayout::new(heights, PAGE_GAP))
    });
    fit_effect(vs, state.ui.sidebar, layout);
    zoom_system(vs, layout);
    navigation_sync(vs, layout);
    reading_progress(state);

    let status = state.reader.document.status;
    let mode = state.reader.viewer.mode;
    let is_ready = move || status.get() == DocStatus::Ready;
    let paint = sidebar_paint(state.ui.sidebar);
    // Publish how far the open/close slide has progressed so the AppTitleBar
    // above can hold its left inset and native traffic lights through a close
    // instead of snapping them on the first frame.
    provide_context(SidebarChromeCtx {
        present: paint.present,
    });

    let appearance_open = RwSignal::new(false);
    let collapsed_ids = RwSignal::new(Vec::<&'static str>::new());
    let overflow_ref: NodeRef<html::Div> = NodeRef::new();

    // AdaptiveToolbar is generic UI: it gets a ready flag and a refresh
    // counter instead of AppState. The page bumps the counter whenever
    // chrome state that affects the bar's geometry changes.
    let toolbar_ready = Signal::derive(move || status.get() == DocStatus::Ready);
    let toolbar_refresh = RwSignal::new(0u64);
    Effect::new(move |_| {
        _ = state.ui.sidebar.get();
        _ = state.reader.document.status.get();
        _ = state.reader.document.num_pages.get();
        _ = state.reader.document.title.get();
        _ = state.reader.document.path.get();
        toolbar_refresh.update(|n| *n += 1);
    });

    // LEFT slot: sidebar toggle (only while the sidebar is closed — the
    // sidebar's own chrome row owns it otherwise), Library, Open, DocumentTitle.
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Show when=move || state.ui.sidebar.get() == SidebarMode::None>
                        <Tooltip text="Toggle sidebar">
                            <Button
                                on_click=move |_| state.ui.sidebar.set(SidebarMode::Thumbs)
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
                    <Tooltip text="Open PDF (Cmd/Ctrl+O)">
                        <Button
                            on_click=move |_| open_dialog(state)
                            variant=ButtonVariant::Toolbar
                            title="Open PDF (Cmd/Ctrl+O)"
                        >
                            <Icon name=IconName::Open size=18 />
                            <span>"Open"</span>
                        </Button>
                    </Tooltip>
                </div>
                <DocumentTitle state=state />
            </div>
        }
    };

    // RIGHT slot: view-mode, fit, zoom, appearance — collision-aware overflow.
    let right = move || {
        let entries = reader_toolbar_entries(
            state,
            appearance_open,
            collapsed_ids,
            overflow_ref,
        );
        view! {
            <AdaptiveToolbar
                ready=toolbar_ready
                refresh=toolbar_refresh.into()
                entries=entries
                collapsed_ids=collapsed_ids
                overflow_ref=overflow_ref
            />
        }
    };

    view! {
        <AppTitleBar state=state left=left right=right>
            // overflow-hidden clips the hidden ReaderBottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div class="relative flex h-full w-full flex-col overflow-hidden bg-paper text-ink">
                <div class="flex min-h-0 flex-1">
                    <Sidebar
                        mode=state.ui.sidebar
                        header=move || view! { <SidebarHeader reader=vs sidebar=state.ui.sidebar /> }
                        info_row=move || view! { <BookInfo reader=vs covers=state.library.covers /> }
                        panels=move || view! {
                            <SidebarOutline
                                state=vs
                                sidebar=state.ui.sidebar
                                shown=paint.show_outline
                                outro=paint.is_closed
                                intro=paint.intro
                            />
                            <SidebarThumbs
                                state=vs
                                sidebar=state.ui.sidebar
                                // Hold the cells back while the open slide is
                                // still animating: mounting 20–30 ThumbCells
                                // (each a pdf.js rasterisation) mid-slide is
                                // what made the first toggle lag. Once the
                                // 300ms window ends (paint.opening clears),
                                // the <For> mounts the window onto a settled
                                // layout. On later opens the thumb cache is
                                // warm, so the gate is harmless there too.
                                live=Signal::derive(move || {
                                    paint.thumbs_live.get() && !paint.opening.get()
                                })
                                shown=paint.show_thumbs
                                outro=paint.is_closed
                                intro=paint.intro
                            />
                        }
                        footer=move || view! {
                            <PanelSwitcher
                                mode=state.ui.sidebar
                                thumbs_active=paint.thumbs_active
                                outline_active=paint.outline_active
                                on_reveal=request_reveal_active
                            />
                        }
                    />
                    <main id="viewer-slot" class="relative min-w-0 flex-1 overflow-hidden">
                        <Show when=is_ready>
                            {move || match mode.get() {
                                ViewMode::Single => view! {
                                    <crate::components::document::SinglePageView state=vs />
                                }
                                .into_any(),
                                ViewMode::Continuous => view! {
                                    <crate::components::document::ContinuousView state=vs layout=layout />
                                }
                                .into_any(),
                            }}
                        </Show>
                        <FloatingDocumentTitle state=state />
                        // Corner page counter, gated on a ready document and
                        // positioned by the page; the indicator itself is
                        // reusable UI with no knowledge of AppState.
                        <Show when=is_ready>
                            <div class="pointer-events-none absolute bottom-3 right-3 z-30">
                                <PageIndicator current=state.reader.viewer.page total=state.reader.document.num_pages />
                            </div>
                        </Show>
                        <ReaderBottomBar reader=vs layout=layout />
                        <crate::components::search::floating_search::FloatingSearch state=vs />
                    </main>
                </div>
            </div>
        </AppTitleBar>
    }
}
