//! The PDF engine's serializable resource report. The workspace can inspect
//! this plain-data contract without linking the engine's browser API.

use serde::{Deserialize, Serialize};

/// The engine's `Stats` facade shape. CONTRACT: field names mirror
/// `public/engine/types.ts`; a rename on either side is a diagnostics
/// regression, caught by the smoke teardown's pairing assertions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    /// This session's recent page-raster cost, milliseconds — an exponential
    /// mean over COMPLETED rasters, timed inside the lane slot so queueing is
    /// not part of it. The reader's virtualizer feeds it to its band as the
    /// pipeline's `fill_ms`, which is what turns "is this scroll faster than
    /// this machine can fill?" into a measurement. `0` until a render lands.
    #[serde(default)]
    pub fill_ms: f64,
    /// How many rasters one session's page lane runs at once. With `fill_ms`
    /// this is the whole capacity figure the band compares the scroll against.
    #[serde(default)]
    pub page_limit: u32,
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
    /// Estimated bytes of engine-owned raster categories — width x height x
    /// 4 RGBA by convention, an ESTIMATE for correlation (what the engine
    /// holds), never a physical allocation query and never a share of
    /// `wasmHeapBytes`/`jsHeapBytes`/process RSS. The page category overlaps
    /// the browser test's DOM-scanned `liveCanvasBytes` by construction:
    /// same surfaces, different ledger (engine registry vs DOM walk).
    #[serde(default)]
    pub page_canvas_bytes_est: u64,
    /// The thumbnail cache's rasters (raw + display), estimated the same
    /// way. Per-document cache: teardown empties it, so the baseline
    /// requires 0 after a close (alongside `thumbs == 0`).
    #[serde(default)]
    pub thumbnail_raster_bytes_est: u64,
    /// Retained unbaked raws (the appearance/scrub retention window), the
    /// byte half of `raw_retention_timers`. Teardown releases every page
    /// surface, so the baseline requires 0 after a close.
    #[serde(default)]
    pub raw_retention_bytes_est: u64,
    /// The bake-intermediates recycler (pooled canvases + shared scratch).
    /// Module-bounded, NOT document-owned: it may hold placeholders across
    /// closes, so the baseline records it and gates per-cycle drift instead
    /// of requiring zero.
    #[serde(default)]
    pub pooled_intermediate_bytes_est: u64,
    /// Engine sessions still held (live, or draining their teardown). The
    /// aggregate only; one session's own snapshot reports 0.
    #[serde(default)]
    pub sessions_live: u32,
    /// Engine sessions fully retired over the realm's life.
    #[serde(default)]
    pub sessions_retired: u64,
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
            && self.raw_retention_bytes_est == 0
            && self.sweep_timer_armed == 0
            && self.thumb_generation_size == 0
            && !self.has_document
            && !self.has_loading_task
            && self.sessions_live == 0
            && self.sessions_opened == self.sessions_destroyed
            && self.workers_created == self.workers_terminated
            && self.renders_started
                == self.renders_completed + self.renders_cancelled + self.renders_failed
            && self.prefetches_started == self.prefetches_completed + self.prefetches_dropped
    }
}

// only the changed file was rewritten
