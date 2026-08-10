//! Top toolbar. OWNED BY branch B (viewer/chrome).
//! Redesigned (U2): hamburger + filename on the left, true viewport-centered
//! page nav (Single mode only). The right group is the U7 audit layout:
//! search + segmented Single/Continuous + zoom, then a single Appearance menu
//! (U6) and a More (⋯) overflow menu. The sidebar panel toggles were removed —
//! the sidebar's own tab rail is the single source of truth for which panel is
//! open.
//!
//! Also owns the shared file-open flow (`open_dialog` / `open_path`), reused by
//! the keyboard shortcuts (src/effects/shortcuts.rs) and later by drag-drop.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::components::atoms::button::{Button, ButtonKind};
use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::segmented::{Segmented, SegmentedLabel};
use crate::components::atoms::separator::Separator;
use crate::components::atoms::tooltip::Tooltip;
use crate::core::document::DocStatus;
use crate::core::layout::ViewMode;
use crate::core::math::{fit_scale, FitMode};
use crate::core::state::{AppState, SidebarMode, Toast, ToastKind};

use super::appearance_menu::AppearanceMenu;
use super::more_menu::MoreMenu;
use super::page_nav::PageNav;
use super::zoom_controls::ZoomControls;

/// Native open-dialog flow: pick a file, then run the shared open-flow.
///
/// Cancel ("Open cancelled") is a silent no-op; any other error surfaces on the
/// doc status / status bar.
pub fn open_dialog(state: AppState) {
    spawn_local(async move {
        match engine::pick_pdf().await {
            Ok(path) => open_path(state, path),
            Err(msg) => {
                if msg != "Open cancelled" {
                    state.doc.error.set(Some(msg.clone()));
                    state.doc.status.set(DocStatus::Error);
                    state.toast.set(Some(Toast {
                        kind: ToastKind::Error,
                        message: format!("Could not open PDF: {}", msg),
                    }));
                }
            }
        }
    });
}

/// Shared open-flow: open `path` in the engine and populate the whole app state
/// (document, viewer, search, persisted last_path). Drag-drop will call this
/// directly in a later phase.
pub fn open_path(state: AppState, path: String) {
    state.doc.status.set(DocStatus::Opening);
    state.doc.error.set(None);

    spawn_local(async move {
        match engine::open(&path).await {
            Ok(open) => {
                let page1 = open.page1_size;
                // Document state.
                state.doc.num_pages.set(open.num_pages);
                // Forget the previous document's measured heights
                // (PageList re-seeds them from page1_size on mount).
                state.doc.page_heights.set(Vec::new());
                state.doc.title.set(open.title);
                state.doc.author.set(open.author);
                state.doc.outline.set(open.outline);
                state.doc.page1_size.set(Some(page1.clone()));
                state.doc.path.set(Some(path.clone()));
                state.doc.error.set(None);
                state.doc.status.set(DocStatus::Ready);
                // A successful open dismisses any stale error toast.
                state.toast.set(None);

                // Viewer: back to page 1, fit width.
                state.viewer.page.set(1);
                state.viewer.scroll_top.set(0.0);
                state.viewer.fit.set(FitMode::Width);
                let (cw, ch) = state.viewer.container_size.get();
                let s =
                    fit_scale(FitMode::Width, cw, ch, page1.width, page1.height, 48.0, 1.0);
                state.viewer.scale.set(s);
                state.viewer.render_scale.set(s);

                // Reset search + clear stale highlights. The floating search
                // overlay must not linger after opening a new document.
                state.search.query.set(String::new());
                state.search.total.set(0);
                state.search.results.set(Vec::new());
                state.search.active.set(None);
                state.search.index_built.set(false);
                state.search.visible.set(false);
                engine::clear_highlights();

                // Fire index build in the background; result is ignored
                // (search effects call it too when needed).
                let _ = spawn_local(async move {
                    let _ = engine::build_search_index().await;
                });

                // Persist last path (the settings-watch effect writes
                // localStorage automatically).
                state.settings.update(|s| s.last_path = Some(path));
            }
            Err(e) => {
                state.doc.error.set(Some(e.message.clone()));
                state.doc.status.set(DocStatus::Error);
                state.toast.set(Some(Toast {
                    kind: ToastKind::Error,
                    message: format!("Could not open PDF: {}", e.message),
                }));
            }
        }
    });
}

