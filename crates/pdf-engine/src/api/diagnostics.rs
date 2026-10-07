//! Engine lifecycle and resource counters for the diagnostics snapshot.
pub use pdf_core::diagnostics::EngineStats;

/// Read the engine's counters; `None` off-wasm or without the engine.
pub fn engine_stats() -> Option<EngineStats> {
    if !crate::bridge::has_pdf_reader() {
        return None;
    }
    let mut stats: EngineStats = serde_wasm_bindgen::from_value(crate::bridge::stats()).ok()?;
    // The build gauge lives on the Rust side, so it is folded in here.
    stats.search_active = crate::session::search::search_build_active();
    Some(stats)
}

/// Turn the engine's lifecycle narration on or off.
pub fn set_lifecycle_log(on: bool) {
    if crate::bridge::has_pdf_reader() {
        crate::bridge::set_lifecycle_log(on);
    }
}
