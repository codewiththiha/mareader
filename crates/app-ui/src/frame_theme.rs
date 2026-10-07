//! The runtime document's theme applier: each frame's own `<html>`,
//! painted per pipeline.

use std::time::Duration;

use leptos::prelude::*;

use reader_core::settings::Settings;

use crate::appearance::{is_scrubbing, raster};
use crate::theme_paint::{
    PaintPipeline, document_element, html_style, paint_appearance_now, paint_chrome_appearance,
    set_paint_pipeline,
};

/// How long before the runtime hands the blob to the Shell.
const PERSIST_MS: u64 = 350;

/// The `<html>` class freezing every CSS animation and transition.
const ANIMATIONS_OFF_CLASS: &str = "animations-off";

/// The `<html>` class a runtime wears while its frame is off screen.
const FRAME_HIDDEN_CLASS: &str = "frame-hidden";

/// The runtime's persistence callback, held for the session.
type PersistFn = StoredValue<std::rc::Rc<dyn Fn(&Settings)>, LocalStorage>;
/// The runtime's adopt-time override pass, held for the session.
type ReconcileFn = StoredValue<Box<dyn Fn(&mut Settings)>, LocalStorage>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramePipeline {
    Library,
    Reader,
    /// Document tokens without a grain layer: the Reader host owns that.
    Pane,
}

/// Install the document's theme, typography, motion and persistence.
pub fn install_frame_theme(
    settings: RwSignal<Settings>,
    pipeline: FramePipeline,
    persist: impl Fn(&Settings) + 'static,
    reconcile: impl Fn(&mut Settings) + 'static,
) {
    set_paint_pipeline(match pipeline {
        FramePipeline::Library => PaintPipeline::Chrome,
        FramePipeline::Reader | FramePipeline::Pane => PaintPipeline::Document,
    });
    if pipeline != FramePipeline::Pane {
        ensure_noise_overlay();
    }

    let appearance = Memo::new(move |_| settings.with(|s| s.appearance));
    let ink_contrast = Memo::new(move |_| settings.with(|s| s.text.ink_contrast));

    match pipeline {
        FramePipeline::Library => {
            Effect::new(move |_| paint_chrome_appearance(appearance.get()));
        }
        FramePipeline::Reader | FramePipeline::Pane => {
            // What the engine's rasters are baked against.
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
                    // The boot paint precedes any raster: nothing to re-bake.
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

    // Persistence: the runtime hands on its session's settings, debounced.
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
    // Cross-frame sync: an adopted blob is followed by the storage echo.
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
        // Adopted, not edited: the persistence effect must not bounce it back.
        adopting.set_value(true);
        settings.set(fresh);
    });

    // Flush rather than drop: a late change is still the user's.
    on_cleanup(flush);
}

/// Tell this document whether its frame is on screen. Idempotent.
pub fn mark_frame_hidden(hidden: bool) {
    let Some(el) = document_element() else {
        return;
    };
    let class = el.class_list();
    let _ = if hidden {
        class.add_1(FRAME_HIDDEN_CLASS)
    } else {
        class.remove_1(FRAME_HIDDEN_CLASS)
    };
}

/// The film-grain layer, in THIS document.
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
