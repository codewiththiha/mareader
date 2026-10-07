//! `PdfSession`: the one owner of an open PDF document.
//!
//! Everything a document holds — the engine's document proxy and its pdf.js
//! worker, the page registry, the render and thumbnail lanes, the thumbnail
//! cache, the prefetch state, the raster theme, the paper palette and its
//! look-ahead, the search index — belongs to exactly one session. The JS
//! engine keys all of it by the session's id (`sid`); this type is the only
//! thing that mints a sid, and every document call goes through it.
//!
//! Lifecycle: [`PdfSession::create`] registers a fresh sid with the engine,
//! [`PdfSession::open`] opens one document in it (a new document is a new
//! session), and [`PdfSession::dispose`] tears it down: stop accepting
//! (every op refuses from the first line of `dispose`), advance invalidation
//! (the paper epoch, the look-ahead set; the engine's own lane epochs),
//! cancel and destroy the engine side (document, worker, page registry,
//! caches), and release. A disposed session answers every call with a no-op
//! or a `no_session` error — it can never be reused, and a sid is never
//! minted twice.
//!
//! The handle is a cheap `Rc` clone, so async work captures the session it
//! was started for and re-checks [`PdfSession::is_live`] after every await:
//! a result that outlives its session is dropped, never committed into
//! whatever session came next.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::api::{self, EngineError, EngineStats};
use crate::backdrop::{self, Paper};
use crate::bridge;
use crate::types::{
    CoverResult, OpenResult, OutlineEntry, PageSizeResult, RenderResult, ThumbResult,
};

use pdf_paper::PaperConfig;
use reader_core::search::SearchResponse;

pub(crate) mod search;

pub use search::{SEARCH_PAGE_CONCURRENCY, drop_retained_search};

use search::SearchState;

/// The session's lifecycle. Only `Live` admits work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Live,
    Disposing,
    Disposed,
}

/// The last sid minted in this realm. Monotonic: a sid names one session
/// for the life of the realm, so a stale sid can never reach a newer
/// session (the engine refuses a sid at or below the highest it has seen).
static NEXT_SID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

fn mint_sid() -> u32 {
    NEXT_SID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

struct Inner {
    sid: u32,
    state: Cell<State>,
    /// The engine accepted the sid (false on the host, where there is no
    /// engine, and when the engine was not loaded yet).
    registered: bool,
    paper: RefCell<Paper>,
    search: RefCell<SearchState>,
}

impl Drop for Inner {
    /// The safety net for a session dropped without `dispose` (an owner
    /// swept by its arena), or whose dispose future was dropped before it
    /// finished (`Disposing`). The engine side must still be released;
    /// nothing can await here, so the destroy runs detached (the engine
    /// ignores a sid it already forgot).
    fn drop(&mut self) {
        if self.state.get() != State::Disposed && self.registered && bridge::has_pdf_reader() {
            bridge::release_session_detached(self.sid);
        }
    }
}

/// A page's own elements, handed to [`PdfSession::register_page`] so the
/// engine paints into THESE and never into whatever element in the document
/// answers to the page's id — a second pane's page carries the same one.
#[derive(Clone, Copy)]
pub struct PageElements<'a> {
    pub canvas: &'a web_sys::Element,
    pub host: Option<&'a web_sys::Element>,
}

/// An open (or opening) PDF document. See the module docs.
#[derive(Clone)]
pub struct PdfSession {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for PdfSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PdfSession")
            .field("sid", &self.inner.sid)
            .field("state", &self.inner.state.get())
            .finish()
    }
}

fn no_session() -> EngineError {
    EngineError {
        name: "no_session".to_string(),
        message: "The PDF session was disposed".to_string(),
    }
}

