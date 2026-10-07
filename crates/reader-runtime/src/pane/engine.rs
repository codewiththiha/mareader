//! The pane's guarded view of its own PDF session.
//!
//! Every PDF engine call a pane makes goes through [`PdfPane`], made by
//! [`crate::pane::handle::PaneHandle::pdf`]: it carries the `PdfSession` the
//! pane holds at that moment (none for a reflowable document or before the
//! first open) and the pane's (and runtime's) lifecycle as captured then.
//! Work ops no-op once disposal began; teardown ops (unregister, cancel,
//! sweeps) stay admitted until `Disposed`. Behind that, the session itself
//! refuses everything once it is disposed — so a view captured before a
//! reopen talks to the OLD session, which answers nothing, and can never
//! reach the new document.
//!
//! The four identity checks a committing result passes (see
//! `docs/session-ownership.md`): runtime and pane (the captured flags, and
//! [`PdfPane::still_current`] for results that land after an await), session
//! (the captured `PdfSession` is live and still the pane's), operation (the
//! engine's per-op guards, per session).

use leptos::prelude::{LocalStorage, StoredValue, WithValue, use_context};
use pdf_engine::api::EngineError;
use pdf_engine::types::{
    CoverResult, OpenResult, OutlineEntry, PageSizeResult, RenderResult, ThumbResult,
};
use pdf_engine::{PageElements, PdfSession};
use reader_core::search::SearchResponse;

/// See the module docs. Clone, not Copy: it holds a session handle.
#[derive(Clone)]
pub struct PdfPane {
    session: Option<PdfSession>,
    work: bool,
    teardown: bool,
}

fn refused() -> EngineError {
    EngineError {
        name: "no_session".to_string(),
        message: "The pane holds no live PDF session".to_string(),
    }
}

impl PdfPane {
    pub(crate) fn new(session: Option<PdfSession>, work: bool, teardown: bool) -> Self {
        Self {
            session,
            work,
            teardown,
        }
    }

    /// A view onto nothing (a host mounted outside any pane): every call is
    /// a no-op or a refusal.
    pub(crate) fn none() -> Self {
        Self::new(None, false, false)
    }

    /// The session this view was captured with.
    pub fn session(&self) -> Option<&PdfSession> {
        self.session.as_ref()
    }

    /// The session, when document WORK may use it.
    fn working(&self) -> Option<&PdfSession> {
        self.session.as_ref().filter(|s| self.work && s.is_live())
    }

    /// The session, when TEARDOWN may still touch it.
    fn tearing(&self) -> Option<&PdfSession> {
        self.session.as_ref().filter(|_| self.teardown)
    }

