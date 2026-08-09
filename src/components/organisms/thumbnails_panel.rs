//! Thumbnail grid. OWNED BY branch D (panels/settings).
//!
//! Renders a low-scale canvas for every page (no text layer) by talking to the
//! engine directly — `PageCanvas` always builds a text layer, thumbnails must
//! not. Clicking a thumbnail jumps to that page and closes the panel. The panel
//! self-cleans: it unregisters every thumbnail canvas when it unmounts (the
//! sidebar switches away), which keeps WKWebView memory in check.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::engine;
use crate::core::state::{AppState, SidebarMode};

/// Render scale for thumbnails (CSS px per PDF unit).
const THUMB_SCALE: f64 = 0.25;

/// 1-based page numbers for a document with `n` pages.
fn page_numbers(n: u32) -> Vec<u32> {
    (1..=n).collect()
}

#[component]
pub fn ThumbnailsPanel(state: AppState) -> impl IntoView {
    let num_pages = state.doc.num_pages;

    // Highest thumbnail canvas we've registered so far; used to unregister
    // stale canvases when the document shrinks and to clean up on unmount.
    // Arc<AtomicU32> (not Rc<Cell>) because `on_cleanup` requires Send + Sync.
    let registered_upto = Arc::new(AtomicU32::new(0));
    // Bumped on every render pass so in-flight passes from an older document
    // stop registering/rendering as soon as a newer pass starts.
    let generation = Arc::new(AtomicU32::new(0));

    {
        let upto = registered_upto.clone();
        on_cleanup(move || {
            for p in 1..=upto.load(Ordering::Relaxed) {
                engine::unregister_page(&format!("thumb-{p}"));
            }
        });
    }

    Effect::new(move || {
        let n = num_pages.get();
        if state.sidebar.get() != SidebarMode::Thumbs {
            // Defensive: normally this panel is unmounted when the sidebar
            // leaves Thumbs; if it isn't, unregister everything so no canvas
            // stays bound to the engine while hidden.
            generation.fetch_add(1, Ordering::Relaxed);
            for p in 1..=registered_upto.load(Ordering::Relaxed) {
                engine::unregister_page(&format!("thumb-{p}"));
            }
            registered_upto.store(0, Ordering::Relaxed);
            return;
        }
        if n == 0 {
            return;
        }

        // If the document shrank, drop the thumbnails past the new count.
        let prev = registered_upto.load(Ordering::Relaxed);
        if prev > n {
            for p in (n + 1)..=prev {
                engine::unregister_page(&format!("thumb-{p}"));
            }
            registered_upto.store(n, Ordering::Relaxed);
        }

        let gen = generation.load(Ordering::Relaxed) + 1;
        generation.store(gen, Ordering::Relaxed);
        let upto = registered_upto.clone();
        let gen_cell = generation.clone();

        spawn_local(async move {
            for p in 1..=n {
                if gen_cell.load(Ordering::Relaxed) != gen {
                    break; // a newer pass superseded us
                }
                let cid = format!("thumb-{p}");
                engine::register_page(p, &cid, None);
                upto.fetch_max(p, Ordering::Relaxed);
                if let Ok(r) = engine::render_page(&cid, THUMB_SCALE, false).await {
                    if let Some(canvas) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&cid))
                    {
                        // Cap the CSS width to the grid cell and let height follow
                        // the aspect ratio so wide pages don't overflow.
                        let _ = canvas.set_attribute(
                            "style",
                            &format!("width:{}px;max-width:100%;height:auto", r.width),
                        );
                    }
                }
            }
        });
    });

    view! {
        <div class="flex-1 overflow-y-auto p-3">
            <div class="grid grid-cols-2 gap-3">
                <For
                    each=move || page_numbers(num_pages.get())
                    key=|p| *p
                    children=move |p| {
                        let cid = format!("thumb-{p}");
                        let jump = p;
                        view! {
                            <button
                                type="button"
                                class="flex cursor-pointer flex-col items-center gap-1"
                                on:click=move |_| {
                                    state.viewer.page.set(jump);
                                    state.sidebar.set(SidebarMode::None);
                                }
                            >
                                <div class="w-full rounded-md border border-line bg-surface p-1">
                                    <canvas id=cid class="block" />
                                </div>
                                <span class="text-xs text-muted">{p}</span>
                            </button>
                        }
                    }
                />
            </div>
        </div>
    }
}
