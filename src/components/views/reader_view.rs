//! The `/reader` route: sidebar + viewer slot + the shared titlebar (via the
//! chrome `TitleBarProvider`), plus the floating doc title, page pill, bottom
//! bar and floating search. The viewer slot switches on viewer.mode.
//!
//! Slot wiring is the SINGLE coordinator's job — branches
//! must not edit this file.

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::segmented::{Segmented, SegmentedLabel};
use pdf_viewer::components::atoms::tooltip::Tooltip;

use pdf_engine::types::DocStatus;
use pdf_core::layout::ViewMode;
use pdf_core::math::FitMode;
use crate::components::chrome::adaptive_group::{AdaptiveGroup, ToolbarEntry};
use crate::components::chrome::floating_doc_title::FloatingDocTitle;
use crate::components::chrome::titlebar_provider::TitleBarProvider;
use crate::components::molecules::appearance_menu::appearance_entry;
use crate::components::molecules::doc_title::DocTitle;
use crate::components::molecules::zoom_controls::zoom_entries;
use crate::core::open_flow::{close_document, open_dialog};
use crate::core::state::AppState;
use crate::effects::reading_progress::reading_progress;
use pdf_viewer::effects::fit::fit_effect;
use pdf_viewer::effects::page_tracking::page_tracking;
use pdf_viewer::effects::zoom::zoom_system;
use pdf_viewer::state::SidebarMode;

