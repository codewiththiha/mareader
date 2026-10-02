//! Engine lifecycle/resource counters: the diagnostics snapshot's engine
//! half.
//!
//! The counters themselves live where the resources live — each engine
//! session (`public/engine/state.ts`) bumps them on the open/teardown paths
//! that actually create or release its document, its pdf.js worker, and its
//! page renders, and every bump lands in the realm totals too. This module
//! is the read side: a typed projection of the engine's aggregate `stats()`
//! (every session's gauges summed, the realm counter totals) — one
//! session's own numbers are [`crate::session::PdfSession::stats`] — plus
//! the opt-in switch for the engine's event narration.
//!
//! Host builds have no engine to read: [`engine_stats`] answers `None` there
//! (the guard short-circuits before any wasm-bindgen import runs), which is
//! also the truthful answer.

pub use pdf_core::diagnostics::EngineStats;

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
    stats.search_active = crate::session::search::search_build_active();
    Some(stats)
}

/// Turn the engine's lifecycle event narration on/off. A no-op without the
/// engine; the counters behind [`engine_stats`] are always live.
pub fn set_lifecycle_log(on: bool) {
    if crate::bridge::has_pdf_reader() {
        crate::bridge::set_lifecycle_log(on);
    }
}
