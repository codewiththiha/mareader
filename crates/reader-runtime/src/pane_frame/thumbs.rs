//! The pane side of the host's thumbnail rail: page N rendered through the
//! engine's own thumbnail lane (cache, theme bake, cancellation) into a
//! proxy canvas in this document, then handed over as a transferred
//! `ImageBitmap` (docs/pane-runtimes.md, "Thumbnails").

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use leptos::task::spawn_local;
use wasm_bindgen::{JsCast, JsValue};

use crate::components::shell::sidebar::panels::thumbnails::geometry::THUMB_SCALE;
use crate::pane_wire::{PaneToHost, THUMB_MESSAGE};

thread_local! {
    /// In-flight requests: the host's request id → the proxy canvas.
    static PENDING: RefCell<HashMap<u64, web_sys::HtmlCanvasElement>> =
        RefCell::new(HashMap::new());
    /// Whether this pane's engine is inside an appearance scrub window
    /// (`Hook::Scrub`). Mid-drag the engine shows RAW rasters under the live
    /// CSS, so no look is baked into anything the rail could be holding —
    /// and the drag's exit is the one bake at the values the drag landed on,
    /// which says so itself. A re-bake request inside the window therefore
    /// stays quiet: otherwise every drag frame would re-render the rail.
    static SCRUBBING: Cell<bool> = const { Cell::new(false) };
}

/// Track the scrub window the host broadcasts. Called from the pane's hook
/// arm and from boot (a drag can be in flight when a pane opens).
pub(super) fn set_scrubbing(on: bool) {
    SCRUBBING.with(|s| s.set(on));
}

fn canvas_id(req: u64) -> String {
    format!("pane-thumb-{req}")
}

/// Render page `page` for request `req`.
pub(super) fn render(req: u64, page: u32) {
    let Some(pane) = super::realm::pane() else {
        send_failed(req, true);
        return;
    };
    let pdf = pane.context().pane.pdf();
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(canvas) = document
        .create_element("canvas")
        .ok()
        .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok())
    else {
        send_failed(req, false);
        return;
    };
    let id = canvas_id(req);
    canvas.set_id(&id);
    if let Some(sid) = pdf.session().map(|s| s.sid()) {
        let _ = canvas.set_attribute("data-engine-sid", &sid.to_string());
    }
    // In the document (the engine finds its canvas by id), never on screen.
    let _ = canvas.set_attribute(
        "style",
        "position:fixed;left:-10000px;top:0;visibility:hidden;pointer-events:none",
    );
    if let Some(body) = document.body() {
        let _ = body.append_child(&canvas);
    }
    PENDING.with(|p| p.borrow_mut().insert(req, canvas.clone()));
    spawn_local(async move {
        let result = pdf.render_thumb(&id, page, THUMB_SCALE).await;
        let still = PENDING.with(|p| p.borrow().contains_key(&req));
        if !still {
            release(&canvas);
            return;
        }
        match result {
            Ok(_) => deliver(req, page, &canvas).await,
            Err(e) => {
                forget(req);
                release(&canvas);
                send_failed(req, e.name == "cancelled");
            }
        }
    });
}

async fn deliver(req: u64, page: u32, canvas: &web_sys::HtmlCanvasElement) {
    let bitmap =
        match web_sys::window().map(|w| w.create_image_bitmap_with_html_canvas_element(canvas)) {
            Some(Ok(promise)) => wasm_bindgen_futures::JsFuture::from(promise).await.ok(),
            _ => None,
        };
    let current = PENDING.with(|p| p.borrow().contains_key(&req));
    forget(req);
    release(canvas);
    let Some(bitmap) = bitmap else {
        send_failed(req, false);
        return;
    };
    // createImageBitmap owns a separate backing store: cancelling the proxy
    // during that await must also close the eventual bitmap, not send it.
    if !current || super::realm::pane().is_none() {
        if let Some(bitmap) = bitmap.dyn_ref::<web_sys::ImageBitmap>() {
            bitmap.close();
        }
        return;
    }
    let message = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&message, &"t".into(), &THUMB_MESSAGE.into());
    let _ = js_sys::Reflect::set(&message, &"req".into(), &JsValue::from_f64(req as f64));
    let _ = js_sys::Reflect::set(&message, &"page".into(), &JsValue::from(page));
    let _ = js_sys::Reflect::set(&message, &"bitmap".into(), &bitmap);
    if !super::realm::send_object(&message, &bitmap)
        && let Some(bitmap) = bitmap.dyn_ref::<web_sys::ImageBitmap>()
    {
        bitmap.close();
    }
}

/// The host's cell unmounted: abort the render (the engine keeps its cached
/// bitmap for an instant repaint).
pub(super) fn cancel(req: u64) {
    if forget(req).is_some()
        && let Some(pane) = super::realm::pane()
    {
        pane.context().pane.pdf().cancel_thumb(&canvas_id(req));
    }
}

/// Every request, at dispose.
pub(super) fn cancel_all() {
    let pending: Vec<u64> = PENDING.with(|p| p.borrow().keys().copied().collect());
    for req in pending {
        cancel(req);
    }
}

/// Warm the engine's thumbnail cache around `page` (the rail's glide).
pub(super) fn prefetch(page: u32) {
    let Some(pane) = super::realm::pane() else {
        return;
    };
    let pdf = pane.context().pane.pdf();
    spawn_local(async move {
        pdf.prefetch_thumb(page, THUMB_SCALE).await;
    });
}

fn forget(req: u64) -> Option<web_sys::HtmlCanvasElement> {
    let canvas = PENDING.with(|p| p.borrow_mut().remove(&req));
    if let Some(canvas) = &canvas {
        release(canvas);
    }
    canvas
}

/// Out of the document with its backing store zeroed (WKWebView keeps a
/// detached canvas's store until GC otherwise).
fn release(canvas: &web_sys::HtmlCanvasElement) {
    canvas.set_width(0);
    canvas.set_height(0);
    canvas.remove();
}

fn send_failed(req: u64, cancelled: bool) {
    super::realm::send(&PaneToHost::ThumbFailed { req, cancelled });
}

/// This pane's look was re-baked into its engine. Every picture the rail
/// holds is the HOST's copy of a raster baked against the look before this
/// one, and only the host can re-render its cells: the frame says so, and
/// the host's rail renders again (`RemoteThumbs::invalidate`).
pub(super) fn pictures_stale() {
    if SCRUBBING.with(|s| s.get()) {
        return;
    }
    super::realm::send(&PaneToHost::ThumbsStale);
}

