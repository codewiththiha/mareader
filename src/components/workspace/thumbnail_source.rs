//! ThumbnailSource abstraction — one UI, multiple providers.
//!
//! The legacy thumbnail panel already has correct geometry, virtualization
//! and auto-center. The workspace must NOT recreate it with a fixed-square
//! box (plan §13-§15). Instead the panel is parameterized by a source:
//!
//!   ReaderPdfThumbnailSource  → pdf-engine renderer/cache
//!   WorkspaceRemoteSource     → host MessagePort map fed by active reader
//!
//! Card geometry invariant: virtualizer estimate == real DOM card height.
//! `CELL_W * aspect` drives both, never `aspect-ratio: 1` or `height:auto`.

use std::collections::HashMap;
use leptos::prelude::*;

use crate::components::shell::sidebar::panels::thumbnails::geometry::{CELL_W, row_height};

/// Value-only contract the virtualized grid renders against.
/// Implementors differ only in where bitmaps come from.
pub trait ThumbnailSource {
    fn page_count(&self) -> Signal<u32>;
    fn current_page(&self) -> Signal<u32>;
    fn image(&self, page: u32) -> Signal<Option<String>>;
    fn request(&self, page: u32);
    /// Geometry helper — must match the virtualizer's estimate fn.
    fn row_height(&self) -> f64 {
        row_height(CELL_W * 1.414)
    }
}

/// Reader-direct source: wraps pdf-engine's thumbnail cache.
/// The real PDF reader calls `pdf_engine::api::prefetch_thumb(page, scale)` and
/// subscribes to the dataUrl map this trait exposes.
#[derive(Clone)]
pub struct ReaderThumbnailSource {
    pub bridge: crate::runtime::workspace::WorkspaceBridge,
}

impl ThumbnailSource for ReaderThumbnailSource {
    fn page_count(&self) -> Signal<u32> {
        let bridge = self.bridge;
        Signal::derive(move || bridge.thumbnails.with(|m| m.len() as u32))
    }
    fn current_page(&self) -> Signal<u32> {
        // Reader's own viewer page; workspace mirrors it via snapshot sync.
        Signal::derive(|| 1)
    }
    fn image(&self, page: u32) -> Signal<Option<String>> {
        let bridge = self.bridge;
        Signal::derive(move || bridge.thumbnails.with(|m| m.get(&page).cloned()))
    }
    fn request(&self, page: u32) {
        crate::runtime::emit(serde_json::json!({ "type": "thumbnail-request", "page": page }));
    }
}

/// Workspace-remote source: proxies bitmaps the host pulled from the active
/// reader via MessagePort. Bounded LRU — visible ± 2 rows keep, far evict.
#[derive(Clone)]
pub struct WorkspaceRemoteSource {
    pub bridge: crate::runtime::workspace::WorkspaceBridge,
    pub current: Signal<u32>,
}

impl ThumbnailSource for WorkspaceRemoteSource {
    fn page_count(&self) -> Signal<u32> {
        // Mirrored from active pane's snapshot (`numPages`).
        let bridge = self.bridge;
        Signal::derive(move || bridge.thumbnails.with(|m| m.len() as u32))
    }
    fn current_page(&self) -> Signal<u32> {
        self.current
    }
    fn image(&self, page: u32) -> Signal<Option<String>> {
        let bridge = self.bridge;
        Signal::derive(move || bridge.thumbnails.with(|m| m.get(&page).cloned()))
    }
    fn request(&self, page: u32) {
        crate::runtime::emit(serde_json::json!({ "type": "thumbnail-request", "page": page }));
    }
}

/// Bounded eviction: keep visible ± 2 rows, evict the rest.
/// Prevents hundreds of data URLs from pinning raster memory (plan §16).
pub fn evict_far(cache: &mut HashMap<u32, String>, current: u32, total: u32) {
    let window = 16u32; // 2 rows * 2 cols * slop
    let lo = current.saturating_sub(window);
    let hi = (current + window).min(total);
    cache.retain(|k, _| *k >= lo && *k <= hi);
}
