//! The [`use_virtualizer`] hook: build the core, wire the effects, own
//! the cleanup, materialize the signals.

use leptos::prelude::*;

use crate::engine::{CoreConfig, VirtualizerCore, build_layout};
use crate::options::VirtualizerOptions;
use crate::virtualizer::{Virtualizer, VirtualizerInner};

/// Must be called inside a reactive owner.
pub fn use_virtualizer(options: VirtualizerOptions) -> Virtualizer {
    let count0 = options.count.get_untracked();
    let config = CoreConfig {
        budget: options.budget,
        shape: options.shape,
        gap: options.gap,
        padding_start: options.padding_start,
        padding_end: options.padding_end,
        viewport: options.initial_viewport,
        initial_offset: options.initial_offset,
        eps: options.measure_epsilon,
        max_retries: options.max_scroll_retries,
        render_screens: options.render_screens,
    };
    let layout = build_layout(
        &options.shape,
        count0,
        &*options.estimate_size,
        options.initial_viewport.cross,
        options.gap,
    );
    let mut core = VirtualizerCore::new(layout, config);
    core.set_pipeline(options.pipeline);
    let initial_range = core.range();
    let initial_scroll = core.scroll_top();
    let initial_epoch = options
        .epoch
        .map(|signal| signal.get_untracked())
        .unwrap_or(0);
    let inner = VirtualizerInner::new(options, core, initial_range, initial_scroll, initial_epoch);

    {
        let inner = inner.clone();
        Effect::new(move |_| {
            // A close resets it: a disposed read is a no-op.
            let count = match inner.options.count.try_get() {
                Some(count) => count,
                None => return,
            };
            let epoch = match inner.options.epoch.map(|signal| signal.try_get()) {
                Some(Some(epoch)) => epoch,
                Some(None) => return,
                None => 0,
            };
            let estimate = inner.options.estimate_size.clone();

            let step = {
                let mut core = inner.core.borrow_mut();
                if core.item_count() != count {
                    Some(core.set_count(count, &*estimate))
                } else if epoch != inner.last_epoch.get() {
                    Some(core.rebuild(&*estimate))
                } else {
                    None
                }
            };

            if let Some(step) = step {
                inner.apply(step);
            }
            inner.last_epoch.set(epoch);
        });
    }

    if let Some(signal) = inner.options.pinned {
        let inner = inner.clone();
        Effect::new(move |_| {
            // Same teardown window: a disposed source reads as not pinned.
            let Some(pinned) = signal.try_get() else {
                return;
            };
            let step = inner.core.borrow_mut().set_pinned(pinned);
            inner.apply(step);
        });
    }

    {
        let inner_handle = StoredValue::new_local(inner.clone());
        on_cleanup(move || inner_handle.with_value(|inner| inner.dispose()));
    }

    let virtualizer = Virtualizer::from_inner(inner);

    // Materialize the derived signals HERE, in this owner.
    let _ = virtualizer.items();
    let _ = virtualizer.rows();
    let _ = virtualizer.total_size();
    let _ = virtualizer.dominant();

    virtualizer
}
