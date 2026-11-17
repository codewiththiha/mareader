//! Engine-owned document session, open pane slots, text layers, and papers.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_library::BookId;
use mareader_session::{
    Anchor, DocId, Document, Inserts, OpenTarget, PageKey, Prime, Raster, RenderError,
    Session as DocSession, Source, SpinePlace, Ticket, Typography, Want, typography,
};
use mareader_settings::TextSettings;
use mareader_workspace::PaneId;

use super::OpenRequest;
use super::poll::landing;
use crate::pane::ViewMode;
use crate::paper::Papers;
use crate::reading::Place;
use crate::session::Waker;
use crate::texts::TextLayers;
use crate::toast::Toast;
use crate::watch::{Reloads, Watch};

/// A document opening in a pane, waiting for its ticket to resolve.
#[derive(Debug)]
pub struct Opening {
    pub ticket: Ticket,
    pub name: String,
    pub file: Option<PathBuf>,
    pub page: usize,
    pub place: Option<Anchor>,
    pub spine: Option<SpinePlace>,
    pub pages: usize,
    pub book: Option<BookId>,
}

/// What a pane holds in the engine's document ledger.
#[derive(Debug)]
pub enum Slot {
    Opening(Opening),
    Open(Place),
}

/// Front-end view hooks invoked when engine document state transitions.
pub trait Sink {
    fn opened(
        &mut self,
        _: PaneId,
        place: &mut Place,
        page: usize,
        count: usize,
        _: Option<Anchor>,
    ) {
        if page > 0 {
            place.go_to(page, count);
        }
    }

    fn closed(&mut self, _: PaneId) {}

    fn rendered(&mut self, _: PageKey, _: Result<Raster, RenderError>) {}

    fn forget_pages(&mut self, _: DocId) {}

    fn adopt_pages(&mut self, _: DocId, _: DocId, _: &Document, _: &Document) {}

    fn prime_hint(&self, _: PaneId, _: &Place, _: &Document) -> Option<Prime> {
        None
    }

    fn reading_anchor(
        &self,
        _: PaneId,
        place: &Place,
        doc: &Document,
        _: &TextLayers,
    ) -> Option<Anchor> {
        doc.anchor(place.current)
    }

    fn reloaded_fixed(&mut self, _: PaneId, place: &mut Place, new: DocId, count: usize) {
        place.doc = new;
        place.selection = None;
        place.go_to(place.current, count);
    }

    #[allow(clippy::too_many_arguments)]
    fn retarget(
        &mut self,
        _: PaneId,
        place: &mut Place,
        _: DocId,
        new: DocId,
        was: Option<&Document>,
        is: Option<&Document>,
        _: &TextLayers,
        _: Option<(PaneId, Anchor)>,
    ) {
        let count = is.map_or(0, Document::page_count);
        let anchor = was.and_then(|d| d.anchor(place.current));
        let spine = anchor.and_then(|a| was.and_then(|d| d.spine_place(a)));
        let page = spine
            .as_ref()
            .and_then(|s| is.and_then(|d| d.locate_spine(s)))
            .or_else(|| anchor.and_then(|a| is?.locate(a)))
            .unwrap_or(place.current);
        place.doc = new;
        place.selection = None;
        place.go_to(page, count);
    }
}

impl Sink for () {}

/// Engine-owned document session, open pane places, typography, and caches.
pub struct State {
    pub(super) session: Option<DocSession>,
    pub(super) workers: usize,
    pub(super) progressive: bool,
    pub texts: TextLayers,
    pub papers: Papers,
    pub(super) panes: BTreeMap<PaneId, Slot>,
    pub(super) window: Arc<Typography>,
    pub(super) own: BTreeMap<PaneId, Arc<Typography>>,
    pub(super) asked: BTreeMap<PaneId, (DocId, Arc<Typography>, u64)>,
    pub(super) inserts: BTreeMap<PaneId, Arc<Inserts>>,
    pub(super) sheets: BTreeMap<PaneId, (f32, f32)>,
    pub(super) watch: Option<Watch>,
    pub(super) reloads: Reloads<Ticket, DocId>,
    pub(super) followed: Vec<DocId>,
}

impl Default for State {
    fn default() -> Self {
        let workers = if cfg!(target_arch = "wasm32") {
            0
        } else {
            DocSession::default_workers()
        };
        Self {
            session: None,
            workers,
            progressive: true,
            texts: TextLayers::default(),
            papers: Papers::default(),
            panes: BTreeMap::new(),
            window: Arc::new(typography(&TextSettings::default())),
            own: BTreeMap::new(),
            asked: BTreeMap::new(),
            inserts: BTreeMap::new(),
            sheets: BTreeMap::new(),
            watch: None,
            reloads: Reloads::default(),
            followed: Vec::new(),
        }
    }
}

impl State {
    /// Zero runs every job on the calling thread: deterministic, for tests.
    pub(crate) fn set_workers(&mut self, workers: usize) {
        if self.session.is_none() {
            self.workers = workers;
        }
    }

    /// Off opens EPUBs whole without waiting for a rasterized preview page.
    pub(crate) fn set_progressive(&mut self, on: bool) {
        self.progressive = on;
    }

