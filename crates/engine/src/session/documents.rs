//! Document session, open pane places, and book queries on [`Session`].

use std::sync::Arc;

use mareader_library::BookId;
use mareader_session::{Anchor, Budget, DocId, Inserts, Want};
use mareader_settings::{BookDesign, TextSettings, WordList};
use mareader_workspace::PaneId;

use crate::documents::{OpenRequest, PaneShow, Sink, State};
use crate::event::Event;
use crate::pane::ViewMode;
use crate::reading::Place;
use crate::save::LastDocument;
use crate::texts::TextLayers;
use crate::toast::Toast;
use crate::vocab::Vocab;
use crate::word_list::Ask;

use super::Session;

/// Collects document lifecycle transitions into session [`Event`]s.
#[derive(Default)]
pub(crate) struct EventSink {
    pub events: Vec<Event>,
}

impl Sink for EventSink {
    fn opened(
        &mut self,
        pane: PaneId,
        place: &mut Place,
        page: usize,
        count: usize,
        anchor: Option<Anchor>,
    ) {
        if page > 0 {
            place.go_to(page, count);
        }
        self.events.push(Event::PaneOpened {
            pane,
            page,
            count,
            anchor,
        });
    }

    fn closed(&mut self, pane: PaneId) {
        self.events.push(Event::PaneClosed(pane));
    }

    fn forget_pages(&mut self, doc: DocId) {
        self.events.push(Event::ForgetPages(doc));
    }
}

impl Session {
    /// Engine-owned documents state, read-only.
    pub fn documents(&self) -> &State {
        &self.documents
    }

    /// Zero runs document jobs on the calling thread: deterministic, for tests.
    pub fn set_document_workers(&mut self, workers: usize) {
        self.documents.set_workers(workers);
    }

    /// The library book `pane` reads as: its own, else the book at its path.
    pub fn book_in(&self, pane: PaneId) -> Option<BookId> {
        let own = self.documents.book(pane);
        let path = self.documents.path(pane);
        own.or_else(|| self.library.model().book_at(path?))
    }

    /// Builds a word-list request for `pane`'s `pages`.
    pub fn word_ask(&self, pane: PaneId, pages: Vec<usize>, list: WordList) -> Option<Ask> {
        let document = self.documents.document_of(pane)?;
        Some(Ask {
            document,
            title: self.documents.title(pane).unwrap_or_default().to_owned(),
            pages,
            level: self.settings.study.cefr.level.band(),
            list,
        })
    }

    /// Polls inline document work when running without a front-end window.
    pub fn pump(&mut self) {
        if let Err(msg) = crate::guard::catch_panic(|| self.pump_inner()) {
            self.emit_panic("pump", &msg);
        }
    }

    fn pump_inner(&mut self) {
        let budget = Budget {
            items: 64,
            bytes: usize::MAX,
        };
        let mut sink = EventSink::default();
        let toasts = self.documents.poll(budget, None, &mut sink);
        self.events.extend(sink.events);
        for toast in toasts {
            self.emit(Event::Toast(toast));
        }
    }

    /// Starts opening `request` into `pane` and records lifecycle events.
    pub(crate) fn open_pane(&mut self, pane: PaneId, request: OpenRequest) -> Option<Toast> {
        let mut sink = EventSink::default();
        let toast = self.documents.open(&self.waker, pane, request, &mut sink);
        self.events.extend(sink.events);
        toast
    }

    /// Closes `pane`'s open or opening document and records lifecycle events.
    pub(crate) fn close_document_pane(&mut self, pane: PaneId) {
        let mut sink = EventSink::default();
        self.documents.close_pane(pane, &mut sink);
        self.events.extend(sink.events);
    }

    /// Reopens selected panes at page 0 after saved places are cleared.
    pub(crate) fn restart_documents_at_start(&mut self, panes: &[PaneId]) -> Vec<Toast> {
        let mut sink = EventSink::default();
        let toasts = self
            .documents
            .restart_at_start(&self.waker, panes, &mut sink);
        self.events.extend(sink.events);
        toasts
    }

    pub(crate) fn document_go_to(&mut self, pane: PaneId, page: usize) -> Option<usize> {
        self.documents.go_to(pane, page)
    }

    pub(crate) fn document_turn(&mut self, pane: PaneId, forward: bool) -> Option<usize> {
        self.documents.turn(pane, forward)
    }

    pub(crate) fn document_set_mode(&mut self, pane: PaneId, mode: ViewMode) -> bool {
        self.documents.set_mode(pane, mode)
    }

    /// Syncs per-pane book typography (including per-book design overrides).
    pub fn sync_documents_text(&mut self, sink: &mut dyn Sink) {
        let designed = self.book_designs();
        let text = self.settings.text.clone();
        let mut own = self.texts.own(&text);
        own.retain(|(pane, _)| designed.iter().all(|(p, _)| p != pane));
        own.extend(designed.iter().map(|(pane, t)| (*pane, t)));
        let said = self.read_aloud.said();
        self.documents.set_text(&text, &own, said, sink);
    }