#[component]
pub fn Toolbar(state: AppState) -> impl IntoView {
    let mode = state.viewer.mode;

    let open_state = state;
    let menu_state = state;
    let mode_state = state;
    let filename_state = state;

    view! {
        <div class="relative flex h-12 items-center gap-2 px-3">
            // LEFT GROUP: hamburger + Open + filename.
            <div class="flex min-w-0 items-center gap-1">
                <Tooltip text="Toggle sidebar".to_string()>
                    <Button
                        on_click=move |_| {
                            let next = if menu_state.sidebar.get() == SidebarMode::None {
                                SidebarMode::Outline
                            } else {
                                SidebarMode::None
                            };
                            menu_state.sidebar.set(next);
                        }
                        kind=ButtonKind::Ghost
                        icon=IconName::Menu
                        title="Toggle sidebar".to_string()
                    />
                </Tooltip>
                <Tooltip text="Open PDF (Cmd/Ctrl+O)".to_string()>
                    <Button
                        on_click=move |_| open_dialog(open_state)
                        kind=ButtonKind::Toolbar
                        icon=IconName::Open
                        label="Open".to_string()
                        title="Open PDF (Cmd/Ctrl+O)".to_string()
                    />
                </Tooltip>
                <span class="min-w-0 max-w-40 truncate text-sm text-ink">
                    {move || {
                        filename_state
                            .doc
                            .title
                            .get()
                            .or(filename_state.doc.path.get())
                            .unwrap_or_else(|| "No document".to_string())
                    }}
                </span>
            </div>

            // CENTER: absolutely positioned, TRUE viewport centering (Single
            // mode only; the self-sized wrapper stays out of the left/right
            // groups' way).
            <Show when=move || mode.get() == ViewMode::Single>
                <div class="absolute left-1/2 top-1/2 z-10 -translate-x-1/2 -translate-y-1/2">
                    <PageNav state=state.clone() />
                </div>
            </Show>

            // RIGHT GROUP.
            <div class="ml-auto flex items-center gap-1">
                // Floating-search toggle (U4): lets mouse-only users open search
                // between Phase 1 and Phase 3; Cmd/Ctrl+F does the same. A raw
                // button (not the Button atom) so pointerdown can stop
                // propagation: the floating bar's outside-click dismiss listens
                // on window pointerdown, which would otherwise close the bar and
                // then the click would re-open it — making the toggle one-way.
                <Tooltip text="Search (Cmd/Ctrl+F)".to_string()>
                    <button
                        type="button"
                        title="Search (Cmd/Ctrl+F)"
                        on:pointerdown=move |ev| ev.stop_propagation()
                        on:click=move |_| state.search.visible.set(!state.search.visible.get())
                        class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent text-ink transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                    >
                        <Icon name=IconName::Search size=16 />
                    </button>
                </Tooltip>
                <Tooltip text="View mode".to_string()>
                    <Segmented
                        options=vec![
                            (ViewMode::Single, SegmentedLabel::Icon(IconName::SinglePage)),
                            (ViewMode::Continuous, SegmentedLabel::Icon(IconName::Continuous)),
                        ]
                        value={mode.read_only()}
                        on_change=move |m: ViewMode| mode_state.viewer.mode.set(m)
                    />
                </Tooltip>
                <ZoomControls state=state.clone() />
                <Separator vertical=true />
                <AppearanceMenu state={state.clone()} />
                <MoreMenu state={state.clone()} />
            </div>
        </div>
    }
}
