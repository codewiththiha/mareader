//! Timeout / debounce / hide-delay primitives. The two families in use —
//! debounced triggers and hover-reveal + hide-after-grace — used to be
//! re-implemented in the title bar, bottom bar, floating search and
//! thumbnail panel, each with its own `StoredValue<Option<TimeoutHandle>>`
//! and cleanup dance.

use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

/// A debounced trigger: calling it repeatedly postpones the fire; calling it
/// after the last postponed window fires `on_fire` (or the pending fire is
/// cancelled on cleanup).
///
/// `Copy`: the handle fields are owner-scoped storage, so consumers can move
/// this into event closures and cleanup hooks freely.
#[derive(Clone, Copy)]
pub struct Debouncer {
    trigger: StoredValue<Rc<dyn Fn()>, LocalStorage>,
    handle: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    /// `alive` is what lets the owner's cleanup disarm a pending fire without
    /// capturing a non-`Send` handle in the cleanup closure (which must be
    /// `Send + Sync`): the fire closure is cloned into the timer and probes it.
    alive: StoredValue<bool, LocalStorage>,
}

impl Debouncer {
    /// (Re)schedule the fire `duration` from now.
    pub fn trigger(&self) {
        self.trigger.with_value(|f| f());
    }

    /// Cancel a pending fire.
    pub fn cancel(&self) {
        self.clear_handle();
    }

    fn clear_handle(&self) {
        if let Some(h) = self.handle.try_get_value().flatten() {
            h.clear();
        }
        self.handle.try_set_value(None);
    }
}

/// A debounced one-shot: typing bursts / resize storms cost one fire.
pub fn use_debounce(duration: Duration, on_fire: impl Fn() + 'static) -> Debouncer {
    let on_fire = Rc::new(on_fire);
    let handle = StoredValue::new_local(None::<TimeoutHandle>);
    let alive = StoredValue::new_local(true);

    let trigger_fn: Rc<dyn Fn()> = Rc::new({
        let on_fire = Rc::clone(&on_fire);
        move || {
            if let Some(h) = handle.try_get_value().flatten() {
                h.clear();
            }
            let f = Rc::clone(&on_fire);
            let h = set_timeout_with_handle(move || {
                if alive.get_value() {
                    f();
                }
            }, duration)
            .ok();
            handle.set_value(h);
        }
    });
    let debouncer = Debouncer {
        trigger: StoredValue::new_local(trigger_fn),
        handle,
        alive,
    };

    let cleanup = debouncer;
    on_cleanup(move || {
        cleanup.alive.set_value(false);
        cleanup.clear_handle();
    });
    debouncer
}

/// The hover-reveal / hide-after-grace pair shared by the title bar and the
/// bottom bar: `show` cancels a pending hide and reveals; `hide_later`
/// schedules a hide after `delay` unless `postpone` says the surface is held
/// open (an open popover, an open search, a pin…).
#[derive(Clone)]
pub struct HoverVisibility {
    pub visible: RwSignal<bool>,
    pub show: Rc<dyn Fn()>,
    pub hide_later: Rc<dyn Fn()>,
}

/// Build a hover-visibility controller owned by the current reactive owner.
/// The postponed check runs both when the hide is scheduled and when the
/// timer fires, so a hold acquired mid-grace also keeps the surface up.
pub fn use_hover_visibility(delay: Duration, postpone: impl Fn() -> bool + 'static) -> HoverVisibility {
    let visible = RwSignal::new(false);
    let handle = StoredValue::new_local(None::<TimeoutHandle>);
    let postpone = Rc::new(postpone);

    let show: Rc<dyn Fn()> = Rc::new({
        move || {
            if let Some(h) = handle.try_get_value().flatten() {
                h.clear();
            }
            handle.try_set_value(None);
            visible.set(true);
        }
    });

    let hide_later: Rc<dyn Fn()> = Rc::new({
        let postpone = postpone.clone();
        move || {
            // A hold (popover open, search open) keeps the bar up.
            if postpone() {
                return;
            }
            if let Some(h) = handle.try_get_value().flatten() {
                h.clear();
            }
            let postpone = postpone.clone();
            let vis = visible;
            let h = set_timeout_with_handle(
                move || {
                    if !postpone() {
                        vis.set(false);
                    }
                },
                delay,
            )
            .ok();
            handle.set_value(h);
        }
    });

    on_cleanup(move || {
        if let Some(h) = handle.try_get_value().flatten() {
            h.clear();
        }
        let _ = handle.try_set_value(None);
    });

    HoverVisibility {
        visible,
        show,
        hide_later,
    }
}
