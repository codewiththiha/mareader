//! Top toolbar. OWNED BY branch B (viewer/chrome).
//! Redesigned (U2): hamburger + filename on the left, true viewport-centered
//! page nav (Single mode only), zoom + theme/texture/noise on the right. The
//! sidebar panel toggles were removed — the sidebar's own tab rail is the single
//! source of truth for which panel is open.
//!
//! Also owns the shared file-open flow (`open_dialog` / `open_path`), reused by
//! the keyboard shortcuts (src/effects/shortcuts.rs) and later by drag-drop.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::components::atoms::button::{Button, ButtonKind};
use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::separator::Separator;
use crate::components::atoms::tooltip::Tooltip;
use crate::core::document::DocStatus;
use crate::core::layout::ViewMode;
use crate::core::math::{fit_scale, FitMode};
use crate::core::state::{AppState, SidebarMode};

use super::noise_toggle::NoiseToggle;
use super::page_nav::PageNav;
use super::texture_menu::TextureMenu;
use super::theme_menu::ThemeMenu;
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
                    state.doc.error.set(Some(msg));
                    state.doc.status.set(DocStatus::Error);
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
                state.doc.error.set(Some(e.message));
                state.doc.status.set(DocStatus::Error);
            }
        }
    });
}

/// Small toolbar icon-button whose active styling is reactive (the Button atom's
/// `active`/`disabled` props are static, so raw elements are used here).
#[component]
fn ToggleButton(
    icon: IconName,
    title: String,
    active: impl Fn() -> bool + Send + 'static,
    on_click: impl Fn() + Send + 'static,
) -> impl IntoView {
    let base = "inline-flex h-9 items-center justify-center gap-1.5 rounded-lg border px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
    view! {
        <button
            type="button"
            title=title
            class=move || {
                if active() {
                    format!("{base} border-accent bg-accent-soft text-accent")
                } else {
                    format!("{base} border-line bg-surface text-ink hover:bg-line")
                }
            }
            on:click=move |_| on_click()
        >
            <Icon name=icon size=16 />
        </button>
    }
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
                <ZoomControls state=state.clone() />
                <Separator vertical=true />
                <ThemeMenu state=state.clone() />
                <TextureMenu state=state.clone() />
                <NoiseToggle state=state.clone() />
                <Separator vertical=true />
                <Tooltip text="Single page view".to_string()>
                    <ToggleButton
                        icon=IconName::SinglePage
                        title="Single page view".to_string()
                        active=move || mode.get() == ViewMode::Single
                        on_click=move || mode_state.viewer.mode.set(ViewMode::Single)
                    />
                </Tooltip>
                <Tooltip text="Continuous scroll".to_string()>
                    <ToggleButton
                        icon=IconName::Continuous
                        title="Continuous scroll".to_string()
                        active=move || mode.get() == ViewMode::Continuous
                        on_click=move || mode_state.viewer.mode.set(ViewMode::Continuous)
                    />
                </Tooltip>
            </div>
        </div>
    }
}
