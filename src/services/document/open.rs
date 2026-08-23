//! Opening a document: the dialog flow, the OS "Open with" handoff, and the
//! shared open sequence (engine open → document/viewer/search state →
//! library record → cover generation). Driven by the toolbar button,
//! Ctrl+O, drag-and-drop and the empty-state placeholder.

use leptos::prelude::*;
// NOTE: the open flow spawns on the wasm-bindgen-futures executor, NOT
// `leptos::task::spawn_local`. The latter ties the future to the reactive
// owner it is spawned under, so an open initiated from a book-card click would
// be CANCELLED the moment `status` flips to `Opening` (that unmounts the card,
// disposing its owner) — leaving the app stuck on "Opening…" forever. The
// wasm-bindgen executor runs the future to completion regardless of owner.
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::Event;

use pdf_engine::api as engine;
use pdf_engine::types::{DocStatus, PageSize};
use pdf_core::filename::display_name;
use crate::state::library::{self, CoverImage, RecentBook};
use pdf_core::math::{fit_scale, FitMode};
use crate::state::{AppState, Toast};
use crate::storage::{save_covers, save_library};

/// Wire OS-level file opening (double-click / "Open with" / default-app
/// launch) into the shared open flow. Called once from the app root.
///
/// Two paths, one handoff point:
///   * PULL — `take_pending_file` collects whatever the OS handed to the
///     backend before the webview finished mounting (initial-launch argv on
///     Windows/Linux, the macOS open-file event at launch). An event emitted
///     before mount would be lost, so the command is the source of truth.
///   * PUSH — the backend emits `pdf-open-file` for files opened while the
///     app is already running (single-instance forward on Windows/Linux,
///     LaunchServices on macOS). The listener just re-runs the pull: the
///     command clears itself, so an event + a stray second pull can never
///     open the same file twice.
pub fn init_open_file_handling(state: AppState) {
    let st = state;
    spawn_local(async move {
        if let Some(path) = engine::take_pending_file().await {
            open_path(st, path);
        }
    });

    if !pdf_engine::has_tauri() {
        return;
    }

    // Park the JS closure in a StoredValue so the listener stays registered
    // for the lifetime of the app (same pattern as the drag-drop listeners in
    // effects/drag_drop.rs; the unlisten handle is deliberately discarded).
    let handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);
    let cb_state = state;
    let cb: Closure<dyn FnMut(Event)> = Closure::wrap(
        Box::new(move |_ev: Event| {
            let st = cb_state;
            spawn_local(async move {
                if let Some(path) = engine::take_pending_file().await {
                    open_path(st, path);
                }
            });
        }) as Box<dyn FnMut(Event)>,
    );
    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        _ = pdf_engine::listen("pdf-open-file", f).await;
    });
    handle.set_value(Some(cb));
}

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
                    state.reader.document.error.set(Some(msg.clone()));
                    state.reader.document.status.set(DocStatus::Error);
                    state.ui.toast.set(Some(Toast {
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
    state.reader.document.status.set(DocStatus::Opening);
    state.reader.document.error.set(None);

    // The resume point is read BEFORE the open resolves so it can't be
    // clobbered by a concurrent page-tracking write from the closing document.
    let saved_page = state
        .library
        .books
        .with_untracked(|books| library::find_page(books, &path))
        .unwrap_or(1);

    spawn_local(async move {
        match engine::open(&path).await {
            Ok(open) => {
                let page1 = open.page1_size;
                let num_pages = open.num_pages;
                let name = display_name(open.title.as_deref(), Some(&path));
                // Document state.
                state.reader.document.num_pages.set(num_pages);
                // Intrinsic per-page sizes, packed as one `PageSize` each.
                let n = num_pages as usize;
                let intrinsic: Vec<PageSize> = if open.page_widths.len() == n
                    && open.page_heights.len() == n
                {
                    open.page_widths
                        .iter()
                        .zip(open.page_heights.iter())
                        .map(|(&width, &height)| PageSize { width, height })
                        .collect()
                } else {
                    vec![page1.clone(); n]
                };
                state.reader.document.metrics.intrinsic.set(intrinsic);
                state.reader.document.title.set(open.title);
                state.reader.document.author.set(open.author);
                state.reader.document.outline.set(open.outline);
                state.reader.document.page1_size.set(Some(page1.clone()));
                state.reader.document.path.set(Some(path.clone()));
                state.reader.document.error.set(None);
                state.reader.document.status.set(DocStatus::Ready);
                // A successful open dismisses any stale error toast.
                state.ui.toast.set(None);

                // Resume point (clamped to the real count — a re-edited
                // document may have fewer pages than remembered).
                let resume = saved_page.min(num_pages.max(1));

                // Fresh-open baseline: page 1, top of the column. The resume
                // jump happens AFTER the view mounts (see below), because
                // writing `page = resume` here — in the same batch as the
                // `page_heights` reset and `scroll_top = 0` — races the
                // page-tracking effects: the scroll→page effect reads scroll 0
                // and "corrects" the page back to 1 before the jump lands.
                state.reader.viewer.page.set(1);
                state.reader.viewer.scroll_top.set(0.0);
                state.reader.viewer.fit.set(FitMode::Width);
                // Heights belong to the document that was just closed; leaving
                // them would have the zoom coordinator anchor against a stale
                // column on the first gesture. PageList re-seeds them from
                // `page_sizes` (intrinsic heights) at the current scale.
                state.reader.document.metrics.css_heights.set(Vec::new());
                let (cw, ch) = state.reader.viewer.container_size.get();
                let s =
                    fit_scale(FitMode::Width, cw, ch, page1.width, page1.height, 48.0, 1.0);
                // Direct writes are correct HERE and nowhere else: this is the
                // initial scale for a brand-new document, so there is no layout
                // to animate from and nothing to anchor to. All three scales
                // must start in agreement.
                state.reader.viewer.zoom_animating.set(false);
                state.reader.viewer.zoom.request.set(None);
                state.reader.viewer.zoom.scale.set(s);
                state.reader.viewer.zoom.display.set(s);
                state.reader.viewer.zoom.render.set(s);

                // Jump to the saved page once the view has mounted and seeded
                // its page heights — the same `page.set()` path outline /
                // thumbnail / search navigation use, which is proven correct.
                if resume > 1 {
                    let jump_state = state;
                    request_animation_frame(move || {
                        jump_state.reader.viewer.page.set(resume);
                    });
                }

                // Reset search + clear stale highlights. The floating search
                // overlay must not linger after opening a new document.
                state.reader.search.reset();
                engine::clear_highlights();

                // Fire index build in the background; result is ignored
                // (search effects call it too when needed).
                spawn_local(async move {
                    _ = engine::build_search_index().await;
                });

                // Persist last path (the settings-watch effect writes
                // localStorage automatically). Kept for schema stability; the
                // library below is the real "recent books" store.
                state.settings.update(|s| s.last_path = Some(path.clone()));

                // Record this book in the recent-books library (most-recent
                // first). An evicted entry past the cap has its cover dropped.
                let mut recent = state.library.books.get_untracked();
                let evicted = library::upsert(
                    &mut recent,
                    RecentBook {
                        path: path.clone(),
                        title: name,
                        page: resume,
                        num_pages,
                    },
                );
                state.library.books.set(recent);
                if let Err(e) = save_library(&state.library.books.get_untracked()) {
                    e.report();
                }
                if let Some(evicted_path) = evicted {
                    state.library.covers.update(|c| {
                        c.remove(&evicted_path);
                    });
                    if let Err(e) = save_covers(&state.library.covers.get_untracked()) {
                        e.report();
                    }
                }

                // Generate the shelf cover (page-1 JPEG) in the background. It
                // is fire-and-forget: a failed render just leaves the stylised
                // fallback cover on the shelf.
                let cover_state = state;
                let cover_path = path.clone();
                spawn_local(async move {
                    match engine::cover_data_url(&cover_path, 240.0).await {
                        Ok(c) => {
                            cover_state.library.covers.update(|covers| {
                                covers.insert(
                                    cover_path,
                                    CoverImage {
                                        data_url: c.data_url,
                                        width: c.width,
                                        height: c.height,
                                    },
                                );
                            });
                            if let Err(e) = save_covers(&cover_state.library.covers.get_untracked()) {
                                e.report();
                            }
                        }
                        Err(_) => { /* stylised fallback cover */ }
                    }
                });

                // Pre-warm the thumbnail cache so the FIRST sidebar open is
                // all cache blits instead of 20+ concurrent pdf.js renders
                // fighting the width animation (same call the auto-center
                // idle prefetch uses). Deferred ~600ms so the reader has
                // settled first (fit_effect, first paint, cover render);
                // sequential awaits keep the engine queue from bursting.
                // 0.25 mirrors THUMB_SCALE (panels/thumbnails/geometry.rs).
                let warm_state = state;
                _ = set_timeout_with_handle(
                    move || {
                        spawn_local(async move {
                            let n = warm_state.reader.document.num_pages.get_untracked();
                            let pages = n.min(16); // first two grid rows + buffer
                            for p in 1..=pages {
                                engine::prefetch_thumb(p, 0.25).await;
                            }
                        });
                    },
                    std::time::Duration::from_millis(600),
                );
            }
            Err(e) => {
                state.reader.document.error.set(Some(e.message.clone()));
                state.reader.document.status.set(DocStatus::Error);
                state.ui.toast.set(Some(Toast {
                    message: format!("Could not open PDF: {}", e.message),
                }));
            }
        }
    });
}
