//! Shared dismissal mechanics: Escape + outside-press handling with
//! exclusion selectors, a suspend signal (dragging), and a "topmost overlay
//! only" registry so two stacked surfaces don't both eat one Escape.
//!
//! This is the consolidation of the dismissal behaviour that used to be
//! duplicated in the primitive popover, the gloss surface, gloss selection
//! mode, the gloss context menu and the floating search — each with its own
//! window listeners, its own `.closest(...)` exclusions and its own cleanup.
//!
//! Rules baked in:
//! * outside events that land inside the surface's own refs are ignored
//!   (via the `is_inside` closure);
//! * outside events that land on an excluded selector are ignored (a search
//!   input does not dismiss when its own result list is clicked; a mark click
//!   does not exit selection mode);
//! * `enabled` suspends dismissal entirely (a drag in flight never collapses
//!   the card under the pointer);
//! * `topmost_only` gives Escape to the most recently opened surface only —
//!   press Escape with a context menu over a popover and only the menu goes.

use std::cell::RefCell;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::types::target_within_selectors;
use crate::components::primitives::hooks::use_window_event::use_window_event;

/// Which outside event the dismissal listens for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DismissTrigger {
    #[default]
    PointerDown,
    Click,
}

/// What dismisses the surface.
#[derive(Debug, Clone, Default)]
pub struct DismissPolicy {
    /// Escape closes it.
    pub escape: bool,
    /// An outside press/click closes it (trigger event configurable).
    pub outside: Option<DismissTrigger>,
    /// Elements matching these selectors count as "inside" (e.g.
    /// `".gloss-mark"`, `".gloss-select-bar"`).
    pub exclude_selectors: Vec<&'static str>,
    /// While `false` (or while the signal is absent) dismissal is live. Set
    /// `Some` to suspend it (drag in flight, processing…).
    pub enabled: Option<Signal<bool>>,
    /// Only the most recently opened dismissable surface receives Escape.
    pub topmost_only: bool,
}

// The topmost-overlay registry. Deliberately `thread_local!` — in WASM the
// UI is single-threaded, so this is an application-global that every
// dismissable surface shares WITHOUT threading a registry handle through
// each component's props (it is exactly the kind of ambient bookkeeping a
// prop would force onto surfaces that never care about stacking). The cost
// is that tests touching it must tolerate shared per-thread state, which
// the tests below do by pushing and popping symmetrically.
thread_local! {
    /// Stack of open dismissable ids, most recent last.
    static DISMISS_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static DISMISS_NEXT: RefCell<u64> = const { RefCell::new(1) };
}

fn next_id() -> u64 {
    DISMISS_NEXT.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    })
}

fn is_topmost(id: u64) -> bool {
    DISMISS_STACK.with(|s| s.borrow().last() == Some(&id))
}

fn push_stack(id: u64) {
    DISMISS_STACK.with(|s| {
        let mut s = s.borrow_mut();
        if !s.contains(&id) {
            s.push(id);
        }
    });
}

fn pop_stack(id: u64) {
    DISMISS_STACK.with(|s| {
        let mut s = s.borrow_mut();
        if let Some(pos) = s.iter().position(|&x| x == id) {
            s.remove(pos);
        }
    });
}

/// Dismiss a surface while `visible`, forwarding to `on_dismiss`.
///
/// `is_inside` answers "is this node part of the surface itself?" (its
/// anchors, its panel, its scroll area) — presses there never dismiss.
pub fn use_dismiss(
    visible: Signal<bool>,
    on_dismiss: Callback<()>,
    policy: DismissPolicy,
    is_inside: impl Fn(&web_sys::Node) -> bool + 'static,
) {
    let id = next_id();
    let is_inside = std::rc::Rc::new(is_inside);

    Effect::new(move |_| {
        // Gate: visibility AND the optional enabled signal.
        let enabled = policy.enabled.map(|e| e.get()).unwrap_or(true);
        let live = visible.get() && enabled;

        if live {
            push_stack(id);
        } else {
            pop_stack(id);
        }
        if !live {
            return;
        }

        let on_dismiss = on_dismiss;

        if policy.escape {
            // Parked-closure pattern: a re-run of this Effect cannot free a
            // live wasm shim mid-queue (see the hook's docs).
            use_window_event("keydown", move |ev: web_sys::Event| {
                let ke = ev.unchecked_ref::<web_sys::KeyboardEvent>();
                if ke.key() != "Escape" {
                    return;
                }
                if policy.topmost_only && !is_topmost(id) {
                    return;
                }
                on_dismiss.run(());
            });
            on_cleanup(move || {
                pop_stack(id);
            });
        }

        if let Some(trigger) = policy.outside {
            let excluded = policy.exclude_selectors.clone();
            let is_inside = std::rc::Rc::clone(&is_inside);
            let handler = move |ev: web_sys::Event| {
                // No target: nothing to test, ignore.
                let Some(node) = ev.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok()) else {
                    return;
                };
                // Inside the surface: the surface's own interaction.
                if is_inside(&node) {
                    return;
                }
                // Inside an excluded region: also the surface's own.
                if target_within_selectors(&ev, &excluded) {
                    return;
                }
                on_dismiss.run(());
            };
            match trigger {
                DismissTrigger::PointerDown => {
                    let handle = window_event_listener_untyped("pointerdown", handler);
                    on_cleanup(move || {
                        handle.remove();
                        pop_stack(id);
                    });
                }
                DismissTrigger::Click => {
                    let handle = window_event_listener_untyped("click", handler);
                    on_cleanup(move || {
                        handle.remove();
                        pop_stack(id);
                    });
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Push `ids`, run `body`, then pop them again so the shared stack is
    /// exactly as we found it — later tests on this thread start clean.
    fn with_stack<T>(ids: &[u64], body: impl FnOnce() -> T) -> T {
        for id in ids {
            push_stack(*id);
        }
        let out = body();
        for id in ids {
            pop_stack(*id);
        }
        out
    }

    #[test]
    fn the_most_recent_surface_is_topmost() {
        with_stack(&[7, 9], || {
            assert!(is_topmost(9));
            assert!(!is_topmost(7));
        });
    }

    #[test]
    fn an_empty_stack_has_no_topmost() {
        // Safe on a fresh thread's stack: an empty Vec's last() is None.
        let empty = DISMISS_STACK.with(|s| s.borrow().is_empty());
        if empty {
            assert!(!is_topmost(42));
        }
    }

    #[test]
    fn pushing_the_same_id_twice_does_not_stack_it_twice() {
        with_stack(&[5], || {
            push_stack(5);
            assert!(is_topmost(5));
            pop_stack(5);
            pop_stack(5); // second pop of an absent id: a no-op
            assert!(!is_topmost(5));
        });
    }

    #[test]
    fn popping_a_middle_surface_preserves_the_rest() {
        with_stack(&[1, 2, 3], || {
            pop_stack(2);
            assert!(!is_topmost(2));
            assert!(is_topmost(3));
            pop_stack(3);
            assert!(is_topmost(1)); // the oldest becomes topmost again
        });
    }

    #[test]
    fn ids_are_handed_out_monotonically() {
        let a = next_id();
        let b = next_id();
        assert!(b > a, "ids must never repeat: {a} then {b}");
    }
}
