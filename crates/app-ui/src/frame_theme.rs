//! The runtime document's own theme applier.
//!
//! Every runtime lives in its own iframe, so it owns a separate `<html>`:
//! the Shell's theme effect paints the SHELL document and nothing inside the
//! frames. Before this module a runtime's appearance menu wrote its session
//! settings and nothing painted them (only the slider scrub's per-frame
//! preview reached the frame's root, which is why a tint drag looked alive
//! and a preset click did not), and nothing persisted them either — so the
//! look snapped back to whatever the runtime booted with.
//!
//! Each runtime installs this once per session, choosing its pipeline:
//!
//! - [`FramePipeline::Library`] paints the chrome layer only (shared + UI
//!   tokens). The shelf has no raster and no reflowable page, so it never
//!   writes `--canvas-*` / `--tx-*` and never addresses an engine.
//! - [`FramePipeline::Reader`] paints both document token sets (they are
//!   disjoint, so whichever format is open finds its own), the reflowable
//!   typography, and re-bakes the raster engine's pixels when — and only
//!   when — the baked signature moved. A reflowable document repaints from
//!   CSS alone; the engine hook is a no-op without a PDF session.
//!
//! Both publish the motion class and persist edits through the runtime's
//! boundary (`persist`), debounced, never on the boot read.

use std::time::Duration;

use leptos::prelude::*;

use reader_core::settings::Settings;

use crate::appearance::{is_scrubbing, raster};
use crate::theme_paint::{
    PaintPipeline, document_element, html_style, paint_appearance_now, paint_chrome_appearance,
    set_paint_pipeline,
};

/// How long after the last settings edit the runtime hands the blob to the
/// Shell for persistence.
const PERSIST_MS: u64 = 350;

/// The `<html>` class that freezes every CSS animation and transition (the
/// Shell publishes the same class on its own document).
const ANIMATIONS_OFF_CLASS: &str = "animations-off";

/// The runtime's persistence callback, held for the session.
type PersistFn = StoredValue<std::rc::Rc<dyn Fn(&Settings)>, LocalStorage>;
/// The runtime's adopt-time override pass, held for the session.
type ReconcileFn = StoredValue<Box<dyn Fn(&mut Settings)>, LocalStorage>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePipeline {
    Library,
    Reader,
}

