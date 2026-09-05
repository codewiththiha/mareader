//! Applies the persisted appearance to the DOM whenever it changes.
//!
//! Three layers, painted on every change (they write DISJOINT property
//! sets, so the pipelines never fight over a token):
//!   shared     — `data-base`, the `.dark` class, `color-scheme`, the texture
//!     and noise variables (identical for every format),
//!   raster     — `--canvas-filter` / `--canvas-blend` plus the seven tinted
//!     `--color-*` overrides, read by the pages that arrive as bitmaps
//!     (`effects::appearance::raster`),
//!   reflowable — the seven `--tx-*` tokens a reflowable page reads
//!     (`effects::appearance::reflow`, over reader-core's
//!     `reader_core::appearance::reflowable`). Such a page paints its own paper and ink
//!     directly, so no filter ever reaches it.
//!
//! WHY INLINE PROPERTIES RATHER THAN CSS BLOCKS. The old design had one
//! `:root[data-theme=...]` block per theme, so every look needed hand-written
//! CSS and only the six that existed were reachable. The tint is now continuous
//! — any hue, any strength — which cannot be enumerated in a stylesheet. The
//! stylesheet keeps the STRUCTURE (which var drives what) and the base
//! palettes; the computed values are pushed here. Setting a property to the
//! empty string removes the override and lets the stylesheet's own value win
//! again, which is how a tint is cleanly un-applied.
//!
//! SLIDER RAM. Writing `settings` on every `input` event made
//! WKWebView allocate a fresh filter intermediate for every visible page,
//! every tick — that is the 1.2GB spike while dragging Colour / Tint
//! strength. Sliders now live-paint CSS at most once per animation frame and
//! commit the Settings signal (and localStorage) only after the gesture
//! pauses. The filter STRING is unchanged, so the look is byte-identical.
//!
//! The scrub/commit scheduler for the sliders lives in the sibling
//! `appearance` module; this file keeps the painting itself, the
//! format attribute the CSS keys off, and the app effects.

use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;

use reader_core::appearance::shared::{noise, texture};
use reader_core::appearance::Appearance;
use reader_core::format::Format;
use reader_core::settings::GlossColor;
use reader_core::settings::RenderPipeline;
use crate::state::{AppState, AppearanceSignal};

use crate::effects::appearance::{raster, reflow, schedule_save};

fn document_element() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
}

pub(crate) fn html_style() -> Option<web_sys::CssStyleDeclaration> {
    document_element()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|h| h.style())
}

fn body_el() -> Option<web_sys::HtmlElement> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
}

/// The layer every format shares: the base-mode attribute, the `.dark`
/// class, the colour scheme, and the texture / grain dials. None of it
/// knows which format is open.
fn paint_shared(a: &Appearance) {
    let Some(el) = document_element() else { return };

    let prev_base = el.get_attribute("data-base");
    // Only a Light/Dark/Dim swap needs the glass layer rebuilt. Slider
    // ticks must not (appendix 19), and a same-base tint is already live
    // on `--color-*` — `.toolbar-glass:has(.menu-popover)` drops the
    // stale backdrop while the picker is open.
    let kick = prev_base.as_deref() != Some(a.base.as_str());

    _ = el.set_attribute("data-base", a.base.as_str());
    // Publish the texture too: in blend mode the backdrop bleeds the same
    // paper texture past the page body (styles/components/shell.css), and
    // the CSS keys off this attribute.
    let _ = el.set_attribute("data-texture", a.texture.as_str());
    let class = el.class_list();
    if a.base.is_dark() {
        _ = class.add_1("dark");
    } else {
        _ = class.remove_1("dark");
    }
    if kick {
        // Kill color transitions for this frame so toolbar buttons cannot
        // linger at a mid-mix of the old and new tokens.
        _ = class.add_1("theme-switching");
    }

    if let Some(style) = html_style() {
        let _ = style.set_property(
            "color-scheme",
            if a.base.is_dark() { "dark" } else { "light" },
        );
        for (name, value) in texture::css_vars(a) {
            let _ = style.set_property(name, &value);
        }
    }

    if kick {
        request_animation_frame(move || {
            if let Some(el) = document_element() {
                _ = el.class_list().remove_1("theme-switching");
            }
        });
    }

    let Some(body) = body_el() else { return };
    let class = body.class_list();
    for (name, on) in noise::body_class_state(a.noise) {
        if on {
            _ = class.add_1(name);
        } else {
            _ = class.remove_1(name);
        }
    }
    for (name, value) in noise::css_vars(a) {
        _ = body.style().set_property(name, &value);
    }
}

/// Write every appearance CSS custom property / class from `a`. Synchronous.
/// The filter string is the same one `Appearance::canvas_filter` already
/// produces — this does not invent a second pipeline. `ink_contrast` is
/// the reflowable formats' ink dial (0..=100), resolved into the flat
/// `--tx-ink` here rather than in a live stylesheet mix.
pub fn paint_appearance_now(a: Appearance, ink_contrast: f64) {
    paint_shared(&a);

    let Some(style) = html_style() else { return };
    // The PDF token set: clear the seven overridable tokens first so a
    // removed tint cannot leave a stale override behind, then write the
    // filter/blend pair and whatever overrides the tint produces.
    for token in raster::UI_TOKENS {
        _ = style.remove_property(token);
    }
    for (name, value) in raster::token_vars(&a) {
        _ = style.set_property(name, &value);
    }
    // The text token set, always written alongside: the namespaces are
    // disjoint, so both formats find their own tokens waiting and a format
    // swap needs no extra wiring.
    for (name, value) in reflow::token_vars(&a, ink_contrast) {
        _ = style.set_property(name, &value);
    }
}