impl PdfSession {
    /// A fresh session with a never-used sid, registered with the engine
    /// when one is attached. On the host (no engine) the session still
    /// exists — its state machines are exercised directly by the tests.
    pub fn create() -> Self {
        let sid = mint_sid();
        let registered = bridge::has_pdf_reader() && bridge::create_session(sid);
        Self {
            inner: Rc::new(Inner {
                sid,
                state: Cell::new(State::Live),
                registered,
                paper: RefCell::new(Paper::default()),
                search: RefCell::new(SearchState::default()),
            }),
        }
    }

    pub fn sid(&self) -> u32 {
        self.inner.sid
    }

    /// Whether the session still admits work. False from the first line of
    /// [`Self::dispose`] on, forever.
    pub fn is_live(&self) -> bool {
        self.inner.state.get() == State::Live
    }

    /// Whether `other` is this very session (not merely one with the same
    /// document).
    pub fn same(&self, other: &PdfSession) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    /// The engine may be called for this session: live, registered, and the
    /// engine still attached.
    fn engine(&self) -> bool {
        self.is_live() && self.inner.registered && bridge::has_pdf_reader()
    }

    /// As [`Self::engine`], as a `Result` for the async calls that report.
    fn require(&self) -> Result<u32, EngineError> {
        if !self.is_live() {
            return Err(no_session());
        }
        api::require_pdf_reader()?;
        if !self.inner.registered {
            return Err(no_session());
        }
        Ok(self.inner.sid)
    }

    pub(crate) fn with_paper<R>(&self, f: impl FnOnce(&mut Paper) -> R) -> R {
        f(&mut self.inner.paper.borrow_mut())
    }

    pub(crate) fn with_search<R>(&self, f: impl FnOnce(&mut SearchState) -> R) -> R {
        f(&mut self.inner.search.borrow_mut())
    }

    // --- Document -------------------------------------------------------

