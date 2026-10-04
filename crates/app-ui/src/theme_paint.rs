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

/// The chrome-only paint: what a surface WITHOUT a document needs — the
/// shared layer (base mode, `.dark`, colour scheme, texture/grain dials) and
/// the tint's UI-token overrides. No `--canvas-*` pair (there is no raster
/// to filter) and no `--tx-*` palette (there is no reflowable page): the
/// shelf paints what the shelf reads and nothing a reader would.
pub fn paint_chrome_appearance(a: Appearance) {
    paint_shared(&a);
    let Some(style) = html_style() else { return };
    // Cleared as a set first: a removed tint must not leave a stale override
    // tinting the UI.
    for name in raster::UI_TOKENS {
        let _ = style.remove_property(name);
    }
    for (name, value) in a.ui_overrides() {
        let _ = style.set_property(name, &value);
    }
}

/// Which pipeline a document paints. Set once per runtime document by
/// [`crate::frame_theme::install_frame_theme`]; the slider scrub's live paint
/// reads it so a drag in the shelf never writes reader tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaintPipeline {
    /// Everything: shared layer, raster tokens, reflow tokens. The reader,
    /// and the shell (whose backdrop shows between frames).
    Document,
    /// Shared layer + UI tokens only. The library.
    Chrome,
}

/// Where a live appearance paint lands. `Window` is the historic target (the
/// document root — the shared chrome and every inheriting pane); `Pane` is
/// one pane's own root box, used while independent themes are on.
pub enum PaintTarget {
    Window,
    Pane(web_sys::Element),
    /// A delegated paint: the same rAF-coalesced slot, run by whoever owns
    /// the real target (the reader host publishes a pane's look through its
    /// appearance boundary). The second value is the ink dial.
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

/// Paint `a` at an explicit target: the window (the shared layer plus both
/// pipelines' tokens) or one pane's root (its own base + tint + texture
/// tokens; grain stays on the window and inherits). The pane variant paints
/// the same COMPUTED values as the window variant — one token pipeline, two
/// destinations — so a pane's look and the window's look can never resolve
/// to different maths.
pub fn paint_into(target: PaintTarget, a: Appearance, ink_contrast: f64) {
    match target {
        PaintTarget::Window => paint_for_pipeline(a, ink_contrast),
        PaintTarget::Pane(el) => paint_pane_appearance(el, a, ink_contrast),
        PaintTarget::Delegated(paint) => paint(a, ink_contrast),
    }
}

/// The per-pane token block: the base palette (`--base-*`, the pane's
/// `data-base` selector cannot re-declare it — the stylesheet's tables live
/// on `:root`), the resolved `--color-*` set (tinted when the pane's look
/// has a tint; plain otherwise, because the window's tint would otherwise
/// leak in through inheritance), the PDF filter/blend pair, the reflow
/// palette and the texture dials. Grain is deliberately absent: noise is
/// the one global dial.
///
/// Same cssText discipline as [`paint_appearance_now`]: the owned set is
/// rebuilt from scratch in one write, and everything else inline on the
/// pane root — its layout box, the engine's publishes — rides through.
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
    // The resolved UI set: the tint's overrides when active, the plain base
    // values otherwise — written unconditionally so an untinted pane in a
    // tinted window cannot inherit the window's tinted tokens.
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
    // `:root.dark` drives the global grain/texture palette. A pane is not
    // :root, so independent base changes must carry the equivalent local
    // stroke/tint/blend tokens; the noise layer itself remains inherited and
    // global by design.
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

/// Remove every token a pane paint owns, returning the pane to pure
/// inheritance from the window's theme. The base attribute and class go
/// with them.
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
    // The remaining owned sets resolve to known name families: drop them by
    // pattern from the live declaration list.
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
