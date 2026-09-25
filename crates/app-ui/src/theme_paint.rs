//! The `<html>` painters: CSS custom properties computed from `Appearance`,
//! written to the document element. Pure DOM writes — the shell's theme
//! effect and the appearance scrubber's live paints both go through here, so
//! one computation owns the variables and no caller re-derives them.

use crate::appearance::{raster, reflow};
use leptos::prelude::request_animation_frame;
use reader_core::appearance::Appearance;
use reader_core::appearance::shared::{noise, texture};
use web_sys::wasm_bindgen::JsCast;

pub fn document_element() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
}

pub fn html_style() -> Option<web_sys::CssStyleDeclaration> {
    document_element()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|h| h.style())
}

pub(crate) fn body_el() -> Option<web_sys::HtmlElement> {
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
    // The PDF token set: the filter/blend pair (always) and whatever
    // overrides the tint produces (empty when no tint is active). The text
    // token set, always written alongside: the namespaces are disjoint, so
    // both formats find their own tokens waiting and a format swap needs no
    // extra wiring.
    let raster_vars = raster::token_vars(&a);
    let reflow_vars = reflow::token_vars(&a, ink_contrast);

    // All of it lands as ONE cssText write. Per-property writes each dirty
    // the root's style on their own — some fifteen invalidations per painted
    // frame, and the scrub path paints one per animation frame for the
    // length of a drag, each of which WKWebView may answer with its own
    // recalc — where a single serialized block costs one. The tokens this
    // layer owns are rebuilt from scratch, so the seven UI overrides a
    // removed tint leaves behind are gone by omission rather than by a
    // remove_property pass; everything else on the root — the engine's
    // --pdf-paper publish, the gloss dials, paint_shared's own writes —
    // rides through the rebuild verbatim.
    let owned = |name: &str| {
        raster::UI_TOKENS.contains(&name)
            || raster_vars.iter().any(|(n, _)| *n == name)
            || reflow_vars.iter().any(|(n, _)| *n == name)
    };
    let mut buf = String::with_capacity(512);
    for decl in style.css_text().split(';') {
        let Some((name, _)) = decl.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || owned(name) {
            continue;
        }
        buf.push_str(decl.trim());
        buf.push(';');
    }
    for (name, value) in raster_vars.iter().chain(reflow_vars.iter()) {
        buf.push_str(name);
        buf.push(':');
        buf.push_str(value);
        buf.push(';');
    }
    // NB: the cssText setter cannot fail — no Result to discard here.
    style.set_css_text(&buf);
}
