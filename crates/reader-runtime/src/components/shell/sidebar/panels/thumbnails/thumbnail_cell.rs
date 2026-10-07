//! A single thumbnail cell owning its render lifecycle: request, cached
//! fast path, unmount cancellation.

// These cross into `Send + Sync` cleanup slots, so they stay `Arc`
// (with locks).
use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::state::ReaderState;
use app_chrome::hooks::use_timeout::use_timeout_slot;

use super::geometry::CELL_W;

/// Delay (ms) before the skeleton pulse is removed after a thumbnail render
/// resolves.
const PULSE_STOP_MS: u64 = 400;

/// Pages whose canvases are engine-bound: `Arc<Mutex<HashSet>>` (see the
/// top of this file).
pub type ThumbRegistry = Arc<Mutex<HashSet<u32>>>;

/// One cell's render lifecycle: a pure state machine, unit-testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThumbRenderState {
    /// Mounted, no render completed yet; a failure returns here.
    Pending,
    /// A `render_thumb` is in flight; no second render may start.
    Rendering,
    /// The engine painted this page; the cell never renders again.
    Settled,
    /// Unmounted (scrolled out, document switch, teardown): terminal.
    Unmounted,
}

/// How many times a cell may restart a render before it gives up.
const MAX_RENDER_ATTEMPTS: u8 = 3;

impl ThumbRenderState {
    /// A render starts only from `Pending`, while attempts remain.
    fn may_start(self, attempts: u8) -> bool {
        self == Self::Pending && attempts < MAX_RENDER_ATTEMPTS
    }

    fn start(self) -> Self {
        Self::Rendering
    }

    /// A successful paint is final; the cover crossfade runs, then stops.
    fn paint(self) -> Self {
        Self::Settled
    }

    /// A failed paint returns to `Pending` for the next sweep.
    fn fail(self) -> Self {
        Self::Pending
    }

    /// Unmounting wins over every other state, in-flight renders included.
    fn unmount(self) -> Self {
        Self::Unmounted
    }

    /// Atomic storage: one `AtomicU8` shared by the task and the cleanup.
    fn as_u8(self) -> u8 {
        self as u8
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Pending,
            1 => Self::Rendering,
            2 => Self::Settled,
            _ => Self::Unmounted,
        }
    }
}