/// Install the document's theme, typography, motion and persistence effects
/// in the current reactive owner (the runtime session).
///
/// `persist` hands an edited blob to the Shell; `reconcile` re-applies the
/// session-local overrides (the reader's per-launch blend) to a blob adopted
/// from storage.
pub fn install_frame_theme(
    settings: RwSignal<Settings>,
    pipeline: FramePipeline,
    persist: impl Fn(&Settings) + 'static,
    reconcile: impl Fn(&mut Settings) + 'static,
) {
    set_paint_pipeline(match pipeline {
        FramePipeline::Library => PaintPipeline::Chrome,
        FramePipeline::Reader => PaintPipeline::Document,
    });
    ensure_noise_overlay();

    let appearance = Memo::new(move |_| settings.with(|s| s.appearance));
    let ink_contrast = Memo::new(move |_| settings.with(|s| s.text.ink_contrast));

    match pipeline {
        FramePipeline::Library => {
            Effect::new(move |_| paint_chrome_appearance(appearance.get()));
        }
        FramePipeline::Reader => {
            // What the engine's rasters are baked against. Texture, grain and
            // the UI tokens are CSS layers over the canvas and never re-bake.
            let baked = StoredValue::new(None::<(String, String, String)>);
            Effect::new(move |_| {
                let a = appearance.get();
                paint_appearance_now(a, ink_contrast.get());
                let signature = (
                    a.canvas_filter(),
                    a.canvas_blend().to_string(),
                    a.base.as_str().to_string(),
                );
                if baked.try_get_value().flatten().as_ref() != Some(&signature) {
                    let first = baked.try_get_value().flatten().is_none();
                    baked.set_value(Some(signature));
                    // The boot paint precedes any raster, so there is nothing
                    // to re-bake yet; a scrub's exit performs its own bake.
                    if !first && !is_scrubbing() {
                        raster::refresh_theme();
                    }
                }
            });
            let typography = Memo::new(move |_| settings.with(|s| s.text.clone()));
            Effect::new(move |_| {
                let t = typography.get();
                let Some(style) = html_style() else { return };
                for (name, value) in reader_core::settings::typography::css_variables(&t) {
                    let _ = style.set_property(name, &value);
                }
            });
        }
    }

    let animations_off = Memo::new(move |_| !settings.with(|s| s.animations.enabled));
    Effect::new(move |_| {
        let off = animations_off.get();
        if let Some(el) = document_element() {
            let class = el.class_list();
            let _ = if off {
                class.add_1(ANIMATIONS_OFF_CLASS)
            } else {
                class.remove_1(ANIMATIONS_OFF_CLASS)
            };
        }
    });

    // Persistence: the runtime is the only writer of its own session's
    // settings, so it is the one that must hand them on. Skips the first run
    // (the boot read is already what storage holds) and debounces a drag into
    // one write.
    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    let pending = StoredValue::new_local(None::<Settings>);
    let booted = StoredValue::new_local(false);
    let adopting = StoredValue::new_local(false);
    let persist: PersistFn = StoredValue::new_local(std::rc::Rc::new(persist));
    let flush = move || {
        if let Some(h) = timer.try_update_value(|t| t.take()).flatten() {
            h.clear();
        }
        if let (Some(Some(snapshot)), Some(persist)) = (
            pending.try_update_value(|p| p.take()),
            persist.try_get_value(),
        ) {
            persist(&snapshot);
        }
    };
    Effect::new(move |_| {
        let snapshot = settings.get();
        if !booted.get_value() {
            booted.set_value(true);
            return;
        }
        if adopting.get_value() {
            adopting.set_value(false);
            return;
        }
        if let Some(h) = timer.try_update_value(|t| t.take()).flatten() {
            h.clear();
        }
        let _ = pending.try_set_value(Some(snapshot));
        let handle = set_timeout_with_handle(flush, Duration::from_millis(PERSIST_MS)).ok();
        let _ = timer.try_set_value(handle);
    });
    // Cross-frame sync. All runtime documents share the Shell's origin and
    // so its localStorage; the Shell's write of the active frame's edit fires
    // `storage` in every OTHER document, the frames included. A warm frame
    // adopts it, so its promotion reveals the current look instead of the one
    // it booted with. The active frame receives the echo of its own edit:
    // it is equal (skipped), or superseded by a newer local edit still
    // waiting to persist (skipped — the local one wins and lands next).
    use wasm_bindgen::JsCast;
    let reconcile: ReconcileFn = StoredValue::new_local(Box::new(reconcile));
    app_chrome::hooks::use_window_event::use_window_event("storage", move |ev| {
        let Ok(ev) = ev.dyn_into::<web_sys::StorageEvent>() else {
            return;
        };
        if ev.key().as_deref() != Some(reader_core::settings::SETTINGS_KEY) {
            return;
        }
        if pending.try_with_value(|p| p.is_some()).unwrap_or(true) {
            return;
        }
        let mut fresh = storage::load_settings();
        reconcile.with_value(|r| r(&mut fresh));
        if settings.try_with_untracked(|cur| *cur == fresh) != Some(false) {
            return;
        }
        // Adopted, not edited: the persistence effect must not bounce the
        // blob straight back through the Shell.
        adopting.set_value(true);
        settings.set(fresh);
    });

    // Flush rather than drop: a change made in the last beat before the
    // runtime was retired is still the user's change.
    on_cleanup(flush);
}

/// The film-grain layer, in THIS document. The noise classes and variables
/// are painted onto the frame's own `<body>` (`paint_shared`), and the grain
/// has to live next to them: an overlay in the Shell sat above the frames
/// but followed the Shell's body classes, not the runtime's — so the
/// animated grain never animated and a toggle made in a frame never reached
/// it. One per document, kept across the sessions a recycled frame mounts.
fn ensure_noise_overlay() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    if document
        .query_selector(".noise-overlay")
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let (Some(body), Ok(overlay)) = (document.body(), document.create_element("div")) else {
        return;
    };
    overlay.set_class_name("noise-overlay");
    let _ = overlay.set_attribute("aria-hidden", "true");
    let _ = body.append_child(&overlay);
}
