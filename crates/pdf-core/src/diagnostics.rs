//! The engine's serializable resource report: plain data, no browser API.

use serde::{Deserialize, Serialize};

/// CONTRACT: field names mirror `public/engine/types.ts`; the smoke
/// teardown pairs them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStats {
    /// Timed inside the lane slot, so queue wait is excluded; `0`
    /// means unmeasured.
    #[serde(default)]
    pub fill_ms: f64,
    /// How many rasters this session's page lane runs at once.
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
    /// A teardown drains the queue, so the baseline requires both back to
    /// zero.
    #[serde(default)]
    pub page_queue: u32,
    #[serde(default)]
    pub page_active: u32,
    /// The thumbnail lane's queued jobs and running slots, same contract.
    #[serde(default)]
    pub thumb_queue: u32,
    #[serde(default)]
    pub thumb_active: u32,
    /// A 0/1 gauge; a close mid-build must leave it empty.
    #[serde(default)]
    pub search_active: u32,
    /// A document proxy is open.
    pub has_document: bool,
    /// A worker LoadingTask is registered on the session.
    pub has_loading_task: bool,
    /// Balance: sessions and workers pair off; renders end as done, cancelled
    /// or failed.
    pub sessions_opened: u64,
    pub sessions_destroyed: u64,
    pub workers_created: u64,
    pub workers_terminated: u64,
    pub renders_started: u64,
    pub renders_completed: u64,
    pub renders_cancelled: u64,
    pub renders_failed: u64,
    /// Lane entries and queue-level drops (superseded or unmounted first).
    pub renders_queued: u64,
    pub renders_dropped: u64,
    /// Prefetch balance: started == completed + dropped, and `active` returns
    /// to zero.
    pub prefetches_started: u64,
    pub prefetches_completed: u64,
    pub prefetches_dropped: u64,
    /// The open document's `numPages`, unlike [`Self::pages`], which
    /// counts hosts.
    #[serde(default)]
    pub document_pages: u32,
    /// Per-canvas bookkeeping kept until teardown: drift is gated, not
    /// zero.
    #[serde(default)]
    pub thumb_generation_size: u32,
    /// The "keep the unbaked raw briefly" timeouts; teardown releases them all.
    #[serde(default)]
    pub raw_retention_timers: u32,
    /// pdf.js's idle sweeper, 0/1: destroy cancels it, so a close reads 0.
    #[serde(default)]
    pub sweep_timer_armed: u32,
    /// Estimated bytes (w x h x 4), a correlation only, never an allocation
    /// query.
    #[serde(default)]
    pub page_canvas_bytes_est: u64,
    /// Thumbnail rasters (raw + display), estimated the same way; a close
    /// empties them.
    #[serde(default)]
    pub thumbnail_raster_bytes_est: u64,
    /// Retained unbaked raws: the byte half of `raw_retention_timers`.
    #[serde(default)]
    pub raw_retention_bytes_est: u64,
    /// Module-bounded, NOT document-owned: it may hold placeholders across
    /// closes, so drift is gated, not zero.
    #[serde(default)]
    pub pooled_intermediate_bytes_est: u64,
    /// Live or draining sessions; one session's own snapshot reports 0.
    #[serde(default)]
    pub sessions_live: u32,
    /// Engine sessions fully retired over the realm's life.
    #[serde(default)]
    pub sessions_retired: u64,
}

impl EngineStats {
    /// `true` only when every live gauge is empty and every pair balances.
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