    /// Open `path` in this session. A session holds exactly one document:
    /// a second open is refused by the engine (`session_in_use`).
    pub async fn open(&self, path: &str) -> Result<OpenResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::open(sid, path).await;
        let open: OpenResult = api::resolve(value, "open")?;
        // Disposed while the engine worked: the engine has already torn the
        // document down with the session; nothing lands here.
        if !self.is_live() {
            return Err(no_session());
        }
        // The search index is scoped to the document's CONTENT identity
        // before anything can query it: a retained index built for these
        // exact bytes is adopted instead of re-extracted.
        self.with_search(|s| s.scope(open.fingerprint.as_deref(), path, open.num_pages));
        Ok(open)
    }

    /// The document's chapter tree, flattened into wire entries.
    ///
    /// INVARIANT: `Ok(empty)` means "no engine, no outline, or no session"
    /// — never an error. A genuine engine failure still surfaces as `Err`.
    pub async fn outline(&self) -> Result<Vec<OutlineEntry>, EngineError> {
        if !self.engine() {
            return Ok(Vec::new());
        }
        let value = bridge::resolve_outline(self.inner.sid).await;
        let payload: OutlinePayload = api::resolve(value, "resolveOutline")?;
        Ok(payload.outline)
    }

    /// Page 1 of `path` as a small JPEG (the shelf cover for this session's
    /// document). A standalone loading task inside the engine, counted on
    /// and torn down within this session.
    pub async fn cover_data_url(
        &self,
        path: &str,
        max_width: f64,
    ) -> Result<CoverResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::cover_data_url(sid, path, max_width).await;
        api::resolve::<CoverResult>(value, "cover")
    }

    /// Make this session the one whose paper the root backdrop shows.
    pub fn present(&self) {
        if self.engine() {
            bridge::present_session(self.inner.sid);
        }
    }

    // --- Pages ----------------------------------------------------------

    /// Register a page's canvas with THIS session's page registry.
    /// `host_id` `None` means the canvas id derives the host id. `elements`
    /// are the page's own canvas and host when the caller holds them: the
    /// engine then pins the page to them instead of looking the id up in the
    /// document, where a second pane's page carries the same id.
    pub fn register_page(
        &self,
        page: u32,
        canvas_id: &str,
        host_id: Option<&str>,
        elements: Option<PageElements<'_>>,
    ) {
        if self.engine() {
            bridge::register_page(
                self.inner.sid,
                page,
                canvas_id,
                host_id.unwrap_or(""),
                elements.map(|e| e.canvas),
                elements.and_then(|e| e.host),
            );
        }
    }

    pub fn unregister_page(&self, canvas_id: &str) {
        // Teardown-shaped: admitted until the engine forgets the sid (the
        // engine ignores a retired one anyway).
        if self.inner.registered && bridge::has_pdf_reader() {
            bridge::unregister_page(self.inner.sid, canvas_id);
        }
    }

    /// Cancel every in-flight page render of this session (the close path's
    /// first act; the dispose remains the one teardown).
    pub fn cancel_page_renders(&self) {
        if self.engine() {
            bridge::cancel_page_renders(self.inner.sid);
        }
    }

    /// Queue one page's raster at `rank` in this session's page lane: lower
    /// runs first.
    pub async fn render_page(
        &self,
        canvas_id: &str,
        scale: f64,
        render_text: bool,
        rank: u32,
    ) -> Result<RenderResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::render_page(sid, canvas_id, scale, render_text, rank).await;
        let result = api::resolve::<RenderResult>(value, "render")?;
        if !self.is_live() {
            return Err(no_session());
        }
        Ok(result)
    }

    /// The intrinsic (scale-1) box of one page, read from the document: one
    /// worker round trip, no surface, no pixels. The reader's fit maths asks
    /// BEFORE a page's first raster — this is what lets a fit re-resolve land
    /// ahead of the raster instead of correcting a page that is already on
    /// screen at the wrong size.
    /// Stand down one page's queued or in-flight raster, leaving its
    /// registration alone.
    pub fn cancel_page(&self, canvas_id: &str) {
        if let Ok(sid) = self.require() {
            bridge::cancel_page(sid, canvas_id);
        }
    }

    pub async fn probe_page_size(&self, page: u32) -> Result<PageSizeResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::probe_page_size(sid, page).await;
        let result = api::resolve::<PageSizeResult>(value, "probe")?;
        if !self.is_live() {
            return Err(no_session());
        }
        Ok(result)
    }

    // --- Thumbnails -----------------------------------------------------

    /// Render one thumbnail through this session's cached thumbnail lane.
    pub async fn render_thumb(
        &self,
        canvas_id: &str,
        page: u32,
        scale: f64,
    ) -> Result<ThumbResult, EngineError> {
        let sid = self.require()?;
        let value = bridge::render_thumb(sid, canvas_id, page, scale).await;
        let result = api::resolve::<ThumbResult>(value, "thumb")?;
        if !self.is_live() {
            return Err(no_session());
        }
        Ok(result)
    }

    /// Cancel an in-flight thumbnail render (cell unmounted). Does NOT evict
    /// the cached bitmap.
    pub fn cancel_thumb(&self, canvas_id: &str) {
        if self.inner.registered && bridge::has_pdf_reader() {
            bridge::cancel_thumb(self.inner.sid, canvas_id);
        }
    }

    /// Synchronous probe: is this page's thumbnail cached at `scale`?
    pub fn has_thumb(&self, page: u32, scale: f64) -> bool {
        self.engine() && bridge::has_thumb(self.inner.sid, page, scale)
    }

    /// Render a page into this session's thumbnail cache with no DOM canvas
    /// (idle prefetch — the look-ahead of the thumbnail lane).
    pub async fn prefetch_thumb(&self, page: u32, scale: f64) {
        if self.engine() {
            let _ = bridge::prefetch_thumb(self.inner.sid, page, scale).await;
        }
    }

    /// Park this session's idle prefetch (the pane left the screen).
    pub fn suspend_prefetches(&self) {
        if self.engine() {
            bridge::suspend_prefetches(self.inner.sid);
        }
    }

    pub fn resume_prefetches(&self) {
        if self.engine() {
            bridge::resume_prefetches(self.inner.sid);
        }
    }

    // --- Memory ---------------------------------------------------------

    /// Advisory cleanup of this session's no-longer-needed rasters.
    pub fn sweep(&self) {
        if self.engine() {
            bridge::sweep(self.inner.sid);
        }
    }

    /// Drop the scrub covers this session's page hosts still carry.
    pub fn sweep_snapshots(&self) {
        if self.engine() {
            bridge::sweep_snapshots(self.inner.sid);
        }
    }

    // --- Search ---------------------------------------------------------

    /// Build (or adopt) this session's search index. Returns the pages
    /// indexed. `Err(no_session)` when the session died mid-build — a
    /// half-built index is never recorded.
    pub async fn build_search_index(&self, num_pages: u32) -> Result<u32, EngineError> {
        search::build(self, num_pages).await
    }

    /// Query this session's index, then publish the query to its text
    /// layers so mounted pages repaint their highlight boxes.
    pub fn search(&self, query: &str) -> SearchResponse {
        let response = self.with_search(|s| s.query(query));
        if self.engine() {
            bridge::set_search_context(self.inner.sid, query);
        }
        response
    }

    pub fn set_active_match(&self, page: u32, index: i32) {
        if self.engine() {
            bridge::set_active_match(self.inner.sid, page, index);
        }
    }

    pub fn clear_highlights(&self) {
        if self.engine() {
            bridge::clear_highlights(self.inner.sid);
        }
    }

    pub(crate) async fn extract_page_text(&self, page: u32) -> Option<wasm_bindgen::JsValue> {
        if !self.engine() {
            return None;
        }
        Some(bridge::extract_page_text(self.inner.sid, page).await)
    }

    // --- Paper ----------------------------------------------------------

    /// The reader's paper settings, stated to THIS session (on creation and
    /// on every change).
    pub fn paper_configure(&self, blend_on: bool, config: PaperConfig) {
        if self.is_live() {
            backdrop::configure(self, blend_on, config);
        }
    }

    /// The document opened: the paper state machine starts for it. Nothing
    /// is published until the first live frame lands.
    pub fn paper_document_open(&self, path: &str, num_pages: u32) {
        if self.is_live() {
            backdrop::document_open(self, path, num_pages);
        }
    }

    /// A live render of `canvas_id` completed: drain its stashed frame.
    pub fn paper_live_frame(&self, canvas_id: &str) {
        if self.is_live() {
            backdrop::live_frame(self, canvas_id);
        }
    }

    /// The viewport's position along the page ladder.
    pub fn paper_position(&self, pos: f64) {
        if self.is_live() {
            backdrop::position(self, pos);
        }
    }

    /// This session's look-ahead samples in flight.
    pub fn paper_pending_samples(&self) -> usize {
        self.with_paper(|p| p.pending_samples())
    }

    pub(crate) fn set_paper(&self, hex: Option<&str>) {
        if self.engine() {
            bridge::set_paper(self.inner.sid, hex.unwrap_or(""));
        }
    }

    pub(crate) fn set_paper_active(&self, on: bool) {
        if self.engine() {
            bridge::set_paper_active(self.inner.sid, on);
        }
    }

    pub(crate) fn take_paper_frame(&self, canvas_id: &str) -> Option<api::PaperFrame> {
        if !self.engine() {
            return None;
        }
        api::paper::parse_frame(&bridge::take_paper_frame(self.inner.sid, canvas_id))
    }

    pub(crate) async fn sample_paper_page(
        &self,
        page: u32,
    ) -> Result<Option<api::PaperFrame>, EngineError> {
        if !self.engine() {
            return Ok(None);
        }
        let value = bridge::sample_paper_page(self.inner.sid, page).await;
        api::paper::resolve_frame(value, &format!("samplePaperPage({page})"))
    }

    // --- Diagnostics ----------------------------------------------------

    /// This session's own gauges and counters. `None` without an engine or
    /// once the engine forgot the sid.
    pub fn stats(&self) -> Option<EngineStats> {
        if !(self.inner.registered && bridge::has_pdf_reader()) {
            return None;
        }
        serde_wasm_bindgen::from_value(bridge::session_stats(self.inner.sid)).ok()
    }

    // --- Teardown -------------------------------------------------------

    /// Tear the session down. Idempotent; the returned future resolves once
    /// the engine side is gone (never on a timer).
    ///
    /// The session stops being usable AT THIS CALL, not at the future's
    /// first poll: the state leaves `Live` (every op above refuses), the
    /// invalidation advances (the paper epoch; in-flight samples are
    /// forgotten) and the search index is retained for a same-book reopen
    /// before this returns. The future is only the engine's own teardown
    /// (cancel lanes and prefetches, destroy the document and its worker,
    /// clear the page registry and caches, forget the sid) → `Disposed`.
    /// A future dropped unpolled still releases the engine: the `Inner`
    /// drop net covers a `Disposing` session too.
    pub fn dispose(&self) -> impl std::future::Future<Output = ()> + use<> {
        let begun = self.inner.state.get() == State::Live;
        if begun {
            self.inner.state.set(State::Disposing);
            self.with_paper(|p| p.invalidate());
            self.with_search(|s| search::retain(std::mem::take(s)));
        }
        let inner = self.inner.clone();
        async move {
            if !begun {
                return;
            }
            if inner.registered && bridge::has_pdf_reader() {
                let _ = bridge::destroy_session(inner.sid).await;
            }
            inner.state.set(State::Disposed);
        }
    }
}