pub fn apply_theme(state: AppState, appearance: AppearanceSignal) {
    // One effect, one paint: hue / texture / grain all live on Appearance,
    // and the live-preview path writes the same properties, so splitting
    // them into three effects just tripled the work on every settings write.
    // The blend backdrop needs nothing from here: it is pure CSS over the
    // same variables this effect paints (--canvas-filter / --canvas-blend)
    // plus --pdf-paper, which the engine publishes on the first render of
    // each document.
    //
    // It subscribes to the appearance MEMO, not to `settings`. Reading the
    // whole blob had a layout toggle, a gloss colour and `last_path` on every
    // document open all repainting eight custom properties and re-baking the
    // engine's rasters for a look that had not moved.

    // What the engine's rasters are baked against: the filter, the blend mode
    // and the base palette. Texture, grain and the UI tokens are CSS layers
    // over the canvas, so they repaint without touching a single bitmap.
    let baked = StoredValue::new_local(None::<(String, String, String)>);

    // The reflowable formats' ink dial: resolved in Rust (into a flat
    // --tx-ink), so the appearance paint needs it alongside the look. Its
    // own memo keeps a dial nudge from subscribing the paint to the whole
    // settings blob — and the engine rebake signature below ignores it, so
    // an ink nudge never re-bakes a single raster.
    let ink_contrast: Memo<f64> = Memo::new(move |_| state.settings.with(|s| s.text.ink_contrast));

    // Warm the style pipeline once after the first paint: the first slider
    // drag on a text document used to pay the cold-start cost of resolving
    // every custom property (and every colour mix) on the mounted blocks.
    // One forced layout read moves that cost to boot.
    let warmed = StoredValue::new_local(false);

    Effect::new(move || {
        let a = appearance.get();
        paint_appearance_now(a, ink_contrast.get());
        // The engine bakes the theme into its rasters (pages + thumbnails);
        // re-bake them at the freshly painted variables. A no-op while a
        // scrub is in flight (scrub mode owns the canvases then) and before
        // the first document opens — and for a text document, whose pages
        // repaint from the tokens alone.
        //
        // Only when the BAKE changed, though: dragging the grain or texture
        // slider moves an overlay, not the pixels underneath, and re-baking
        // every mounted page and thumbnail for it was the most expensive
        // thing an appearance tick could do.
        let signature = (
            a.canvas_filter(),
            a.canvas_blend().to_string(),
            a.base.as_str().to_string(),
        );
        if baked.try_get_value().flatten().as_ref() != Some(&signature) {
            baked.set_value(Some(signature));
            raster::refresh_theme();
        }

        if !warmed.get_value() {
            warmed.set_value(true);
            let _ = body_el().map(|b| b.offset_height());
        }
    });

    // The rendering pipeline is a one-field choice with an expensive
    // consequence (every mounted raster is swapped), so it gets its own
    // narrow effect: no other settings write may trigger it, and the engine
    // is told only when the reader actually flips it. The first run also
    // pushes the persisted choice into an engine that always boots live.
    let pipeline: Memo<RenderPipeline> = Memo::new(move |_| state.settings.with(|st| st.render_pipeline));

    Effect::new(move || {
        raster::set_live_pipeline(pipeline.get().is_live());
    });

    // Same narrowing for the gloss tokens: three fields out of the blob, so a
    // slider tick elsewhere in settings cannot rewrite them.
    let gloss: Memo<(GlossColor, String, f64)> = Memo::new(move |_| {
        state.settings.with(|st| {
            (st.gloss_color, st.gloss_custom.clone(), st.gloss_opacity)
        })
    });

    Effect::new(move || {
        let (color, custom, opacity) = gloss.get();
        let Some(el) = document_element() else {
            return;
        };
        let Some(style) = el.dyn_into::<web_sys::HtmlElement>().ok().map(|h| h.style()) else {
            return;
        };
        match color.resolve(&custom) {
            Some(hex) => {
                let _ = style.set_property("--gloss-color", &hex);
            }
            None => {
                let _ = style.remove_property("--gloss-color");
            }
        }
        let _ = style.set_property("--gloss-opacity", &format!("{:.2}", opacity));
    });

    // The reading surface resolves its paper from the OPEN FORMAT: the PDF
    // blend backdrop paints --color-paper (see shell.css), text/Markdown
    // pages paint the surface with --tx-paper. `data-format` is the one
    // CSS switch, and it also gates the dim text pages' texture family —
    // paint it here so it lands with the appearance it rides on.
    Effect::new(move || {
        let name = match state.reader.format() {
            Format::Pdf => "pdf",
            Format::Text => "text",
            Format::Markdown => "markdown",
        };
        if let Some(el) = document_element() {
            _ = el.set_attribute("data-format", name);
        }
    });

    Effect::new(move || {
        let settings = state.settings.with(|s| s.clone());
        schedule_save(settings);
    });
}
