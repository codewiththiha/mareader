//! The first-paint gate: a paper-coloured cover masking the viewer
//! until the saved page has painted.

use leptos::prelude::*;

use reader_core::document::DocStatus;

/// Paginated surfaces clear the anchor guard on mount.
fn release_when_painted(state: crate::context::ReaderContext) {
    let r = state.reader;
    Effect::new(move |_| {
        if r.document.status.get() != DocStatus::Ready || r.viewer.first_paint.get() {
            return;
        }
        if r.viewer.mode.get().is_paginated() {
            if r.viewer.awaiting_anchor.get_untracked() {
                r.viewer.awaiting_anchor.set(false);
            }
            // PDF paged modes release on a real canvas completion too.
            if !r.reflowable() {
                return;
            }
            let vs = r.viewer;
            // Let the landed frame paint before the cover lifts.
            request_animation_frame(move || {
                // The reader may close first; a disposed write panics.
                if vs.first_paint.try_get_untracked().is_none() {
                    return;
                }
                vs.first_paint.set(true);
            });
        }
    });
}

/// The text anchor net: a loop that cannot land must not strand
/// the cover.
fn release_if_never_painted(state: crate::context::ReaderContext) {
    let r = state.reader;
    let net: StoredValue<Option<TimeoutHandle>, LocalStorage> = StoredValue::new_local(None);
    on_cleanup(move || {
        if let Some(handle) = net.try_get_value().flatten() {
            handle.clear();
        }
        let _ = net.try_set_value(None);
    });
    Effect::new(move |_| {
        if let Some(handle) = net.try_update_value(Option::take).flatten() {
            handle.clear();
        }
        if r.document.status.get() != DocStatus::Ready || r.viewer.first_paint.get() {
            return;
        }
        // Never trade a PDF's settled pixels for a timer.
        if !r.reflowable() {
            return;
        }
        let vs = r.viewer;
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                // `try_set` refuses a leaked fire.
                let _ = vs.first_paint.try_set(true);
            },
            std::time::Duration::from_millis(900),
        ) {
            let _ = net.try_set_value(Some(handle));
        }
    });
}

/// Both halves: the release path, then the net.
pub(crate) fn first_paint_gate(state: crate::context::ReaderContext) {
    release_when_painted(state);
    release_if_never_painted(state);
}
