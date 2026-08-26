//! The `/reader` route: sidebar + viewer slot + the app title bar, plus the
//! floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use leptos::html;
use leptos::prelude::*;
use virtual_list::Viewport;
use virtual_list_leptos::{VirtualizerOptions, use_virtualizer};

use super::toolbar_entries::reader_toolbar_entries;
use crate::components::primitives::hooks::dom::{TOOLBAR_LEADING_ID, VIEWER_SLOT_ID};
use crate::components::app_shell::adaptive_toolbar::AdaptiveToolbar;
use crate::components::app_shell::app_title_bar::AppTitleBar;
use crate::components::app_shell::document_title::DocumentTitle;
use crate::components::app_shell::floating_document_title::FloatingDocumentTitle;
use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::tooltip::Tooltip;
use crate::components::sidebar::Sidebar;
use crate::components::sidebar::document_info::BookInfo;
use crate::components::sidebar::header::SidebarHeader;
use crate::components::sidebar::outline_view::SidebarOutline;
use crate::components::sidebar::shell::{SidebarChromeCtx, request_reveal_active, sidebar_paint};
use crate::components::sidebar::switcher::PanelSwitcher;
use crate::components::sidebar::thumbnails_view::SidebarThumbs;
use crate::components::viewer_controls::bottom_bar::ReaderBottomBar;
use crate::components::viewer_controls::page_indicator::PageIndicator;
use crate::effects::reader::fit_mode::fit_effect;
use crate::effects::reader::navigation_sync::navigation_sync;
use crate::effects::reader::reading_progress::reading_progress;
use crate::effects::reader::zoom::zoom_system;
use crate::services::document::{close_document, open_dialog};
use crate::state::AppState;
use crate::state::SidebarMode;
use pdf_core::layout::{PAGE_GAP, RENDER_BUDGET, ViewMode};
use pdf_engine::types::DocStatus;

#[component]
pub fn ReaderPage(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.reader;

    // Keep `css_heights` fully seeded from intrinsic sizes: it is the shared
    // measurement store backing the virtualizer and zoom-rescale path, not a
    // second layout model.
    Effect::new(move || {
        let count = vs.document.num_pages.get() as usize;
        let scale = vs.viewer.zoom.render.get();
        let empty_intrinsic = vs.document.metrics.intrinsic.with(|sizes| sizes.is_empty());
        let fallback = vs
            .document
            .page1_size
            .get()
            .map(|size| size.height)
            .unwrap_or(0.0);
        if count == 0 || scale <= 0.0 || (empty_intrinsic && fallback <= 0.0) {
            return;
        }
        vs.document.metrics.css_heights.update(|heights| {
            if heights.len() == count {
                return;
            }
            *heights = vs.document.metrics.intrinsic.with(|sizes| {
                (0..count)
                    .map(|index| {
                        sizes
                            .get(index)
                            .map(|size| size.height)
                            .filter(|height| *height > 0.0)
                            .unwrap_or(fallback)
                            * scale
                    })
                    .collect()
            });
        });
    });

    let count = Signal::derive(move || vs.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        let measured = vs
            .document
            .metrics
            .css_heights
            .with_untracked(|heights| heights.get(index).copied());
        if let Some(height) = measured.filter(|height| *height > 0.0) {
            return height;
        }
        let intrinsic = vs
            .document
            .metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|size| size.height))
            .filter(|height| *height > 0.0);
        let fallback = vs
            .document
            .page1_size
            .get_untracked()
            .map(|size| size.height)
            .unwrap_or(0.0);
        intrinsic.unwrap_or(fallback) * vs.viewer.zoom.render.get_untracked()
    };
    let epoch = Signal::derive(move || {
        let count = vs.document.num_pages.get() as usize;
        let mut hasher = DefaultHasher::new();
        count.hash(&mut hasher);
        vs.document.metrics.intrinsic.with(|sizes| {
            sizes.len().hash(&mut hasher);
            for size in sizes {
                size.width.to_bits().hash(&mut hasher);
                size.height.to_bits().hash(&mut hasher);
            }
        });
        hasher.finish()
    });
    let pinned_sig: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);
    let initial_vh = {
        let (_, height) = vs.viewer.container_size.get_untracked();
        if height > 1.0 { height } else { 800.0 }
    };
    let virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, estimate)
            .gap(PAGE_GAP)
            .budget(RENDER_BUDGET)
            .initial(Viewport::main_only(initial_vh), 0.0)
            .pinned(pinned_sig.into())
            .epoch(epoch),
    );

    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            let mut pin = None;
            if vs.viewer.zoom_animating.get() {
                let dominant = v.dominant().get();
                pin = Some((dominant, dominant));
            }
            if let Some((first, last)) = vs.viewer.selected_pages.get() {
                let selected = (
                    first.saturating_sub(1) as usize,
                    last.saturating_sub(1) as usize,
                );
                pin = Some(match pin {
                    Some((a, b)) => (a.min(selected.0), b.max(selected.1)),
                    None => selected,
                });
            }
            pinned_sig.set(pin);
        });
    }

    fit_effect(vs, state.ui.sidebar, virtualizer.clone());
    zoom_system(vs, virtualizer.clone());
    navigation_sync(vs, virtualizer.clone());
    let virtualizer_view = StoredValue::new_local(virtualizer.clone());
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
                    id=TOOLBAR_LEADING_ID
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
        let entries = reader_toolbar_entries(state, appearance_open, collapsed_ids, overflow_ref);
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
                                // Cells mount with the aside. Cached thumbs
                                // blit during the slide; cold cells retain their
                                // skeleton until their capped render completes.
                                live=Signal::derive(move || paint.thumbs_live.get())
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
                    <main id=VIEWER_SLOT_ID class="relative min-w-0 flex-1 overflow-hidden">
                        <Show when=is_ready>
                            {move || match mode.get() {
                                ViewMode::Single => view! {
                                    <crate::components::document::SinglePageView state=vs />
                                }
                                .into_any(),
                                ViewMode::Continuous => view! {
                                    <crate::components::document::ContinuousView
                                        state=vs
                                        virtualizer=virtualizer_view.get_value()
                                    />
                                }
                                .into_any(),
                            }}
                        </Show>
                        <FloatingDocumentTitle state=state />
                        // Corner page counter, gated on a ready document and
                        // positioned by the page; the indicator itself is
                        // reusable UI with no knowledge of AppState.
                        <Show when=is_ready>
                            <div class=format!("pointer-events-none absolute bottom-3 right-3 {}", crate::components::primitives::floating::types::z::CONTROLS)>
                                <PageIndicator
                                    current=state.reader.viewer.page
                                    total=state.reader.document.num_pages
                                    hidden=Signal::derive(move || state.reader.gloss.selection_active.get())
                                />
                            </div>
                        </Show>
                        <ReaderBottomBar reader=vs virtualizer=virtualizer_view />
                        <crate::components::search::floating_search::FloatingSearch
                            state=vs
                            virtualizer=virtualizer_view
                        />
                        <crate::components::ai::selection_menu::SelectionMenu state=state />
                        <crate::components::ai::gloss::gloss_ai_popover::GlossAiPopover state=state />
                    </main>
                </div>
            </div>
        </AppTitleBar>
    }
}