fn view_mode_entry(state: AppState) -> ToolbarEntry {
    let mode = state.viewer.mode;

    // ── inline (what the bar shows) ──────────────────────────────
    // Compact icon-only segmented — same as before.
    let inline: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(move || {
        view! {
            <Tooltip text="View mode".to_string()>
                <Segmented
                    options=vec![
                        (ViewMode::Single,
                         SegmentedLabel::Icon(IconName::SinglePage),
                         "Single page view"),
                        (ViewMode::Continuous,
                         SegmentedLabel::Icon(IconName::Continuous),
                         "Continuous scroll view"),
                    ]
                    value={mode.read_only()}
                    on_change=move |m: ViewMode| state.viewer.mode.set(m)
                />
            </Tooltip>
        }
        .into_any()
    });

    ToolbarEntry {
        id: "view-mode",
        // collapses AFTER fit (70), BEFORE zoom-step (80)
        priority: 75,
        keep_mounted: false,
        inline: inline.clone(),
        sizer: inline,
        // Menu: full-width segmented WITH text labels; picking one closes the menu.
        collapsed: Arc::new(move |done| {
            view! {
                <div class="w-full px-1 py-1">
                    <Segmented
                        full_width=true
                        options=vec![
                            (ViewMode::Single,
                             SegmentedLabel::IconText(IconName::SinglePage, "Single"),
                             "Single page view"),
                            (ViewMode::Continuous,
                             SegmentedLabel::IconText(IconName::Continuous, "Continuous"),
                             "Continuous scroll view"),
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| {
                            state.viewer.mode.set(m);
                            done.run(());
                        }
                    />
                </div>
            }
            .into_any()
        }),
    }
}

fn fit_entry(state: AppState) -> ToolbarEntry {
    let inline: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(move || {
        view! {
            <div class="flex items-center gap-1">
                <Tooltip text="Fit width (Cmd/Ctrl+0)".to_string()>
                    <Button
                        on_click=move |_| state.viewer.fit.set(FitMode::Width)
                        kind=ButtonKind::Ghost
                        icon=IconName::FitWidth
                        title="Fit width (Cmd/Ctrl+0)".to_string()
                    />
                </Tooltip>
                <Tooltip text="Fit page".to_string()>
                    <Button
                        on_click=move |_| state.viewer.fit.set(FitMode::Page)
                        kind=ButtonKind::Ghost
                        icon=IconName::FitPage
                        title="Fit page".to_string()
                    />
                </Tooltip>
            </div>
        }
        .into_any()
    });

    ToolbarEntry {
        id: "fit",
        // collapses FIRST of the layout trio
        priority: 70,
        keep_mounted: false,
        inline: inline.clone(),
        sizer: inline,
        // Menu: two equal-half buttons; clicking closes the menu.
        collapsed: Arc::new(move |done| {
            view! {
                <div class="flex w-full items-center gap-1 px-1 py-1">
                    <button type="button"
                        on:click=move |_| {
                            state.viewer.fit.set(FitMode::Width);
                            done.run(());
                        }
                        class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-line bg-surface px-2 text-sm text-ink hover:bg-line"
                    >
                        <Icon name=IconName::FitWidth size=14 />
                        <span>"Fit width"</span>
                    </button>
                    <button type="button"
                        on:click=move |_| {
                            state.viewer.fit.set(FitMode::Page);
                            done.run(());
                        }
                        class="inline-flex h-9 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md border border-line bg-surface px-2 text-sm text-ink hover:bg-line"
                    >
                        <Icon name=IconName::FitPage size=14 />
                        <span>"Fit page"</span>
                    </button>
                </div>
            }
            .into_any()
        }),
    }
}

#[component]
pub fn ReaderView(state: AppState) -> impl IntoView {
    // The viewer slice of app state, handed to the reusable viewer components
    // and effects (all field paths match the app-level state).
    let vs = state.viewer_state();
    fit_effect(vs);
    zoom_system(vs);
    page_tracking(vs);
    reading_progress(state);

    let status = state.doc.status;
    let mode = state.viewer.mode;
    let state_sidebar = state;
    let is_ready = move || status.get() == DocStatus::Ready;

    let appearance_open = RwSignal::new(false);
    let collapsed_ids = RwSignal::new(Vec::<&'static str>::new());
    let overflow_ref: NodeRef<html::Div> = NodeRef::new();

    // LEFT slot: sidebar toggle (only while the sidebar is closed — the
    // sidebar's own chrome row owns it otherwise), Library, Open, DocTitle.
    let left = move || {
        view! {
            <div class="flex min-w-0 items-center gap-1">
                <div
                    id="toolbar-left-pre"
                    data-tauri-drag-region="true"
                    class="flex shrink-0 items-center gap-1"
                >
                    <Show when=move || state.sidebar.get() == SidebarMode::None>
                        <Tooltip text="Toggle sidebar".to_string()>
                            <Button
                                on_click=move |_| state.sidebar.set(SidebarMode::Thumbs)
                                kind=ButtonKind::Ghost
                                icon=IconName::Sidebar
                                title="Toggle sidebar".to_string()
                            />
                        </Tooltip>
                    </Show>
                    <Show when=move || {
                        matches!(
                            state.doc.status.get(),
                            DocStatus::Ready | DocStatus::Opening
                        )
                    }>
                        <Tooltip text="Library".to_string()>
                            <Button
                                on_click=move |_| close_document(state)
                                kind=ButtonKind::Ghost
                                icon=IconName::Library
                                title="Close this book and return to the library".to_string()
                            />
                        </Tooltip>
                    </Show>
                    <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                        <Button
                            on_click=move |_| open_dialog(state)
                            kind=ButtonKind::Toolbar
                            icon=IconName::Open
                            label="Open".to_string()
                            title="Open PDF (Cmd/Ctrl+O)".to_string()
                        />
                    </Tooltip>
                </div>
                <DocTitle state=state />
            </div>
        }
    };

    // RIGHT slot: view-mode, fit, zoom, appearance — collision-aware overflow.
    let right = move || {
        let mut entries = vec![view_mode_entry(state), fit_entry(state)];
        entries.extend(zoom_entries(state));          // zoom-step (80), readout (MAX)
        entries.push(appearance_entry(
            state,
            appearance_open,
            collapsed_ids,
            overflow_ref,
        ));                                          // MAX
        view! {
            <AdaptiveGroup
                state=state
                entries=entries
                collapsed_ids=collapsed_ids
                overflow_ref=overflow_ref
            />
        }
    };

    view! {
        <TitleBarProvider state=state left=left right=right>
            // overflow-hidden clips the hidden BottomBar's slide-down translate
            // so it can never leak a phantom scrollbar onto the window.
            <div class="relative flex h-full w-full flex-col overflow-hidden bg-paper text-ink">
                <div class="flex min-h-0 flex-1">
                    <crate::components::organisms::sidebar::Sidebar state=state_sidebar />
                    <main id="viewer-slot" class="relative min-w-0 flex-1 overflow-hidden">
                        <Show when=is_ready>
                            {move || match mode.get() {
                                ViewMode::Single => view! {
                                    <pdf_viewer::components::single_page_view::SinglePageView state=vs />
                                }
                                .into_any(),
                                ViewMode::Continuous => view! {
                                    <pdf_viewer::components::continuous_view::ContinuousView state=vs />
                                }
                                .into_any(),
                            }}
                        </Show>
                        <FloatingDocTitle state=state />
                        <crate::components::molecules::page_pill::PagePill state=state />
                        <crate::components::molecules::bottom_bar::BottomBar state=state />
                        // Floating search overlay (U4): mounted at the viewer
                        // slot; its top-14 offset clears the titlebar.
                        <pdf_viewer::components::floating_search::FloatingSearch state=vs />
                    </main>
                </div>
            </div>
        </TitleBarProvider>
    }
}