    pub(super) fn ensure_session(&mut self, waker: &Waker) -> &mut DocSession {
        let workers = self.workers;
        if self.session.is_none() {
            let wake = Arc::clone(waker);
            self.watch = Watch::new(move || wake());
        }
        self.session.get_or_insert_with(|| {
            let wake = Arc::clone(waker);
            DocSession::new(workers, move || wake())
        })
    }

    /// Starts opening `request` in `pane`; reuses an open twin when ready.
    pub fn open(
        &mut self,
        waker: &Waker,
        pane: PaneId,
        mut request: OpenRequest,
        sink: &mut dyn Sink,
    ) -> Option<Toast> {
        if let Source::Path(p) = &request.source
            && let Ok(real) = std::fs::canonicalize(p)
        {
            request.source = Source::Path(real);
        }
        let name = request.label();
        let (file, page, place, spine, pages) = (
            request.file(),
            request.page,
            request.place,
            request.spine,
            request.pages,
        );
        let book = request.book;
        let typo = self.want(pane);
        let target = OpenTarget {
            page,
            place,
            spine: spine.clone(),
            pages,
        };
        if let Some(doc) = file.as_ref().and_then(|f| self.shown(f, &typo)) {
            let entry = self.session.as_ref().and_then(|s| s.get(doc));
            let page = entry.and_then(|e| e.doc.target_page(&target));
            let complete = entry.is_some_and(|e| e.doc.is_complete());
            if complete || page.is_some() {
                let count = entry.map_or(0, |e| e.doc.page_count());
                let page = page.unwrap_or_else(|| landing(entry.map(|e| &*e.doc), &target));
                let anchor = entry.and_then(|e| e.doc.target_anchor(&target));
                let mut p = Place::new(doc);
                p.book = book;
                sink.opened(pane, &mut p, page, count, anchor);
                self.set_slot(pane, Some(Slot::Open(p)), sink);
                return None;
            }
        }
        let progressive = self.progressive;
        let session = self.ensure_session(waker);
        let ticket = if progressive {
            session.open_at(request.source, request.name, typo, target)
        } else {
            session.open(request.source, request.name, typo)
        };
        let opening = Opening {
            ticket,
            name,
            file,
            page,
            place,
            spine,
            pages,
            book,
        };
        let had_old = self.panes.contains_key(&pane);
        self.set_slot(pane, Some(Slot::Opening(opening)), sink);
        if had_old {
            sink.closed(pane);
        }
        None
    }

    /// Moves `pane` to `page`, clamped to its document's page count.
    pub fn go_to(&mut self, pane: PaneId, page: usize) -> Option<usize> {
        let count = self.page_count(pane)?;
        let place = self.place_mut(pane)?;
        place.go_to(page, count);
        Some(place.current)
    }

    /// Turns `pane` one step in its view mode and returns the landing page.
    pub fn turn(&mut self, pane: PaneId, forward: bool) -> Option<usize> {
        let count = self.page_count(pane)?;
        let place = self.place_mut(pane)?;
        let to = place.turned(forward, count);
        place.go_to(to, count);
        Some(to)
    }

    /// Changes `pane`'s view mode; true when the mode actually changed.
    pub fn set_mode(&mut self, pane: PaneId, mode: ViewMode) -> bool {
        let Some(place) = self.place_mut(pane) else {
            return false;
        };
        if place.mode == mode {
            return false;
        }
        place.mode = mode;
        true
    }

    pub fn close_pane(&mut self, pane: PaneId, sink: &mut dyn Sink) {
        self.set_slot(pane, None, sink);
        self.sheets.remove(&pane);
        sink.closed(pane);
    }

    /// Reopens selected panes at page 0 after saved positions are cleared.
    pub(crate) fn restart_at_start(
        &mut self,
        waker: &Waker,
        panes: &[PaneId],
        sink: &mut dyn Sink,
    ) -> Vec<Toast> {
        let requests: Vec<(PaneId, OpenRequest)> = panes
            .iter()
            .filter_map(|&pane| {
                let mut request = OpenRequest::path(self.path(pane)?.to_path_buf());
                request.pane = Some(pane);
                request.book = self.book(pane);
                Some((pane, request))
            })
            .collect();
        for (pane, _) in &requests {
            self.close_pane(*pane, sink);
        }
        requests
            .into_iter()
            .filter_map(|(pane, request)| self.open(waker, pane, request, sink))
            .collect()
    }

    pub(super) fn set_slot(&mut self, pane: PaneId, slot: Option<Slot>, sink: &mut dyn Sink) {
        let old = match slot {
            Some(s) => self.panes.insert(pane, s),
            None => self.panes.remove(&pane),
        };
        self.asked.remove(&pane);
        self.inserts.remove(&pane);
        if let Some(Slot::Open(place)) = old {
            self.release(place.doc, sink);
        }
    }

    pub(crate) fn want_demand(&mut self, wants: impl IntoIterator<Item = Want>) {
        if let Some(s) = &mut self.session {
            s.want(wants);
        }
    }

    pub fn end_frame(&mut self) {
        self.texts.end_frame();
    }

    pub fn rest(&mut self) {
        if let Some(s) = &self.session {
            s.rest();
        }
        self.texts.rest();
    }
}
