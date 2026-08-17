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
use crate::core::filename::display_name;
use crate::core::library::{self, CoverImage, RecentBook};
use crate::core::math::{fit_scale, FitMode};
use crate::core::state::{AppState, SidebarMode, Toast, ToastKind};
use crate::util::storage::{save_covers, save_library};

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
/// (document, viewer, search, library). Resumes at the saved page if this book
/// was opened before, and records it in the recent-books library. Drag-drop
/// calls this directly.
pub fn open_path(state: AppState, path: String) {
    state.doc.status.set(DocStatus::Opening);
    state.doc.error.set(None);

    // The resume point is read BEFORE the open resolves so it can't be
    // clobbered by a concurrent page-tracking write from the closing document.
    let saved_page = library::find_page(&state.library.get_untracked(), &path).unwrap_or(1);

    spawn_local(async move {
        match engine::open(&path).await {
            Ok(open) => {
                let page1 = open.page1_size;
                let num_pages = open.num_pages;
                let name = display_name(open.title.as_deref(), Some(&path));
                // Document state.
                state.doc.num_pages.set(num_pages);
                // Intrinsic per-page sizes, straight from the engine. These
                // replace the previous document's measured column outright.
                state.doc.page_sizes.set(open.page_heights.clone());
                state.doc.page_widths.set(open.page_widths.clone());
                state.doc.title.set(open.title);
                state.doc.author.set(open.author);
                state.doc.outline.set(open.outline);
                state.doc.page1_size.set(Some(page1.clone()));
                state.doc.path.set(Some(path.clone()));
                state.doc.error.set(None);
                state.doc.status.set(DocStatus::Ready);
                // A successful open dismisses any stale error toast.
                state.toast.set(None);

                // Resume: jump to the saved page (clamped to the real count —
                // a re-edited document may have fewer pages than remembered).
                let resume = saved_page.min(num_pages.max(1));
                state.viewer.page.set(resume);
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
                state.search.matches.set(Vec::new());
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
                // localStorage automatically). Kept for schema stability; the
                // library below is the real "recent books" store.
                state.settings.update(|s| s.last_path = Some(path.clone()));

                // Record this book in the recent-books library (most-recent
                // first). An evicted entry past the cap has its cover dropped.
                let mut recent = state.library.get_untracked();
                let evicted = library::upsert(
                    &mut recent,
                    RecentBook {
                        path: path.clone(),
                        title: name,
                        page: resume,
                        num_pages,
                    },
                );
                state.library.set(recent);
                save_library(&state.library.get_untracked());
                if let Some(evicted_path) = evicted {
                    state.covers.update(|c| {
                        c.remove(&evicted_path);
                    });
                    save_covers(&state.covers.get_untracked());
                }

                // Generate the shelf cover (page-1 JPEG) in the background. It
                // is fire-and-forget: a failed render just leaves the stylised
                // fallback cover on the shelf.
                let cover_state = state;
                let cover_path = path.clone();
                let _ = spawn_local(async move {
                    match engine::cover_data_url(&cover_path, 240.0).await {
                        Ok(c) => {
                            cover_state.covers.update(|covers| {
                                covers.insert(
                                    cover_path,
                                    CoverImage {
                                        data_url: c.data_url,
                                        width: c.width,
                                        height: c.height,
                                    },
                                );
                            });
                            save_covers(&cover_state.covers.get_untracked());
                        }
                        Err(_) => { /* stylised fallback cover */ }
                    }
                });
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

/// Close the current document and return to the library shelf.
///
/// Tears the engine's document state down and resets the document / viewer /
/// search signals so the reader lands back on the empty-state bookshelf (which
/// renders whenever `doc.status != Ready`). The library itself is untouched:
/// the just-closed book keeps its saved page so reopening resumes there.
pub fn close_document(state: AppState) {
    // NOTE: the engine document is deliberately NOT destroyed here. `open()`
    // tears the previous document down as its first step, so a fast
    // "close → reopen another book" would race two destroys against the same
    // loading task. The viewer unmounting below releases every page canvas
    // anyway, and the next open (or app close) drops the pdf.js document.
    state.doc.status.set(DocStatus::Idle);
    state.doc.error.set(None);
    state.doc.path.set(None);
    state.doc.num_pages.set(0);
    state.doc.title.set(None);
    state.doc.author.set(None);
    state.doc.outline.set(Vec::new());
    state.doc.page1_size.set(None);
    state.doc.page_sizes.set(Vec::new());
    state.doc.page_widths.set(Vec::new());
    state.doc.page_heights.set(Vec::new());

    state.viewer.page.set(1);
    state.viewer.scroll_top.set(0.0);

    state.search.query.set(String::new());
    state.search.total.set(0);
    state.search.matches.set(Vec::new());
    state.search.active.set(None);
    state.search.index_built.set(false);
    state.search.visible.set(false);
    state.search.dismissed.set(false);

    state.sidebar.set(SidebarMode::None);
}
