//! The shared "open a document" flow.
//!
//! Lifted out of `components::molecules::toolbar`, which is a piece of chrome
//! and had no business owning it: the same flow is driven by the toolbar
//! button, the Ctrl+O shortcut, drag-and-drop and the empty-state placeholder.
//! Living in `core` it is reachable from all of them without any of them
//! depending on the toolbar widget.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::core::document::DocStatus;
use crate::core::math::{fit_scale, FitMode};
use crate::core::state::{AppState, Toast, ToastKind};

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
                // Intrinsic per-page heights, straight from the engine. These
                // replace the previous document's measured column outright.
                state.doc.page_sizes.set(open.page_heights.clone());
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
                // Heights belong to the document that was just closed; leaving
                // them would have the zoom coordinator anchor against a stale
                // column on the first gesture. PageList re-seeds them from
                // `page_sizes` (intrinsic heights) at the current scale.
                state.doc.page_heights.set(Vec::new());
                let (cw, ch) = state.viewer.container_size.get();
                let s =
                    fit_scale(FitMode::Width, cw, ch, page1.width, page1.height, 48.0, 1.0);
                // Direct writes are correct HERE and nowhere else: this is the
                // initial scale for a brand-new document, so there is no layout
                // to animate from and nothing to anchor to. All three scales
                // must start in agreement.
                state.viewer.zoom_animating.set(false);
                state.viewer.zoom_request.set(None);
                state.viewer.scale.set(s);
                state.viewer.display_scale.set(s);
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
