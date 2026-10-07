//! The host half of the rail's thumbnails, provided as context to
//! the rail.

use std::collections::{HashMap, HashSet};
use std::rc::Weak;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;
use wasm_bindgen::{JsCast, JsValue};

use super::Inner;
use crate::pane_wire::HostToPane;

/// The settle halves of one outstanding request.
struct Waiting {
    resolve: js_sys::Function,
    reject: js_sys::Function,
}

#[derive(Default)]
struct ThumbState {
    pane: Weak<Inner>,
    next: u64,
    waiting: HashMap<u64, Waiting>,
    /// Pages painted since the reset: a remounting cell starts
    /// uncovered.
    painted: HashSet<u32>,
}

/// One pane's remote thumbnail lane, Copy; the state dies with the
/// pane.
#[derive(Clone, Copy)]
pub struct RemoteThumbs {
    state: StoredValue<ThumbState, LocalStorage>,
    /// Bumped when every picture is stale; the cells watch it.
    pub epoch: RwSignal<u64>,
}

impl RemoteThumbs {
    pub(super) fn new() -> Self {
        Self {
            state: StoredValue::new_local(ThumbState::default()),
            epoch: RwSignal::new(0),
        }
    }

    pub(super) fn attach(&self, pane: Weak<Inner>) {
        self.state.update_value(|s| s.pane = pane);
    }

    /// Whether page `page` was painted under the current epoch.
    pub fn has(&self, page: u32) -> bool {
        self.state
            .try_with_value(|s| s.painted.contains(&page))
            .unwrap_or(false)
    }

    /// Render `page` into `canvas`; `slot` takes the request id for
    /// cancellation.
    pub async fn render(
        &self,
        page: u32,
        canvas: web_sys::HtmlCanvasElement,
        slot: Arc<AtomicU64>,
    ) -> Result<(), bool> {
        let Some((req, pane)) = self.state.try_update_value(|s| {
            s.next += 1;
            (s.next, s.pane.clone())
        }) else {
            return Err(true);
        };
        let Some(pane) = pane.upgrade() else {
            return Err(true);
        };
        if pane.disposed.get()
            || pane.incoming.borrow().is_some()
            || pane.ctx.reader.document.status.get_untracked()
                != reader_core::document::DocStatus::Ready
        {
            return Err(true);
        }
        let epoch = self.epoch.get_untracked();
        slot.store(req, Ordering::Relaxed);
        let state = self.state;
        let promise = js_sys::Promise::new(&mut |resolve, reject| {
            state.update_value(|s| {
                s.waiting.insert(req, Waiting { resolve, reject });
            });
        });
        pane.post_live(&HostToPane::Thumb { req, page });
        drop(pane);
        let bitmap = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|cancelled| cancelled.as_bool().unwrap_or(false))?;
        let Ok(bitmap) = bitmap.dyn_into::<web_sys::ImageBitmap>() else {
            return Err(false);
        };
        // Delivery can race unmount/reset: never resurrect a zeroed canvas.
        if slot.load(Ordering::Relaxed) != req
            || !canvas.is_connected()
            || self.epoch.try_get_untracked() != Some(epoch)
        {
            bitmap.close();
            return Err(true);
        }
        canvas.set_width(bitmap.width());
        canvas.set_height(bitmap.height());
        let drawn = canvas
            .get_context("2d")
            .ok()
            .flatten()
            .and_then(|c| c.dyn_into::<web_sys::CanvasRenderingContext2d>().ok())
            .is_some_and(|ctx| ctx.draw_image_with_image_bitmap(&bitmap, 0.0, 0.0).is_ok());
        bitmap.close();
        if !drawn {
            return Err(false);
        }
        let _ = self.state.try_update_value(|s| s.painted.insert(page));
        Ok(())
    }

    /// The cell unmounted: settle its request as cancelled and tell the
    /// frame to stop.
    pub fn cancel(&self, req: u64) {
        let waiting = self
            .state
            .try_update_value(|s| (s.waiting.remove(&req), s.pane.clone()));
        let Some((Some(waiting), pane)) = waiting else {
            return;
        };
        let _ = waiting.reject.call1(&JsValue::NULL, &JsValue::TRUE);
        if let Some(pane) = pane.upgrade() {
            pane.post_live(&HostToPane::ThumbCancel { req });
        }
    }

    pub(super) fn deliver(&self, req: u64, bitmap: JsValue) {
        let waiting = self
            .state
            .try_update_value(|s| s.waiting.remove(&req))
            .flatten();
        match waiting {
            Some(waiting) => {
                let _ = waiting.resolve.call1(&JsValue::NULL, &bitmap);
            }
            None => {
                if let Ok(bitmap) = bitmap.dyn_into::<web_sys::ImageBitmap>() {
                    bitmap.close();
                }
            }
        }
    }

    pub(super) fn fail(&self, req: u64, cancelled: bool) {
        if let Some(Some(waiting)) = self.state.try_update_value(|s| s.waiting.remove(&req)) {
            let _ = waiting
                .reject
                .call1(&JsValue::NULL, &JsValue::from_bool(cancelled));
        }
    }

    /// Every picture is stale: a new look. The cells render again.
    pub(super) fn invalidate(&self) {
        let _ = self.state.try_update_value(|s| s.painted.clear());
        let _ = self.epoch.try_update(|e| *e += 1);
    }

    /// Another document (or another frame): every outstanding request is
    /// cancelled and every picture is stale.
    pub(super) fn reset(&self) {
        let waiting = self
            .state
            .try_update_value(|s| std::mem::take(&mut s.waiting))
            .unwrap_or_default();
        for (_, waiting) in waiting {
            let _ = waiting.reject.call1(&JsValue::NULL, &JsValue::TRUE);
        }
        self.invalidate();
    }
}