    /// Whether a result obtained through this view may still commit: the
    /// pane still admits work and still holds THIS session, live.
    pub fn still_current(&self, pane: &crate::pane::handle::PaneHandle) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| pane.admits_work() && pane.holds_pdf(s))
    }

    // --- Document -------------------------------------------------------

    pub async fn open(&self, path: &str) -> Result<OpenResult, EngineError> {
        match self.working() {
            Some(s) => s.open(path).await,
            None => Err(refused()),
        }
    }

    pub async fn outline(&self) -> Result<Vec<OutlineEntry>, EngineError> {
        match self.working() {
            Some(s) => s.outline().await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn cover_data_url(
        &self,
        path: &str,
        max_width: f64,
    ) -> Result<CoverResult, EngineError> {
        match self.working() {
            Some(s) => s.cover_data_url(path, max_width).await,
            None => Err(refused()),
        }
    }

    pub fn present(&self) {
        if let Some(s) = self.working() {
            s.present();
        }
    }

    // --- Pages ----------------------------------------------------------

    /// Register a mounted page WITH its own elements: the engine is pinned
    /// to them and never looks the id up in the document, where another
    /// pane's page carries the same one. There is no id-only form on
    /// purpose.
    pub fn register_page(
        &self,
        page: u32,
        canvas_id: &str,
        host_id: Option<&str>,
        elements: PageElements<'_>,
    ) {
        if let Some(s) = self.working() {
            s.register_page(page, canvas_id, host_id, Some(elements));
        }
    }

    pub fn unregister_page(&self, canvas_id: &str) {
        if let Some(s) = self.tearing() {
            s.unregister_page(canvas_id);
        }
    }

    pub fn cancel_page_renders(&self) {
        if let Some(s) = self.tearing() {
            s.cancel_page_renders();
        }
    }

    /// Queue one page's raster at `rank` in the session's page lane (lower
    /// first).
    pub async fn render_page(
        &self,
        canvas_id: &str,
        scale: f64,
        render_text: bool,
        rank: u32,
    ) -> Result<RenderResult, EngineError> {
        match self.working() {
            Some(s) => s.render_page(canvas_id, scale, render_text, rank).await,
            None => Err(refused()),
        }
    }

    /// Stand down one page's queued or in-flight raster without unregistering it.
    /// Same liveness rule as a render.
    pub fn cancel_page(&self, canvas_id: &str) {
        if let Some(s) = self.working() {
            s.cancel_page(canvas_id);
        }
    }

    /// One page's intrinsic (scale-1) box, from the document: a worker round
    /// trip and no pixels. Read before a page's first raster so a fit that the
    /// page's real size would move lands BEFORE the raster (see
    /// `components::formats::pdf::canvas`).
    pub async fn probe_page_size(&self, page: u32) -> Result<PageSizeResult, EngineError> {
        match self.working() {
            Some(s) => s.probe_page_size(page).await,
            None => Err(refused()),
        }
    }

    // --- Thumbnails -----------------------------------------------------

    pub async fn render_thumb(
        &self,
        canvas_id: &str,
        page: u32,
        scale: f64,
    ) -> Result<ThumbResult, EngineError> {
        match self.working() {
            Some(s) => s.render_thumb(canvas_id, page, scale).await,
            None => Err(refused()),
        }
    }

    pub fn cancel_thumb(&self, canvas_id: &str) {
        if let Some(s) = self.tearing() {
            s.cancel_thumb(canvas_id);
        }
    }

    pub fn has_thumb(&self, page: u32, scale: f64) -> bool {
        self.working().is_some_and(|s| s.has_thumb(page, scale))
    }

    pub async fn prefetch_thumb(&self, page: u32, scale: f64) {
        if let Some(s) = self.working() {
            s.prefetch_thumb(page, scale).await;
        }
    }

    /// Park the session's idle prefetch. Work-stopping, so admitted like
    /// teardown (a suspended pane no longer admits work).
    pub fn suspend_prefetches(&self) {
        if let Some(s) = self.tearing() {
            s.suspend_prefetches();
        }
    }

    pub fn resume_prefetches(&self) {
        if let Some(s) = self.working() {
            s.resume_prefetches();
        }
    }

    // --- Memory ---------------------------------------------------------

    pub fn sweep(&self) {
        if let Some(s) = self.tearing() {
            s.sweep();
        }
    }

    /// This session's engine report: diagnostics, and what the reader's band
    /// asks a raster's cost and the lane's width of. `None` with no live
    /// session, which the caller reads as "no new information".
    pub fn stats(&self) -> Option<pdf_core::diagnostics::EngineStats> {
        self.working().and_then(|s| s.stats())
    }

    pub fn sweep_snapshots(&self) {
        if let Some(s) = self.tearing() {
            s.sweep_snapshots();
        }
    }

    // --- Search ---------------------------------------------------------

    pub async fn build_search_index(&self, num_pages: u32) -> Result<u32, EngineError> {
        match self.working() {
            Some(s) => s.build_search_index(num_pages).await,
            None => Err(refused()),
        }
    }

    /// Query the session's index. `None` when the pane holds no live PDF
    /// session.
    pub fn search(&self, query: &str) -> Option<SearchResponse> {
        self.working().map(|s| s.search(query))
    }

    pub fn set_active_match(&self, page: u32, index: i32) {
        if let Some(s) = self.working() {
            s.set_active_match(page, index);
        }
    }

    pub fn clear_highlights(&self) {
        if let Some(s) = self.tearing() {
            s.clear_highlights();
        }
    }

    // --- Paper ----------------------------------------------------------

    pub fn paper_configure(&self, blend_on: bool, config: pdf_paper::PaperConfig) {
        if let Some(s) = self.working() {
            s.paper_configure(blend_on, config);
        }
    }

    pub fn paper_document_open(&self, path: &str, num_pages: u32) {
        if let Some(s) = self.working() {
            s.paper_document_open(path, num_pages);
        }
    }

    pub fn paper_live_frame(&self, canvas_id: &str) {
        if let Some(s) = self.working() {
            s.paper_live_frame(canvas_id);
        }
    }

    pub fn paper_position(&self, pos: f64) {
        if let Some(s) = self.working() {
            s.paper_position(pos);
        }
    }
}

/// A component's binding to the PDF session its pane held when it mounted.
///
/// A page canvas or thumbnail cell is mounted FOR one document: it binds
/// once, in its body, and every later engine call — the registration, the
/// render, the cleanup's cancel — goes to that session through
/// [`MountedPdf::pdf`]. Work is admitted only while the pane still holds that
/// exact session, so a component left over from a replaced document can never
/// register into, render from or blit out of the next one; teardown always
/// reaches the session it bound (the engine ignores a retired sid). A
/// component mounted outside any pane binds nothing and paints nothing.
///
/// Copy and `Send + Sync` (a pane handle plus an arena slot), so cleanup
/// closures and callbacks can carry it.
#[derive(Clone, Copy)]
pub struct MountedPdf {
    pane: Option<crate::pane::handle::PaneHandle>,
    session: StoredValue<Option<PdfSession>, LocalStorage>,
}

impl MountedPdf {
    /// Bind to the pane in context and its PDF session right now. Call from
    /// a component body (it reads context and allocates in the owner).
    pub fn bind() -> Self {
        let pane = use_context::<crate::pane::handle::PaneHandle>();
        let session = pane.and_then(|p| p.pdf().session().cloned());
        Self {
            pane,
            session: StoredValue::new_local(session),
        }
    }

    /// The engine id of the bound session, for the one DOM attribute that
    /// tells the engine whose element a thumbnail canvas is
    /// (`data-engine-sid`). `None` when nothing is bound.
    pub fn sid(&self) -> Option<u32> {
        self.session
            .try_with_value(|s| s.as_ref().map(PdfSession::sid))
            .flatten()
    }

    /// The guarded view onto the bound session, with the pane's lifecycle as
    /// it stands NOW. After the component's owner is gone, a view onto
    /// nothing.
    pub fn pdf(&self) -> PdfPane {
        let Some(pane) = self.pane else {
            return PdfPane::none();
        };
        match self.session.try_with_value(|s| s.clone()) {
            Some(session) => pane.pdf_for(session.as_ref()),
            None => PdfPane::none(),
        }
    }
}
