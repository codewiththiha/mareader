//! Read-only document queries, selection, resume places, and AI views.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use mareader_library::{BookId, Spot};
use mareader_session::{
    Anchor, DocId, Document, Entry, Selection, Session as DocSession, TextLayer, Typography,
};
use mareader_workspace::PaneId;

use super::DocumentPlace;
use super::state::{Slot, State};
use crate::page_ai::view::{AiView, Views};
use crate::pane::ViewMode;
use crate::paper::Papers;
use crate::read_aloud::place::Stand;
use crate::reading::Place;
use crate::study::words;
use crate::texts::TextLayers;

/// Split-borrowed view of one pane for drawing.
pub enum PaneShow<'a> {
    Empty,
    Opening,
    Open {
        entry: &'a Entry,
        place: &'a mut Place,
        texts: &'a mut TextLayers,
        papers: &'a mut Papers,
    },
}

impl State {
    pub fn opening(&self) -> bool {
        (self.panes.values()).any(|s| matches!(s, Slot::Opening(_)))
            || self.session.as_ref().is_some_and(DocSession::opening)
    }

    pub fn title(&self, pane: PaneId) -> Option<&str> {
        match self.panes.get(&pane)? {
            Slot::Opening(o) => Some(&o.name),
            Slot::Open(place) => self
                .session
                .as_ref()?
                .get(place.doc)
                .map(|e| e.name.as_str()),
        }
    }

    pub fn path(&self, pane: PaneId) -> Option<&Path> {
        match self.panes.get(&pane)? {
            Slot::Opening(o) => o.file.as_deref(),
            Slot::Open(place) => self.session.as_ref()?.get(place.doc)?.path.as_deref(),
        }
    }

    pub fn book(&self, pane: PaneId) -> Option<BookId> {
        match self.panes.get(&pane)? {
            Slot::Opening(o) => o.book,
            Slot::Open(place) => place.book,
        }
    }

    pub fn slot(&self, pane: PaneId) -> Option<&Slot> {
        self.panes.get(&pane)
    }

    pub fn place(&self, pane: PaneId) -> Option<&Place> {
        match self.panes.get(&pane)? {
            Slot::Open(place) => Some(place),
            Slot::Opening(_) => None,
        }
    }

    pub fn place_mut(&mut self, pane: PaneId) -> Option<&mut Place> {
        match self.panes.get_mut(&pane)? {
            Slot::Open(place) => Some(place),
            Slot::Opening(_) => None,
        }
    }

