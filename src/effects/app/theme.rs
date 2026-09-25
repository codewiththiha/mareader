//! The durable theme applier: settings' appearance slice, painted onto
//! `<html>` and re-painted on every change — for the whole window's life,
//! which is why THIS surface is the shell's and no runtime's (§17: an effect
//! that must survive the reader boundary belongs to the shell). The pure
//! painters live in `app_ui::theme_paint`; the effect here owns the
//! subscription and the engine-rebake gating (a scrub leaves rasters alone).

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_state::AppearanceSignal;
use app_ui::appearance::{is_scrubbing, raster, schedule_save};
use app_ui::theme_paint::{document_element, paint_appearance_now};

use crate::state::ShellState;

pub fn apply_theme(state: ShellState, appearance: AppearanceSignal) {
    // One effect, one paint: hue / texture / grain all live on Appearance and
    // the live-preview path writes the same properties, so splitting them
    // into three effects tripled the work per settings write. The blend
    // backdrop needs nothing from here: it is pure CSS over the variables
    // this effect paints plus --pdf-paper, which the engine publishes on the
    // first render of each document.
    //
    // Subscribes to the appearance MEMO, not `settings`: reading the whole
    // blob had a layout toggle, a gloss colour and `last_path` on every open
    // repainting eight properties and re-baking the rasters for a look that
    // had not moved.

    // What the engine's rasters are baked against: the filter, the blend mode
    // and the base palette. Texture, grain and the UI tokens are CSS layers
    // over the canvas, so they repaint without touching a single bitmap.
    let baked = StoredValue::new(None::<(String, String, String)>);

    // The reflowable formats' ink dial, resolved in Rust into a flat --tx-ink,
    // so the appearance paint needs it alongside the look. Its own memo keeps
    // a dial nudge from subscribing the paint to the whole settings blob —
    // and the engine rebake signature below ignores it, so an ink nudge never
    // re-bakes a raster.
    let ink_contrast: Memo<f64> = Memo::new(move |_| state.settings.with(|s| s.text.ink_contrast));

    // Warm the style pipeline once after the first paint: the first slider
    // drag on a text document used to pay the cold-start cost of resolving
    // every custom property (and every colour mix) on the mounted blocks.
    // One forced layout read moves that cost to boot.
    let warmed = StoredValue::new(false);

    Effect::new(move || {
        let a = appearance.get();
        paint_appearance_now(a, ink_contrast.get());
        // The engine bakes the theme into its rasters (pages + thumbnails);
        // re-bake them at the freshly painted variables. Skipped while a scrub
        // is in flight (scrub mode owns the canvases; its exit bakes once at
        // the settled values). Only when the BAKE changed, though: grain and
        // texture sliders move an overlay, not the pixels underneath.
        let signature = (
            a.canvas_filter(),
            a.canvas_blend().to_string(),
            a.base.as_str().to_string(),
        );
        if baked.try_get_value().flatten().as_ref() != Some(&signature) {
            baked.set_value(Some(signature));
            // Not while a slider scrub is in flight: the drag repaints these
            // variables every frame and the scrub exit performs the one final
            // bake at the settled values.
            if !is_scrubbing() {
                raster::refresh_theme();
            }
        }

        if !warmed.get_value() {
            warmed.set_value(true);
            let _: Option<i32> = document_element()
                .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
                .map(|b| b.offset_height());
        }
    });

    // The reading surface resolves its paper from the OPEN FORMAT
    // (`data-format` on `<html>`): a READER-session fact, so the session owns
    // the write and carries it away on disposal. The theme's own durable
    // writes end here: a settings edit anywhere persists the blob the shell
    // owns the key for.
    Effect::new(move || {
        let settings = state.settings.get_untracked();
        schedule_save(settings);
    });
}
