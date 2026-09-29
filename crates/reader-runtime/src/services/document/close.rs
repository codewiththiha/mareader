//! What a pane owes when the workspace is about to leave for the library.
//!
//! Leaving is the HOST's move (`crate::host::ReaderHost::return_to_library`):
//! it asks every pane to prepare, then hands the Shell its navigate command,
//! and the Shell's dispose comes back over the boundary to tear the host and
//! its panes down (`crate::host::manager::PaneManager::dispose_all`). This
//! module is the pane's half of the first step, and nothing more: there is no
//! separate document-close teardown — a pane's document session ends in the
//! pane's dispose, which is the one teardown path.

use runtime_contract::boundary::ShellApi;

/// Write the pane's durable reading point and stop its in-flight raster work.
///
/// The durable write happens HERE, inside the live session, through the
/// boundary: the Shell owns the library blob, and this session is about to
/// end. The pane's dispose flushes again unconditionally, so a late change
/// still lands.
pub fn prepare_leave(ctx: &crate::context::ReaderContext) {
    if let Some(point) = ctx.try_read_point() {
        ctx.api.read_point(&point);
    }
    // Stop in-flight raster work in the same tick as the click: from here
    // the navigate command crosses to the Shell and the dispose comes back
    // over the frame channel — hops during which a live page render could
    // finish as if leaving had interrupted nothing. Cancelling is the first
    // act of teardown; the pane's dispose still owns the session's destroy.
    // THIS pane's session only.
    ctx.pane.pdf().cancel_page_renders();
}