/// One thumbnail cell: a blended canvas under a themed cover.
#[component]
pub fn ThumbCell(
    state: ReaderState,
    /// 1-based page number this cell renders.
    page: u32,
    /// Document generation guard: a stale render cannot paint a new document.
    generation: Arc<AtomicU32>,
    /// Pages whose canvases are engine-bound; the cell removes itself.
    bound: ThumbRegistry,
    /// Bumped by the panel's heal sweep: a lost race retries in place.
    #[prop(into)]
    heal: Signal<u64>,
    /// Set when a render fails: the panel's sweep drives healing.
    needs_heal: RwSignal<bool>,
) -> impl IntoView {
    // Cached bitmaps mount already loaded, so a re-entering row never
    // replays the skeleton crossfade.
    let thumbs = expect_context::<crate::frame_pane::thumbs::RemoteThumbs>();
    let starts_cached = thumbs.has(page);
    let req = Arc::new(AtomicU64::new(0));
    let loaded = RwSignal::new(starts_cached);
    // A NodeRef onto the cover; the pulse removal is parked in a scoped
    // timer slot.
    let cover_ref: NodeRef<html::Div> = NodeRef::new();
    let pulse_timer = use_timeout_slot();
    // The render slot and attempts are shared by the task and the cleanup.
    let render = Arc::new(AtomicU8::new(ThumbRenderState::Pending.as_u8()));
    let attempts = Arc::new(AtomicU8::new(0));
    // The current page drives the accent ring and badge.
    let is_current = move || state.viewer.page.get() == page;
    // The cell's own canvas by reference: an id lookup could find another
    // pane's twin.
    let canvas_ref: NodeRef<html::Canvas> = NodeRef::new();

    // Page-1 aspect drives the geometry; 3:4 portrait is the fallback.
    let aspect = move || state.document.page1_aspect();
    let cell_h = move || CELL_W * aspect();

    // Release the binding on unmount; the cached bitmap is kept, so a
    // return repaints instantly.
    let req_cleanup = req.clone();
    let page_cleanup = page;
    let bound_cleanup = bound.clone();
    let render_cleanup = render.clone();
    on_cleanup(move || {
        // Terminal: an in-flight task sees `Unmounted` when it wakes.
        let current = ThumbRenderState::from_u8(render_cleanup.load(Ordering::Relaxed));
        render_cleanup.store(current.unmount().as_u8(), Ordering::Relaxed);
        thumbs.cancel(req_cleanup.swap(0, Ordering::Relaxed));
        // WKWebView does not free a canvas backing store on DOM removal; zero
        // it on close.
        if let Some(cv) = canvas_ref.try_get_untracked().flatten() {
            cv.set_width(0);
            cv.set_height(0);
            // Symmetry with the engine's `releaseCanvas`: zero it and clear it.
            if let Ok(Some(ctx)) = cv.get_context("2d")
                && let Some(ctx2d) = ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>()
            {
                ctx2d.clear_rect(0.0, 0.0, 0.0, 0.0);
            }
        }
        if let Ok(mut guard) = bound_cleanup.lock() {
            guard.remove(&page_cleanup);
        }
    });

    // Render on mount and after a heal sweep; a prefetch may fill the cache
    // late.
    let doc_gen = generation.clone();
    let bound_render = bound.clone();
    let try_render = {
        let render = render.clone();
        let attempts = attempts.clone();
        move || {
            let slot = ThumbRenderState::from_u8(render.load(Ordering::Relaxed));
            if !slot.may_start(attempts.load(Ordering::Relaxed)) {
                return;
            }
            attempts.fetch_add(1, Ordering::Relaxed);
            render.store(slot.start().as_u8(), Ordering::Relaxed);
            let gen_now = doc_gen.load(Ordering::Relaxed);
            let req_async = req.clone();
            let gen_async = doc_gen.clone();
            let bound_async = bound_render.clone();
            let render_async = render.clone();
            spawn_local(async move {
                if let Ok(mut guard) = bound_async.lock() {
                    guard.insert(page);
                }
                let Some(canvas) = canvas_ref.get_untracked() else {
                    render_async.store(ThumbRenderState::Pending.as_u8(), Ordering::Relaxed);
                    return;
                };
                let result = thumbs.render(page, canvas, req_async).await;
                // `Unmounted` is terminal, and the generation guard bars
                // a stale paint.
                let slot_now = ThumbRenderState::from_u8(render_async.load(Ordering::Relaxed));
                if slot_now == ThumbRenderState::Unmounted
                    || gen_async.load(Ordering::Relaxed) != gen_now
                {
                    return;
                }
                match result {
                    Ok(_) => {
                        // A prefetch cache hit lands after mount;
                        // cached must not leave the cover opaque.
                        render_async.store(slot_now.paint().as_u8(), Ordering::Relaxed);
                        if !loaded.get_untracked() {
                            loaded.set(true);
                            if let Some(el) = cover_ref.get() {
                                _ = el.class_list().add_1("thumb-skeleton-settling");
                            }
                            let render_pulse = render_async.clone();
                            if let Ok(h) = set_timeout_with_handle(
                                move || {
                                    // The cleanup clears the handle;
                                    // leaked timers are also guarded.
                                    if render_pulse.load(Ordering::Relaxed)
                                        == ThumbRenderState::Unmounted.as_u8()
                                    {
                                        return;
                                    }
                                    if let Some(el) = cover_ref.get() {
                                        let _ = el.class_list().remove_1("thumb-skeleton-loading");
                                        let _ = el.class_list().remove_1("thumb-skeleton-settling");
                                    }
                                },
                                Duration::from_millis(PULSE_STOP_MS),
                            ) {
                                if let Some(prev) = pulse_timer.get_value() {
                                    prev.clear();
                                }
                                pulse_timer.set_value(Some(h));
                            }
                        }
                    }
                    Err(cancelled) => {
                        // A stale cancel can leave `loaded` set with
                        // no blit; put the cover back.
                        if loaded.get_untracked() {
                            loaded.set(false);
                        }
                        render_async.store(slot_now.fail().as_u8(), Ordering::Relaxed);
                        // Keep genuine errors visible; a stale
                        // cancel is not noise.
                        if !cancelled {
                            web_sys::console::warn_1(
                                &format!("[thumbnails] render page {page} failed").into(),
                            );
                        }
                        // Only a stale cell schedules a heal sweep.
                        needs_heal.set(true);
                    }
                }
            });
        }
    };
    // A new look or document makes every picture stale; a settled cell
    // renders again.
    let epoch = thumbs.epoch;
    let render_epoch = render.clone();
    let attempts_epoch = attempts.clone();
    Effect::new(move |previous: Option<u64>| {
        let now = epoch.get();
        if previous.is_some_and(|p| p != now) {
            let slot = ThumbRenderState::from_u8(render_epoch.load(Ordering::Relaxed));
            if slot == ThumbRenderState::Settled {
                render_epoch.store(ThumbRenderState::Pending.as_u8(), Ordering::Relaxed);
            }
            attempts_epoch.store(0, Ordering::Relaxed);
        }
        now
    });
    Effect::new(move |_| {
        _ = heal.get();
        _ = epoch.get();
        try_render();
    });

    view! {
        <button
            type="button"
            class="group flex w-full cursor-pointer flex-col items-center"
            // Jumping does NOT close the panel: thumbnail browsing is a loop.
            on:click=move |_| {
                state.viewer.page.set(page);
            }
        >
            // A themed backdrop under a blended canvas; the cover fades
            // OUT over it.
            <div
                class="thumb-card relative w-[120px] rounded-md"
                class=("ring-2", is_current)
                class=("ring-accent", is_current)
                class=("ring-1", move || !is_current())
                class=("ring-line", move || !is_current())
                style:height=move || format!("{}px", cell_h())
            >
                // The frame's bitmap at its own resolution; CSS sizes the card.
                <canvas
                    node_ref=canvas_ref
                    class="thumb-canvas absolute inset-0 block h-full w-full"
                    class=("thumb-canvas-blank", move || !loaded.get())
                />
                // The pulse stays DARKER than base through the fade; a
                // stop timer ends it.
                <div
                    node_ref=cover_ref
                    class="thumb-skeleton absolute inset-0 flex items-center justify-center pointer-events-none"
                    aria-hidden="true"
                    class=("thumb-skeleton-loading", move || !starts_cached)
                    class=("transition-opacity", move || !starts_cached)
                    class=("duration-300", move || !starts_cached)
                    class=("opacity-100", move || !loaded.get())
                    class=("opacity-0", move || loaded.get())
                />
                // Permanent page badge: stays at z-10 after the cover is gone.
                <div
                    class="thumb-num pointer-events-none absolute bottom-1.5 inset-x-0 z-10 flex justify-center"
                    class=("is-current", is_current)
                >
                    <span
                        class="rounded px-1.5 py-0.5 text-[11px] font-semibold tabular-nums shadow-sm transition-colors"
                        class=("bg-accent", is_current)
                        class=("text-white", is_current)
                        class=("bg-surface/90", move || !is_current())
                        class=("text-ink", move || !is_current())
                        class=("border", move || !is_current())
                        class=("border-line/60", move || !is_current())
                    >
                        {page}
                    </span>
                </div>
            </div>
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pending_cells_with_attempts_left_may_start() {
        assert!(ThumbRenderState::Pending.may_start(0));
        assert!(ThumbRenderState::Pending.may_start(2));
        assert!(!ThumbRenderState::Pending.may_start(MAX_RENDER_ATTEMPTS));
        // A render already in flight blocks a second one...
        assert!(!ThumbRenderState::Rendering.may_start(0));
        // ...and a painted or unmounted cell never renders again.
        assert!(!ThumbRenderState::Settled.may_start(0));
        assert!(!ThumbRenderState::Unmounted.may_start(0));
    }

    #[test]
    fn a_paint_is_final_a_failure_retries_and_unmount_is_terminal() {
        assert_eq!(
            ThumbRenderState::Pending.start(),
            ThumbRenderState::Rendering
        );
        assert_eq!(
            ThumbRenderState::Rendering.paint(),
            ThumbRenderState::Settled
        );
        assert_eq!(
            ThumbRenderState::Rendering.fail(),
            ThumbRenderState::Pending
        );
        // Unmounting wins over every state and is terminal.
        assert_eq!(
            ThumbRenderState::Pending.unmount(),
            ThumbRenderState::Unmounted
        );
        assert_eq!(
            ThumbRenderState::Rendering.unmount(),
            ThumbRenderState::Unmounted
        );
        assert_eq!(
            ThumbRenderState::Unmounted.unmount(),
            ThumbRenderState::Unmounted
        );
    }

    #[test]
    fn the_slot_round_trips_through_its_atomic_encoding() {
        for slot in [
            ThumbRenderState::Pending,
            ThumbRenderState::Rendering,
            ThumbRenderState::Settled,
            ThumbRenderState::Unmounted,
        ] {
            assert_eq!(ThumbRenderState::from_u8(slot.as_u8()), slot);
        }
    }
}