/// `{ok:true, outline}` — engine.resolveOutline.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlinePayload {
    outline: Vec<OutlineEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        futures::executor::block_on(f)
    }

    #[test]
    fn sids_are_unique_and_monotonic() {
        let a = PdfSession::create();
        let b = PdfSession::create();
        assert!(b.sid() > a.sid());
        assert!(!a.same(&b));
        assert!(a.same(&a.clone()));
    }

    #[test]
    fn a_disposed_session_refuses_every_call() {
        let s = PdfSession::create();
        assert!(s.is_live());
        block_on(s.dispose());
        assert!(!s.is_live());
        let opened = block_on(s.open("/shelf/book.pdf"));
        assert_eq!(opened.err().map(|e| e.name).as_deref(), Some("no_session"));
        let rendered = block_on(s.render_page("cv", 1.0, false, 0));
        assert_eq!(
            rendered.err().map(|e| e.name).as_deref(),
            Some("no_session")
        );
        assert!(!s.has_thumb(1, 0.25));
        assert!(block_on(s.outline()).unwrap().is_empty());
        // A second dispose is a no-op, not a second teardown.
        block_on(s.dispose());
        assert!(!s.is_live());
    }

    #[test]
    fn a_session_stops_accepting_at_the_dispose_call() {
        let s = PdfSession::create();
        let teardown = s.dispose();
        // Not polled yet: the session already refuses.
        assert!(!s.is_live());
        let opened = block_on(s.open("/shelf/book.pdf"));
        assert_eq!(opened.err().map(|e| e.name).as_deref(), Some("no_session"));
        block_on(teardown);
        assert!(!s.is_live());
    }

    #[test]
    fn disposing_one_session_leaves_another_untouched() {
        let a = PdfSession::create();
        let b = PdfSession::create();
        backdrop::test_feed(&a, "/a.pdf");
        backdrop::test_feed(&b, "/b.pdf");
        block_on(a.dispose());
        assert!(b.is_live());
        assert!(backdrop::test_has_palette(&b));
        assert!(!backdrop::test_has_palette(&a));
    }
}

