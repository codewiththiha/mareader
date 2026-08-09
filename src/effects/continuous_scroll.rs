//! Continuous-scroll effect: maps container scroll -> viewer.scroll_top and
//! batches scale changes. OWNED BY branch A (viewer/continuous).

use crate::core::state::AppState;

/// Must be called once when the continuous view mounts.
pub fn continuous_scroll(_state: AppState) {
    // TODO(branch A): scroll listener (passive + rAF-throttled) on #page-list,
    // visible-range memo, scale-change batching via engine::render_pages.
}
