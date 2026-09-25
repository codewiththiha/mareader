//! Closing a document: flush the reading position and hand the Shell its
//! navigate-to-library command. There is no separate document-close teardown
//! — the route flip disposes the runtime as a unit
//! (`crate::runtime::ReaderRuntime::dispose`), so there is exactly one
//! teardown path.

use runtime_contract::boundary::ShellApi;

/// Close the current document and return to the library shelf.
///
/// The durable write happens HERE, inside the live session; the teardown is
/// the Shell's move. Closing the book navigates to the library, and that
/// navigation disposes this runtime as a unit (§12) — the session root's
/// cleanup owns the engine destroy, the sweeps and the resource release, so
/// nothing here resets reader state or waits on the engine. One teardown
/// path, not two.
pub fn close_document(ctx: &crate::context::ReaderContext) {
    // The durable write goes FIRST, through the boundary: the Shell owns the
    // library blob, and this session is about to end. The runtime disposal
    // itself is the Shell's move (navigate_library disposes the session);
    // the dispose tail flushes again unconditionally, so a late change still
    // lands.
    if let Some(point) = ctx.try_read_point() {
        ctx.api.read_point(&point);
    }
    // Stop in-flight raster work in the same tick as the click: from here
    // the navigate command crosses to the Shell and the dispose comes back
    // over the frame channel — hops during which a live page render could
    // finish as if leaving had interrupted nothing. Cancelling here is the
    // first act of teardown; the session root's cleanup still owns the
    // engine destroy, so there remains exactly one teardown path.
    pdf_engine::api::cancel_page_renders();
    ctx.api.navigate_library();
}
