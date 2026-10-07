//! Timeout, debounce and hide-delay primitives, shared by every surface.

use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;

/// A debounced trigger: repeated calls postpone the fire, `Copy`.
#[derive(Clone, Copy)]
pub struct Debouncer {
    trigger: StoredValue<Rc<dyn Fn()>, LocalStorage>,
    handle: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    /// `alive` lets the owner's cleanup disarm a pending fire.
    alive: StoredValue<bool, LocalStorage>,
}

impl Debouncer {
    /// (Re)schedule the fire `duration` from now.
    pub fn trigger(&self) {
        // Try, not plain: a trigger can land during a dispose flush.
        self.trigger.try_with_value(|f| f());
    }

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
    use_debounce_for(move || duration, on_fire)
}

/// A pending-timer slot owned by the current reactive scope.
pub fn use_timeout_slot() -> StoredValue<Option<TimeoutHandle>, LocalStorage> {
    let handle = StoredValue::new_local(None::<TimeoutHandle>);
    let cleanup = handle;
    on_cleanup(move || {
        if let Some(h) = cleanup.try_get_value().flatten() {
            h.clear();
        }
        let _ = cleanup.try_set_value(None);
    });
    handle
}

/// The duration-getter flavour of [`use_debounce`].
pub fn use_debounce_for(
    duration: impl Fn() -> Duration + 'static,
    on_fire: impl Fn() + 'static,
) -> Debouncer {
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
            let h = set_timeout_with_handle(
                move || {
                    // It can outlive its owner: a disposed read is a no-op.
                    if alive.try_get_value() == Some(true) {
                        f();
                    }
                },
                duration(),
            )
            .ok();
            handle.try_set_value(h);
        }
    });
    let debouncer = Debouncer {
        trigger: StoredValue::new_local(trigger_fn),
        handle,
        alive,
    };

    let cleanup = debouncer;
    on_cleanup(move || {
        cleanup.alive.try_set_value(false);
        cleanup.clear_handle();
    });
    debouncer
}

/// The hover-reveal and hide-after-grace pair the bars share.
#[derive(Clone)]
pub(crate) struct HoverVisibility {
    pub visible: RwSignal<bool>,
    pub show: Rc<dyn Fn()>,
    pub hide_later: Rc<dyn Fn()>,
}

/// Build a hover-visibility controller for the current owner.
pub(crate) fn use_hover_visibility(
    delay: Duration,
    postpone: impl Fn() -> bool + 'static,
) -> HoverVisibility {
    let visible = RwSignal::new(false);
    let handle = StoredValue::new_local(None::<TimeoutHandle>);
    let postpone = Rc::new(postpone);

    let show: Rc<dyn Fn()> = Rc::new({
        move || {
            if let Some(h) = handle.try_get_value().flatten() {
                h.clear();
            }
            handle.try_set_value(None);
            // A show can come from a callback that outlived the owner: try.
            let _ = visible.try_set(true);
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
                        let _ = vis.try_set(false);
                    }
                },
                delay,
            )
            .ok();
            handle.try_set_value(h);
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
