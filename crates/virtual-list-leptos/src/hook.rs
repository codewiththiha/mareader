//! The [`use_virtualizer`] hook: build the core from options, wire the
//! reactive effects, register the owner cleanup, and materialize the derived
//! signals. Everything the hook touches is the pure state machine in
//! [`crate::engine`] plus a [`VirtualizerInner`] created here.
//!
//! Kept separate from the handle/glue code in [`crate::virtualizer`] so the
//! hook itself stays a compact list of wiring steps: config → core → signals
//! → effects → cleanup → materialize.

use leptos::prelude::*;

use crate::engine::{CoreConfig, VirtualizerCore, build_layout};
use crate::options::VirtualizerOptions;
use crate::virtualizer::{Virtualizer, VirtualizerInner};

/// Create a virtualizer. Must be called inside a reactive owner.
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
    };
    let layout = build_layout(
        &options.shape,
        count0,
        &*options.estimate_size,
        options.initial_viewport.cross,
        options.gap,
    );
    let core = VirtualizerCore::new(layout, config);
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
            let count = inner.options.count.get();
            let epoch = inner.options.epoch.map(|signal| signal.get()).unwrap_or(0);
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
            let step = inner.core.borrow_mut().set_pinned(signal.get());
            inner.apply(step);
        });
    }

    {
        let inner_handle = StoredValue::new_local(inner.clone());
        on_cleanup(move || inner_handle.with_value(|inner| inner.dispose()));
    }

    let virtualizer = Virtualizer::from_inner(inner);

    // MATERIALIZE THE DERIVED SIGNALS HERE, IN THIS OWNER.
    //
    // The reader's effects (navigation_sync's scroll→page sync, the pinned
    // window in ReaderPage) call `v.dominant()`, and other components call
    // `items()`/`rows()`/`total_size()`/... lazily through these accessors.
    // `Signal::derive_local` registers the memo with the CURRENT reactive
    // owner, and a Leptos effect runs its callback inside a per-run temporary
    // owner that is disposed the moment the run ends. A signal first created
    // inside such a run would therefore be DISPOSED before the next effect
    // run could read it — every subsequent `dominant().get()` panics with
    // "already been disposed", which kills the scroll→page sync, the zoom
    // window pinning and the thumbnail page tracking all at once. Creating
    // them eagerly here (use_virtualizer runs in the component's stable
    // owner) makes their lifetime the component's, not a single effect run's.
    // Memos are lazy, so this is node registration only — no computation.
    let _ = virtualizer.items();
    let _ = virtualizer.rows();
    let _ = virtualizer.total_size();
    let _ = virtualizer.dominant();

    virtualizer
}
