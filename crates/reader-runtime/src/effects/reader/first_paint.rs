//! The first-paint gate: an opaque cover the colour of the reader's own paper
//! masks the viewer from the moment the document is ready until the page the
//! reader should see has actually PAINTED, so the first frames are never seen
//! — the reader appears already settled on the saved page instead of racing
//! toward it.
//!
//! The release is paint-driven and each surface owns its definition of
//! painted: PDF hosts release on a successful current raster completion
//! (`crate::zoom::target::page_rendered`); the text stream and text
//! pages lift it when their mount anchor lands (DOM text paints synchronously,
//! `crate::components::viewer::shells::anchor_settle`). The anchor loops run
//! under the cover: the viewer is mounted, only masked.

use leptos::prelude::*;

use reader_core::document::DocStatus;

/// Paginated surfaces have no scroll anchor to land. Clear that anchor guard
/// on mount, but only DOM text may release on the next frame: PDF hosts wait
/// for their successful raster callback in every mode.
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
                // One frame later the reader can be closed: the flag lives
                // with the reader state and a disposed write panics.
                if vs.first_paint.try_get_untracked().is_none() {
                    return;
                }
                vs.first_paint.set(true);
            });
        }
    });
}

/// The text anchor net: a settle loop that cannot land must not strand DOM
/// text under its cover. PDF frames instead have a host-owned startup error
/// deadline; a timer is never positive evidence that a raster painted. The
/// worst case is the cover lifting over a still-settling frame — never over the
/// wrong page (the strips' initial windows already open on it) and never over
/// the white invert (the paper-ready gate stands down until a colour is
/// sampled).
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
        // Never trade a PDF replacement's settled old pixels for a timer.
        // The host owns its startup error deadline; keep the text anchor net.
        if !r.reflowable() {
            return;
        }
        let vs = r.viewer;
        if let Ok(handle) = set_timeout_with_handle(
            move || {
                // The cleanup clears this handle; `try_set` is the second half
                // of the guard, so even a leaked fire cannot abort the wasm.
                let _ = vs.first_paint.try_set(true);
            },
            std::time::Duration::from_millis(900),
        ) {
            let _ = net.try_set_value(Some(handle));
        }
    });
}

/// Both halves of the gate: the surfaces' release path first, then the net that
/// catches a surface which never reports.
pub(crate) fn first_paint_gate(state: crate::context::ReaderContext) {
    release_when_painted(state);
    release_if_never_painted(state);
}
