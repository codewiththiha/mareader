//! Dismissal mechanics: Escape, outside press, exclusions, suspension
//! and a topmost-only registry.

use std::cell::RefCell;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::types::target_within_selectors;
use crate::hooks::use_window_event::use_window_event;

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
    /// Dismissal is live while this is true.
    pub enabled: Option<Signal<bool>>,
    /// Only the most recently opened dismissable surface receives Escape.
    pub topmost_only: bool,
}

// The topmost-overlay registry, thread-local by design.
thread_local! {
    /// Stack of open dismissable ids, most recent last.
    static DISMISS_STACK: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
    static DISMISS_NEXT: RefCell<u64> = const { RefCell::new(1) };
    /// Modals whose Escape listener is installed (see [`use_modal_escape`]).
    static OPEN_MODALS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
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

/// Whether any dismissable surface is open.
fn has_open_dismissable() -> bool {
    DISMISS_STACK.with(|s| !s.borrow().is_empty())
}

/// Escape closes a modal unless a dismissable surface is open.
pub fn use_modal_escape(open: RwSignal<bool>) {
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        OPEN_MODALS.with(|n| n.set(n.get() + 1));
        on_cleanup(|| OPEN_MODALS.with(|n| n.set(n.get().saturating_sub(1))));
        use_window_event("keydown", move |ev: web_sys::Event| {
            if let Ok(key) = ev.dyn_into::<web_sys::KeyboardEvent>()
                && key.key() == "Escape"
                && !has_open_dismissable()
            {
                open.set(false);
            }
        });
    });
}

/// Whether this Escape already belongs to a layer above the page.
pub fn escape_is_claimed() -> bool {
    OPEN_MODALS.with(|n| n.get() > 0) || has_open_dismissable()
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
pub fn use_dismiss(
    visible: Signal<bool>,
    on_dismiss: Callback<()>,
    policy: DismissPolicy,
    is_inside: impl Fn(&web_sys::Node) -> bool + 'static,
) {
    let id = next_id();
    let is_inside = std::rc::Rc::new(is_inside);

    Effect::new(move |_| {
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
            // Parked-closure pattern: no free of a live shim mid-queue.
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
                let Some(node) = ev.target().and_then(|t| t.dyn_into::<web_sys::Node>().ok())
                else {
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
                    use_window_event("pointerdown", handler);
                    on_cleanup(move || {
                        pop_stack(id);
                    });
                }
                DismissTrigger::Click => {
                    use_window_event("click", handler);
                    on_cleanup(move || {
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

    /// Push `ids`, run `body`, pop them: the stack is left as found.
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
    fn an_open_modal_or_surface_claims_escape_from_the_page() {
        // Each test runs on its own thread, so these registries start empty.
        assert!(!escape_is_claimed());
        with_stack(&[77], || assert!(escape_is_claimed()));
        assert!(!escape_is_claimed());
        OPEN_MODALS.with(|n| n.set(1));
        assert!(escape_is_claimed());
        OPEN_MODALS.with(|n| n.set(0));
        assert!(!escape_is_claimed());
    }

    #[test]
    fn ids_are_handed_out_monotonically() {
        let a = next_id();
        let b = next_id();
        assert!(b > a, "ids must never repeat: {a} then {b}");
    }
}