    /// Borrows `pane`'s open entry, place, text layers, and papers together.
    pub fn split_pane(&mut self, pane: PaneId) -> PaneShow<'_> {
        let Some(slot) = self.panes.get_mut(&pane) else {
            return PaneShow::Empty;
        };
        match slot {
            Slot::Opening(_) => PaneShow::Opening,
            Slot::Open(place) => match self.session.as_ref().and_then(|s| s.get(place.doc)) {
                Some(entry) => PaneShow::Open {
                    entry,
                    place,
                    texts: &mut self.texts,
                    papers: &mut self.papers,
                },
                None => PaneShow::Empty,
            },
        }
    }

    pub fn entry(&self, doc: DocId) -> Option<&Entry> {
        self.session.as_ref()?.get(doc)
    }

    pub fn document(&self, doc: DocId) -> Option<&Arc<Document>> {
        self.session.as_ref()?.get(doc).map(|e| &e.doc)
    }

    pub fn document_of(&self, pane: PaneId) -> Option<Arc<Document>> {
        let doc = self.place(pane)?.doc;
        Some(Arc::clone(&self.session.as_ref()?.get(doc)?.doc))
    }

    pub fn stand(&self, pane: PaneId) -> Option<Stand> {
        let place = self.place(pane)?;
        let entry = self.session.as_ref()?.get(place.doc)?;
        Some(Stand {
            pane,
            doc: place.doc,
            document: Arc::clone(&entry.doc),
            current: place.current,
        })
    }

    pub fn typography(&self, pane: PaneId) -> Option<Arc<Typography>> {
        let place = self.place(pane)?;
        let entry = self.session.as_ref()?.get(place.doc)?;
        entry.doc.is_reflowable().then(|| Arc::clone(&entry.typo))
    }

    pub fn mode(&self, pane: PaneId) -> Option<ViewMode> {
        Some(self.place(pane)?.mode)
    }

    pub fn current(&self, pane: PaneId) -> Option<usize> {
        Some(self.place(pane)?.current)
    }

    pub fn visible(&self, pane: PaneId) -> Option<Range<usize>> {
        Some(self.place(pane)?.visible.clone())
    }

    pub fn reflowable(&self, pane: PaneId) -> bool {
        let Some(place) = self.place(pane) else {
            return false;
        };
        let entry = self.session.as_ref().and_then(|s| s.get(place.doc));
        entry.is_some_and(|e| e.doc.is_reflowable())
    }

    pub fn page_count(&self, pane: PaneId) -> Option<usize> {
        let place = self.place(pane)?;
        Some(self.session.as_ref()?.get(place.doc)?.doc.page_count())
    }

    pub fn doc_count(&self) -> usize {
        self.session.as_ref().map_or(0, DocSession::len)
    }

    pub fn pending(&self) -> usize {
        self.session.as_ref().map_or(0, DocSession::pending)
    }

    pub fn text_layers(&self) -> usize {
        self.texts.len()
    }

    /// `page`'s text layer in `pane`: held in cache, else read afresh.
    pub fn layer(&self, pane: PaneId, page: usize) -> Option<Arc<TextLayer>> {
        let doc = self.place(pane)?.doc;
        let entry = self.session.as_ref()?.get(doc)?;
        self.texts
            .get(doc, page)
            .or_else(|| entry.doc.text_layer(page).ok().map(Arc::new))
    }

    /// Selects the first `text` on `page` in `pane`; false if absent.
    pub fn select(&mut self, pane: PaneId, page: usize, text: &str) -> bool {
        let doc = self.place(pane).map(|p| p.doc);
        let entry = doc.and_then(|d| self.session.as_ref()?.get(d));
        let Some(layer) = entry.and_then(|e| e.doc.text_layer(page).ok()) else {
            return false;
        };
        let chars: Vec<char> = layer.text.chars().iter().map(|c| c.ch).collect();
        let needle: Vec<char> = text.chars().collect();
        let found = chars
            .windows(needle.len().max(1))
            .position(|w| w == needle.as_slice());
        match (found, self.panes.get_mut(&pane)) {
            (Some(at), Some(Slot::Open(place))) => {
                place.selection = Some(Selection::within(page, at..at + needle.len()));
                true
            }
            _ => false,
        }
    }

    pub fn selection(&self, pane: PaneId) -> Option<Selection> {
        self.place(pane)?.selection.filter(|s| !s.is_empty())
    }

    pub fn clear_selection(&mut self, pane: PaneId) -> bool {
        match self.panes.get_mut(&pane) {
            Some(Slot::Open(place)) => place.selection.take().is_some_and(|s| !s.is_empty()),
            _ => false,
        }
    }

    /// The pane's selection as a highlight's spot and text.
    pub fn highlighted(&self, pane: PaneId) -> Option<(Spot, String)> {
        let sel = self.selection(pane)?;
        let layer = |page| self.layer(pane, page);
        let spot = words::span(sel, layer)?;
        Some((spot, sel.text(layer)))
    }

    /// Resume data for the open file in `pane`.
    pub fn resume_place(
        &self,
        pane: PaneId,
        view_anchor: impl FnOnce(&Place) -> Option<Anchor>,
    ) -> Option<DocumentPlace> {
        match self.panes.get(&pane)? {
            Slot::Opening(o) => Some(DocumentPlace {
                path: o.file.clone()?,
                page: o.page,
                anchor: o.place,
                spine: o.spine.clone(),
                pages: o.pages,
            }),
            Slot::Open(place) => {
                let entry = self.session.as_ref()?.get(place.doc)?;
                let anchor = view_anchor(place).or_else(|| entry.doc.anchor(place.current));
                let spine = anchor.and_then(|at| entry.doc.spine_place(at));
                let legacy = entry.doc.is_complete().then_some(anchor).flatten();
                Some(DocumentPlace {
                    path: entry.path.clone()?,
                    page: entry.doc.global_page(place.current),
                    anchor: legacy,
                    spine,
                    pages: entry.doc.global_page_count(),
                })
            }
        }
    }
}

impl Views for State {
    fn ai_view(&self, pane: PaneId) -> Option<AiView> {
        let place = self.place(pane)?;
        let entry = self.session.as_ref()?.get(place.doc)?;
        let typo = match entry.doc.is_reflowable() {
            true => Arc::clone(&entry.typo),
            false => Arc::clone(self.typo(pane)),
        };
        let layout = entry.doc.layout().unwrap_or(place.doc.get() | 1 << 63);
        Some(AiView {
            doc: place.doc,
            layout,
            document: Arc::clone(&entry.doc),
            typo,
            visible: place.visible.clone(),
            current: place.current,
            zoom: place.zoom,
        })
    }

    fn text_layer(&self, doc: DocId, page: usize) -> Option<Arc<TextLayer>> {
        self.texts.get(doc, page)
    }
}
