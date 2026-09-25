//! Engine lifecycle/resource counters: the diagnostics snapshot's engine
//! half.
//!
//! The counters themselves live where the resources live — the engine session
//! (`public/engine/state.ts`) bumps them on the open/teardown paths that
//! actually create or release the document, the pdf.js worker, and page
//! renders. This module is the read side: a typed projection of the engine's
//! `stats()` facade member, plus the opt-in switch for the engine's
//! event narration.
//!
//! Host builds have no engine to read: [`engine_stats`] answers `None` there
//! (the guard short-circuits before any wasm-bindgen import runs), which is
//! also the truthful answer.

use serde::{Deserialize, Serialize};

/// The engine's `Stats` facade shape. CONTRACT: field names mirror
/// `public/engine/types.ts`; a rename on either side is a diagnostics
/// regression, caught by the smoke teardown's pairing assertions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    /// Registered page hosts (live page surfaces).
    pub pages: u32,
    /// Cached thumbnail rasters.
    pub thumbs: u32,
    /// The thumbnail cache's ceiling.
    pub thumb_limit: u32,
    /// In-flight thumbnail renders.
    pub thumb_tasks: u32,
    /// Live page render tasks (started, not yet resolved).
    pub active_renders: u32,
    /// Thumbnail prefetches in flight (queued or rendering).
    pub active_prefetches: u32,
    /// The bounded page-render lane: jobs waiting for a slot, and the slots
    /// currently running. A teardown drains the queue, so the baseline can
    /// require both back to zero rather than trusting the lane to empty
    /// itself later.
    #[serde(default)]
    pub page_queue: u32,
    #[serde(default)]
    pub page_active: u32,
    /// The thumbnail lane's queued jobs and running slots, same contract.
    #[serde(default)]
    pub thumb_queue: u32,
    #[serde(default)]
    pub thumb_active: u32,
    /// Search index builds in flight (a 0/1 gauge). A close that lands
    /// mid-build must leave this empty or the baseline fails.
    #[serde(default)]
    pub search_active: u32,
    /// A document proxy is open.
    pub has_document: bool,
    /// A worker LoadingTask is registered on the session.
    pub has_loading_task: bool,
    /// Monotonic lifecycle counters. Pairing rules (asserted by the smoke
    /// teardown): `sessions_opened == sessions_destroyed`;
    /// `workers_created == workers_terminated`; `renders_started ==
    /// renders_completed + renders_cancelled + renders_failed`.
    pub sessions_opened: u64,
    pub sessions_destroyed: u64,
    pub workers_created: u64,
    pub workers_terminated: u64,
    pub renders_started: u64,
    pub renders_completed: u64,
    pub renders_cancelled: u64,
    pub renders_failed: u64,
    /// Jobs that entered the bounded render lane, and queue-level drops
    /// (superseded or unmounted before their turn).
    pub renders_queued: u64,
    pub renders_dropped: u64,
    /// Thumbnail prefetch lifecycle (warmup/idle cache fills): the pairing
    /// rule is `prefetches_started == prefetches_completed +
    /// prefetches_dropped`, with `active_prefetches` back to zero after a
    /// dispose.
    pub prefetches_started: u64,
    pub prefetches_completed: u64,
    pub prefetches_dropped: u64,
    /// The OPEN DOCUMENT's page count (`PDFDocumentProxy.numPages`) — not
    /// [`Self::pages`], which counts registered page hosts. The baseline
    /// reads this to prove the workload fixtures are large enough for a
    /// distant jump to cross many pages.
    #[serde(default)]
    pub document_pages: u32,
    /// The thumbnail generation map's size — per-canvas bookkeeping the
    /// lane keeps until document teardown. Measured so the baseline can see
    /// whether it grows unreasonably over a long session (Phase 0
    /// measurement, not a redesign).
    #[serde(default)]
    pub thumb_generation_size: u32,
    /// Raw-raster retention timers still armed (the theme scrub's "keep the
    /// unbaked raw briefly" timeouts). Teardown releases every page surface,
    /// so the baseline requires this back to zero.
    #[serde(default)]
    pub raw_retention_timers: u32,
    /// The pdf.js idle sweeper timer, 0/1. A document-scoped timer: destroy
    /// cancels it, so it must read 0 after a close.
    #[serde(default)]
    pub sweep_timer_armed: u32,
}

impl EngineStats {
    /// The teardown baseline's engine half: `true` only when every live
    /// gauge is empty and every counter pair is balanced — the state a
    /// closed reader must leave the engine in.
    pub fn drained(&self) -> bool {
        self.pages == 0
            && self.thumbs == 0
            && self.thumb_tasks == 0
            && self.active_renders == 0
            && self.active_prefetches == 0
            && self.page_queue == 0
            && self.page_active == 0
            && self.thumb_queue == 0
            && self.thumb_active == 0
            && self.search_active == 0
            && self.raw_retention_timers == 0
            && self.sweep_timer_armed == 0
            && self.thumb_generation_size == 0
            && !self.has_document
            && !self.has_loading_task
            && self.sessions_opened == self.sessions_destroyed
            && self.workers_created == self.workers_terminated
            && self.renders_started
                == self.renders_completed + self.renders_cancelled + self.renders_failed
            && self.prefetches_started == self.prefetches_completed + self.prefetches_dropped
    }
}

/// Read the engine's counters. `None` off-wasm, without the engine, or when
/// the answer is not the shape the facade promised — the diagnostics surface
/// reports the gap rather than guessing.
pub fn engine_stats() -> Option<EngineStats> {
    if !crate::bridge::has_pdf_reader() {
        return None;
    }
    let mut stats: EngineStats = serde_wasm_bindgen::from_value(crate::bridge::stats()).ok()?;
    // The build gauge lives on the Rust side of the bridge (the extraction
    // loop is wasm), so it is folded in here rather than counted in JS.
    stats.search_active = super::search::search_build_active();
    Some(stats)
}

/// Turn the engine's lifecycle event narration on/off. A no-op without the
/// engine; the counters behind [`engine_stats`] are always live.
pub fn set_lifecycle_log(on: bool) {
    if crate::bridge::has_pdf_reader() {
        crate::bridge::set_lifecycle_log(on);
    }
}
