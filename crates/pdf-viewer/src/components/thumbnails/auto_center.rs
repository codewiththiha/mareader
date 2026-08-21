//! Auto-center the current page in the thumbnail grid: the glide / grace /
//! debounce machinery, split out of `thumbnails/panel.rs`.
//!
//! Two effects live here:
//!   - the "take me to where I am" listener (`pdfreader:reveal-active`),
//!   - the debounced, grace-aware glide that follows the reader's page.
//!
//! Both read the panel-lifetime handles bundled in [`AutoCenter`]; the panel
//! constructs one and passes it in. Pure move from `panel.rs` — no behavior
//! change.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

use crate::state::{ViewerState, SidebarMode};

use super::geometry::{
    row_count, row_height, CELL_W, GLIDE_DEBOUNCE_MS, GRACE_MS, PAD,
};

type RevealSlot =
    std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<dyn Fn()>>>>;

/// Panel-lifetime state shared between the thumbnail panel's own effects and
/// the auto-center machinery. Constructed once per panel mount; the panel
/// keeps the handles it needs (drive listeners, geometry) and hands the rest
/// to [`AutoCenter::install`].
pub struct AutoCenter {
    /// Last time the USER physically scrolled/dragged the thumb panel. The
    /// auto-center effect yields to it for a grace period so the grid never
    /// fights someone who is browsing the thumbs themselves. NEG_INFINITY = the
    /// user has never driven it (auto-center always allowed).
    pub last_user_drive: Rc<Cell<f64>>,
    /// (was-this-panel-open, last-centered page) — auto-center only acts on a
    /// panel open or a real page change, never on churn. Kept in a StoredValue
    /// so reading/writing it never registers a reactive dependency.
    pub centered: StoredValue<(bool, u32), LocalStorage>,
    /// Handle for the debounced auto-center glide. Parked in a StoredValue so
    /// the effect can cancel a pending glide when it re-runs or the panel
    /// closes, and a fired glide that finds the user-drive grace still active
    /// can re-arm itself.
    pub glide_timer: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    /// The self-re-arming glide step lives in an `Rc` parked here (component-
    /// scoped, not an effect-run local) so a fired step can upgrade its Weak
    /// back-reference and re-arm itself while the user is still driving the
    /// grid. The step is replaced on every effect run; on_cleanup cancels the
    /// pending timer, so the old step is dropped with its timer.
    pub glide_slot: StoredValue<Option<RevealSlot>, LocalStorage>,
    /// The panel's scroll container element (shared with the size-tracking
    /// effect in the panel, which also writes it).
    pub container_el: StoredValue<Option<web_sys::Element>, LocalStorage>,
    /// The panel's viewport-height signal (seeded with `MIN_VIEWPORT_H`,
    /// tightened by the panel's mount effect + ResizeObserver).
    pub viewport_h: RwSignal<f64>,
}

impl AutoCenter {
    /// Create the bundle with fresh handles.
    pub fn new(
        container_el: StoredValue<Option<web_sys::Element>, LocalStorage>,
        viewport_h: RwSignal<f64>,
    ) -> Self {
        Self {
            last_user_drive: Rc::new(Cell::new(f64::NEG_INFINITY)),
            centered: StoredValue::new_local((false, 0u32)),
            glide_timer: StoredValue::new_local(None::<TimeoutHandle>),
            glide_slot: StoredValue::new_local(None::<RevealSlot>),
            container_el,
            viewport_h,
        }
    }