    /// Per-pane text overrides from each book's saved publisher-design choice.
    pub(crate) fn book_designs(&self) -> Vec<(PaneId, TextSettings)> {
        let own = self.texts.own(&self.settings.text);
        let lib = self.library.model();
        let pane_text = |pane: PaneId| {
            let mine = own.iter().find(|(p, _)| *p == pane);
            mine.map_or(&self.settings.text, |(_, t)| *t)
        };
        let designed = |pane: PaneId| {
            let book = self.book_in(pane)?;
            let design = lib.book(book)?.design.value?;
            let book_design = match design {
                mareader_library::Design::Auto => BookDesign::Auto,
                mareader_library::Design::On => BookDesign::On,
                mareader_library::Design::Off => BookDesign::Off,
            };
            Some((
                pane,
                TextSettings {
                    book_design,
                    ..pane_text(pane).clone()
                },
            ))
        };
        self.tree.panes().into_iter().filter_map(designed).collect()
    }

    /// Updates the inserts `pane`'s reflowable book lays in.
    pub fn set_document_inserts(
        &mut self,
        pane: PaneId,
        inserts: Arc<Inserts>,
        sink: &mut dyn Sink,
    ) {
        let said = self.read_aloud.said();
        self.documents.set_inserts(pane, inserts, said, sink);
    }

    /// Updates a paged pane's settled sheet size and retypesets if needed.
    pub fn set_document_sheet(
        &mut self,
        pane: PaneId,
        sheet: Option<(f32, f32)>,
        sink: &mut dyn Sink,
    ) {
        let said = self.read_aloud.said();
        self.documents.set_sheet(pane, sheet, said, sink);
    }

    /// Polls finished document worker events and file-watch reloads.
    pub fn poll_documents(&mut self, budget: Budget, sink: &mut dyn Sink) -> Vec<Toast> {
        let said = self.read_aloud.said();
        self.documents.poll(budget, said, sink)
    }

    /// Selects the first `text` on `page` in `pane`.
    pub(crate) fn select_document_text(&mut self, pane: PaneId, page: usize, text: &str) -> bool {
        self.documents.select(pane, page, text)
    }

    /// Clears `pane`'s text selection; false if none was active.
    pub(crate) fn clear_document_selection(&mut self, pane: PaneId) -> bool {
        self.documents.clear_selection(pane)
    }

    /// Runs `f` with `pane`'s mutable reading place and page count.
    pub fn with_document_place<R>(
        &mut self,
        pane: PaneId,
        f: impl FnOnce(&mut Place, usize) -> R,
    ) -> Option<R> {
        let count = self.documents.page_count(pane).unwrap_or(0);
        let place = self.documents.place_mut(pane)?;
        Some(f(place, count))
    }

    /// Runs `f` with `pane`'s entry, place, text layers, papers, and vocab.
    pub fn show_document_pane<R>(
        &mut self,
        pane: PaneId,
        f: impl FnOnce(PaneShow<'_>, &mut Vocab) -> R,
    ) -> R {
        let shown = self.documents.split_pane(pane);
        f(shown, &mut self.vocab)
    }

    /// Borrows the library state and document text cache disjointly.
    pub fn with_rail<R>(
        &mut self,
        f: impl FnOnce(&crate::library::State, &mut TextLayers) -> R,
    ) -> R {
        f(&self.library, &mut self.documents.texts)
    }

    /// Publishes this frame's page demand and ends the text-layer frame.
    pub fn end_document_frame(&mut self, wants: impl IntoIterator<Item = Want>) {
        self.documents.want_demand(wants);
        self.documents.end_frame();
    }

    /// The file being read: the active pane's, else the first with one.
    pub fn last_document(&self) -> Option<LastDocument> {
        self.last_document_with(|_, _| None)
    }

    /// Resume info for the first open file, with a caller-supplied anchor.
    pub fn last_document_with(
        &self,
        view_anchor: impl Fn(PaneId, &Place) -> Option<Anchor>,
    ) -> Option<LastDocument> {
        let panes = std::iter::once(self.active()).chain(self.tree().panes());
        let (pane, at) = panes.into_iter().find_map(|pane| {
            let at = self
                .documents
                .resume_place(pane, |p| view_anchor(pane, p))?;
            Some((pane, at))
        })?;
        let place = at.anchor.map(|anchor| (anchor.block, anchor.offset));
        let spine = at.spine.map(|place| mareader_library::SpinePosition {
            href: place.href,
            item: place.item,
            block: place.block,
            offset: place.offset,
        });
        let book = self.documents.book(pane);
        Some(LastDocument {
            path: at.path,
            page: at.page,
            place,
            spine,
            pages: at.pages,
            book,
        })
    }

    /// Off opens EPUBs whole without waiting for a rasterized preview page.
    pub fn set_progressive_open(&mut self, on: bool) {
        self.documents.set_progressive(on);
    }

    /// Shrinks document worker and text caches while idle.
    pub fn rest_documents(&mut self) {
        self.documents.rest();
    }
}
