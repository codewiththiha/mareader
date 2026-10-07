//! What a pane owes when the workspace leaves for the library.

use runtime_contract::boundary::ShellApi;

/// Write the durable reading point and stop in-flight document work.
pub fn prepare_leave(ctx: &crate::context::ReaderContext) {
    if let Some(point) = ctx.try_read_point() {
        ctx.api.read_point(&point);
    }
    // Stop in-flight raster work in the same tick.
    ctx.pane.quiesce();
}