    /// Install the two auto-center effects. Must be called once from the
    /// panel, alongside the mount/size-tracking effect. Consumes the bundle:
    /// clone any handle you still need (`last_user_drive`) before calling.
    pub fn install(self, state: ViewerState) {
        let auto = self;

        // --- "Take me to where I am": re-clicking the active Thumbs tab. ---
        //
        // The auto-centre effect below already follows the reader, but it
        // deliberately yields to the user-drive grace — if you have been
        // scrolling the grid yourself it will not yank the view back. That is
        // right for passive following and wrong for an explicit request, so
        // this clears the grace timestamp and centres immediately.
        {
            let reveal_drive = auto.last_user_drive.clone();
            let container_el = auto.container_el;
            Effect::new(move |_| {
                let reveal_drive = reveal_drive.clone();
                let handle = window_event_listener(
                    leptos::ev::Custom::new("pdfreader:reveal-active"),
                    move |_: web_sys::CustomEvent| {
                        if state.sidebar.get_untracked() != SidebarMode::Thumbs {
                            return;
                        }
                        let Some(el) = container_el.get_value() else { return };
                        let vh = el.client_height() as f64;
                        let rh = row_height(aspect(state));
                        let total_rows = rows(state);
                        if vh <= 0.0 || rh <= 0.0 || total_rows == 0 {
                            return;
                        }
                        // Clear the grace so the request is honoured even if the
                        // reader was just scrolling the grid — that IS how they got
                        // lost, so refusing to move would defeat the gesture.
                        reveal_drive.set(f64::NEG_INFINITY);

                        let p = state.viewer.page.get_untracked();
                        let row = (p.saturating_sub(1) / 2) as f64;
                        let cell_h = CELL_W * aspect(state);
                        let cell_center_y = PAD + row * rh + cell_h / 2.0;
                        let max_scroll = (PAD * 2.0 + total_rows as f64 * rh - vh).max(0.0);
                        let target = (cell_center_y - vh / 2.0).clamp(0.0, max_scroll);

                        let opts = web_sys::ScrollToOptions::new();
                        opts.set_top(target);
                        opts.set_behavior(web_sys::ScrollBehavior::Smooth);
                        el.scroll_to_with_scroll_to_options(&opts);
                    },
                );
                on_cleanup(move || handle.remove());
            });
        }

        // The glide is DEBOUNCED and grace-aware. In continuous mode the reader
        // writes `viewer.page` at every row boundary, so an immediate smooth
        // scroll per write would keep re-starting the in-flight glide — the panel
        // would vibrate behind the reader and churn the virtualization window.
        // Every run therefore cancels the previous glide and re-arms it (the same
        // set_timeout_with_handle + on_cleanup pattern as effects/fit.rs), so it
        // fires ONCE, GLIDE_DEBOUNCE_MS after page writes settle. Opening the
        // panel is exempt — it is a one-shot, not churn, so it snaps next tick
        // instead of waiting out the debounce. A page change that lands inside
        // the user-drive grace is NOT dropped: the timer waits out the remaining
        // grace and, if the user is still driving when it fires, re-checks and
        // defers again (re-arming itself) — so the skipped page still ends up
        // centered once the panel (and the reader) have been still for the window.
        Effect::new(move |_| {
            let in_thumbs = state.sidebar.get() == SidebarMode::Thumbs;
            let p = state.viewer.page.get();
            let vh = auto.viewport_h.get();
            let rh = row_height(aspect(state));
            let cell_h = CELL_W * aspect(state);
            let total_rows = rows(state);

            let (was_open, _prev_p) = auto.centered.get_value();
            if !in_thumbs {
                auto.centered.set_value((false, 0));
                return;
            }
            // Element/geometry not ready yet (fresh mount): stay "unopened" so the
            // first run with real geometry counts as the panel just opening.
            let Some(el) = auto.container_el.get_value() else {
                return;
            };
            if vh <= 0.0 || rh <= 0.0 || total_rows == 0 {
                return;
            }

            let just_opened = !was_open;
            // Record the page this run intends to center. Kept honest even while a
            // grace/debounce defers the actual scroll, so a page change that lands
            // inside the grace is remembered instead of permanently skipped.
            auto.centered.set_value((true, p));

            // Row containing page p (2 columns per row, 0-based).
            let row = (p.saturating_sub(1) / 2) as f64;
            let cell_center_y = PAD + row * rh + cell_h / 2.0;
            let max_scroll = (PAD * 2.0 + total_rows as f64 * rh - vh).max(0.0);
            let target = (cell_center_y - vh / 2.0).clamp(0.0, max_scroll);

            let cur = el.scroll_top() as f64;
            if (target - cur).abs() <= 1.0 {
                // Already centered: cancel any pending glide (it targets an older
                // geometry/page) and stop.
                if let Some(h) = auto.glide_timer.get_value() {
                    h.clear();
                    auto.glide_timer.set_value(None);
                }
                return;
            }
            // User is browsing the thumb grid themselves -> don't yank (GRACE_MS).
            // The deferred glide below waits out the grace and re-checks at fire
            // time, so a page turned inside the grace still gets centered after.
            let in_grace = !just_opened && js_sys::Date::now() - auto.last_user_drive.get() < GRACE_MS;

            // Instant (explicitly — NOT Auto, which would defer to the element's
            // CSS scroll-behavior) on panel open or far jumps; smooth for nearby
            // page turns.
            let behavior = if just_opened || (target - cur).abs() > 2.0 * vh {
                web_sys::ScrollBehavior::Instant
            } else {
                web_sys::ScrollBehavior::Smooth
            };

            // Self-re-arming glide step: performs the scroll once the grace has
            // fully lapsed and the panel is still showing thumbs; while the user
            // keeps driving, it defers by re-checking after the grace lapses. The
            // step's back-reference to its own holder is a Weak, and the holder's
            // strong `Rc` lives in the component-scoped `glide_slot` StoredValue —
            // NOT an effect-run local (that would drop when this callback returns,
            // permanently breaking the upgrade). So a fired step can always find
            // itself and re-arm; the deferral survives the effect callback.
            let step_slot: RevealSlot = Rc::new(RefCell::new(None));
            let step_self = Rc::downgrade(&step_slot);
            let step_state = state;
            let step_el = el;
            let step_drive = auto.last_user_drive.clone();
            let step_timer = auto.glide_timer;
            let step_page = p;
            let step: Rc<dyn Fn()> = Rc::new(move || {
                let now = js_sys::Date::now();
                let elapsed = now - step_drive.get();
                let in_thumbs_now = step_state.sidebar.get_untracked() == SidebarMode::Thumbs;
                let page_now = step_state.viewer.page.get_untracked();
                let cur_now = step_el.scroll_top() as f64;
                // The world moved on since the glide was armed (panel closed, the
                // reader turned past this page, or the row is already centered):
                // drop the deferred glide.
                if !in_thumbs_now || page_now != step_page || (target - cur_now).abs() <= 1.0 {
                    step_timer.set_value(None);
                    return;
                }
                if elapsed < GRACE_MS {
                    // User still driving the grid — re-check once the grace lapses.
                    let next = step_self.upgrade().and_then(|slot| slot.borrow().clone());
                    let h = next.and_then(|next| {
                        set_timeout_with_handle(
                            move || next(),
                            Duration::from_millis((GRACE_MS - elapsed + 50.0) as u64),
                        )
                        .ok()
                    });
                    step_timer.set_value(h);
                    return;
                }
                let opts = web_sys::ScrollToOptions::new();
                opts.set_top(target);
                opts.set_behavior(behavior);
                step_el.scroll_to_with_scroll_to_options(&opts);
                step_timer.set_value(None);

                // Idle prefetch: warm the thumbnail cache around the reader's
                // current page so a later grid fling mounts every cell as a
                // synchronous cache blit (no skeleton, no waiting). Best-effort.
                let prefetch_page = step_page;
                leptos::task::spawn_local(async move {
                    for p in prefetch_page.saturating_sub(2)..=prefetch_page + 8 {
                        pdf_engine::api::prefetch_thumb(p, super::geometry::THUMB_SCALE).await;
                    }
                });
            });
            *step_slot.borrow_mut() = Some(step.clone());
            auto.glide_slot.set_value(Some(step_slot));

            // Debounce: cancel any pending glide and re-arm. Sustained page writes
            // keep re-running this effect, so each run clears the previous timer
            // and re-arms — exactly one glide, after the writes settle.
            if let Some(h) = auto.glide_timer.get_value() {
                h.clear();
                auto.glide_timer.set_value(None);
            }
            let delay = if just_opened {
                // Panel opening is a one-shot event, not churn: snap next tick
                // (the old synchronous behavior), no debounce lag on open.
                0
            } else if in_grace {
                // Wait out the remaining grace (+ a settle buffer) before gliding.
                (GRACE_MS - (js_sys::Date::now() - auto.last_user_drive.get()) + 60.0) as u64
            } else {
                GLIDE_DEBOUNCE_MS
            };
            let fire = step.clone();
            let h = set_timeout_with_handle(move || fire(), Duration::from_millis(delay)).ok();
            let glide_timer = auto.glide_timer;
            let glide_slot = auto.glide_slot;
            auto.glide_timer.set_value(h);
            on_cleanup(move || {
                if let Some(h) = glide_timer.get_value() {
                    h.clear();
                    glide_timer.set_value(None);
                }
                glide_slot.set_value(None);
            });
        });
    }
}

/// Page-1 aspect ratio driving the fixed row height (same closure the panel
/// view uses; duplicated here so the auto-center machinery is self-contained).
fn aspect(state: ViewerState) -> f64 {
    state
        .doc
        .page1_size
        .get()
        .map(|s| if s.width > 0.0 { s.height / s.width } else { 0.75 })
        .unwrap_or(0.75)
}

/// Number of 2-column rows needed for the current page count.
fn rows(state: ViewerState) -> usize {
    row_count(state.doc.num_pages.get() as usize)
}
