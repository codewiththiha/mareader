//! The `<html>` painters: appearance computed into CSS custom properties.

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

fn body_el() -> Option<web_sys::HtmlElement> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
}

/// The layer every format shares: base mode, scheme, texture dials.
fn paint_shared(a: &Appearance) {
    let Some(el) = document_element() else { return };

    let prev_base = el.get_attribute("data-base");
    // Only a base swap needs the glass layer rebuilt.
    let kick = prev_base.as_deref() != Some(a.base.as_str());

    _ = el.set_attribute("data-base", a.base.as_str());
    let class = el.class_list();
    if a.base.is_dark() {
        _ = class.add_1("dark");
    } else {
        _ = class.remove_1("dark");
    }
    if kick {
        // Kill color transitions this frame, so tokens cannot mix mid-shift.
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

/// Write every appearance custom property and class from `a`.
pub fn paint_appearance_now(a: Appearance, ink_contrast: f64) {
    paint_shared(&a);

    let Some(style) = html_style() else { return };
    // The PDF token set and the text token set, both written together.
    let raster_vars = raster::token_vars(&a);
    let reflow_vars = reflow::token_vars(&a, ink_contrast);

    // All of it lands as ONE cssText write.
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

/// The chrome-only paint: a surface with no document, read from the shelf.
pub fn paint_chrome_appearance(a: Appearance) {
    paint_shared(&a);
    let Some(style) = html_style() else { return };
    // Cleared as a set first: no stale override from a removed tint.
    for name in raster::UI_TOKENS {
        let _ = style.remove_property(name);
    }
    for (name, value) in a.ui_overrides() {
        let _ = style.set_property(name, &value);
    }
}

/// Which pipeline a document paints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintPipeline {
    /// Everything: shared layer, raster tokens, reflow tokens.
    Document,
    /// Shared layer + UI tokens only. The library.
    Chrome,
}

/// Where a live appearance paint lands.
pub enum PaintTarget {
    Window,
    Pane(web_sys::Element),
    /// A delegated paint: the same rAF slot, run by the target's owner.
    Delegated(Box<dyn Fn(Appearance, f64)>),
}

thread_local! {
    static PIPELINE: std::cell::Cell<PaintPipeline> =
        const { std::cell::Cell::new(PaintPipeline::Document) };
}

pub fn set_paint_pipeline(p: PaintPipeline) {
    PIPELINE.with(|c| c.set(p));
}

fn paint_pipeline() -> PaintPipeline {
    PIPELINE.with(|c| c.get())
}

/// Paint `a` with whatever this document's pipeline is.
fn paint_for_pipeline(a: Appearance, ink_contrast: f64) {
    match paint_pipeline() {
        PaintPipeline::Document => paint_appearance_now(a, ink_contrast),
        PaintPipeline::Chrome => paint_chrome_appearance(a),
    }
}

/// Paint `a` at an explicit target: the window, or one pane root.
pub fn paint_into(target: PaintTarget, a: Appearance, ink_contrast: f64) {
    match target {
        PaintTarget::Window => paint_for_pipeline(a, ink_contrast),
        PaintTarget::Pane(el) => paint_pane_appearance(el, a, ink_contrast),
        PaintTarget::Delegated(paint) => paint(a, ink_contrast),
    }
}

/// The per-pane token block, in one cssText write.
pub fn paint_pane_appearance(el: web_sys::Element, a: Appearance, ink_contrast: f64) {
    _ = el.set_attribute("data-base", a.base.as_str());
    let class = el.class_list();
    if a.base.is_dark() {
        _ = class.add_1("dark");
    } else {
        _ = class.remove_1("dark");
    }

    let Ok(style) = el
        .clone()
        .dyn_into::<web_sys::HtmlElement>()
        .map(|h| h.style())
    else {
        return;
    };

    let mut vars: Vec<(String, String)> = Vec::with_capacity(24);
    for (name, value) in a.base_palette() {
        vars.push((name.to_string(), value.to_string()));
    }
    // The resolved UI set: tint overrides when active, plain base otherwise.
    let overrides = a.ui_overrides();
    if overrides.is_empty() {
        for (name, value) in a.base_palette() {
            let resolved = name.strip_prefix("--base-").unwrap_or(name);
            vars.push((format!("--color-{resolved}"), value.to_string()));
        }
    } else {
        for (name, value) in overrides {
            vars.push((name.to_string(), value));
        }
    }
    for (name, value) in crate::appearance::raster::token_vars(&a) {
        // --canvas-filter/--canvas-blend always apply; token_vars also
        // repeats the UI overrides, which were just written.
        if !name.starts_with("--color-") {
            vars.push((name.to_string(), value));
        }
    }
    for (name, value) in crate::appearance::reflow::token_vars(&a, ink_contrast) {
        vars.push((name.to_string(), value));
    }
    for (name, value) in reader_core::appearance::shared::texture::css_vars(&a) {
        vars.push((name.to_string(), value));
    }
    // `:root.dark` drives the global palette; a pane carries local tokens.
    let texture_palette = if a.base.is_dark() {
        [
            ("--texture-line", "rgba(255, 255, 255, 0.22)"),
            ("--texture-paper", "rgba(255, 255, 255, 0.08)"),
            ("--texture-blend", "screen"),
        ]
    } else {
        [
            ("--texture-line", "rgba(15, 23, 42, 0.16)"),
            ("--texture-paper", "rgba(15, 23, 42, 0.05)"),
            ("--texture-blend", "multiply"),
        ]
    };
    vars.extend(
        texture_palette
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string())),
    );

    let owned =
        |name: &str| vars.iter().any(|(n, _)| n == name) || raster::UI_TOKENS.contains(&name);
    let mut buf = String::with_capacity(768);
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
    for (name, value) in &vars {
        buf.push_str(name);
        buf.push(':');
        buf.push_str(value);
        buf.push(';');
    }
    style.set_css_text(&buf);
}

/// Remove every token a pane paint owns, back to pure inheritance.
pub fn clear_pane_appearance(el: web_sys::Element) {
    _ = el.remove_attribute("data-base");
    _ = el.class_list().remove_1("dark");
    let Ok(style) = el
        .clone()
        .dyn_into::<web_sys::HtmlElement>()
        .map(|h| h.style())
    else {
        return;
    };
    for name in raster::UI_TOKENS {
        let _ = style.remove_property(name);
    }
    for name in [
        "--canvas-filter",
        "--canvas-blend",
        "--texture-opacity",
        "--texture-scale-user",
        "--texture-line",
        "--texture-paper",
        "--texture-blend",
    ] {
        let _ = style.remove_property(name);
    }
    // The remaining owned sets drop by name pattern from the declaration list.
    let mut buf = String::new();
    for decl in style.css_text().split(';') {
        let Some((name, _)) = decl.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty()
            || name.starts_with("--base-")
            || name.starts_with("--tx-")
            || name.starts_with("--color-")
        {
            continue;
        }
        buf.push_str(decl.trim());
        buf.push(';');
    }
    style.set_css_text(&buf);
}
