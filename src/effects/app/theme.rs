//! The durable theme applier: the appearance slice painted onto `<html>`
//! for the window's life.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_state::AppearanceSignal;
use app_ui::appearance::{is_scrubbing, raster, schedule_save};
use app_ui::theme_paint::{document_element, paint_appearance_now};

use crate::state::ShellState;

pub fn apply_theme(state: ShellState, appearance: AppearanceSignal) {
    // One effect, one paint: hue, texture and grain share Appearance.

    // What the engine's rasters are baked against.
    let baked = StoredValue::new(None::<(String, String, String)>);

    // The reflowable formats' ink dial, its own memo.
    let ink_contrast: Memo<f64> = Memo::new(move |_| state.settings.with(|s| s.text.ink_contrast));

    // Warm the style pipeline once after the first paint.
    let warmed = StoredValue::new(false);

    Effect::new(move || {
        let a = appearance.get();
        paint_appearance_now(a, ink_contrast.get());
        // The engine bakes the theme into its rasters; re-bake at the painted
        // variables.
        let signature = (
            a.canvas_filter(),
            a.canvas_blend().to_string(),
            a.base.as_str().to_string(),
        );
        if baked.try_get_value().flatten().as_ref() != Some(&signature) {
            baked.set_value(Some(signature));
            // Not while a slider scrub is in flight.
            if !is_scrubbing() {
                raster::refresh_theme();
            }
        }

        // Not mid-scrub: the value only matters at the next launch.
        if !is_scrubbing() {
            remember_boot_paint();
        }

        if !warmed.get_value() {
            warmed.set_value(true);
            let _: Option<i32> = document_element()
                .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
                .map(|b| b.offset_height());
        }
    });

    // The reading surface resolves its paper per PANE.
    Effect::new(move || {
        let settings = state.settings.get_untracked();
        schedule_save(settings);
    });
}

/// localStorage key public/bootPaint.js reads before the shell exists.
const BOOT_PAINT_KEY: &str = "mareader.boot-paint.v1";

/// Remember the paper just painted, for the next launch.
fn remember_boot_paint() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(root) = document_element() else {
        return;
    };
    let Ok(Some(computed)) = window.get_computed_style(&root) else {
        return;
    };
    let paper = computed
        .get_property_value("--color-paper")
        .unwrap_or_default();
    let paper = paper.trim();
    if paper.is_empty() {
        return;
    }
    let scheme = computed
        .get_property_value("color-scheme")
        .unwrap_or_default();
    let value = format!("{paper}|{}", scheme.trim());
    let Ok(Some(storage)) = window.local_storage() else {
        return;
    };
    if storage.get_item(BOOT_PAINT_KEY).ok().flatten().as_deref() != Some(value.as_str()) {
        let _ = storage.set_item(BOOT_PAINT_KEY, &value);
    }
}
