//! Front-end page textures, GPU upload, and per-pane `PdfView` state.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use egui::{Context, Rect, Ui, Vec2};
use mareader_appearance::Rgb;
use mareader_engine::documents::{DocumentPlace, PaneShow, Sink, State, UPLOADS};
use mareader_engine::memory::give_back;
use mareader_engine::pane::{Arrow, ViewMode, ZoomCommand};
use mareader_engine::reading::Place;
use mareader_engine::reading_place;
use mareader_engine::session::{PaneDriver, Session};
use mareader_engine::study::words::{self, Asked};
use mareader_engine::texts::TextLayers;
use mareader_engine::triggers::ClipAction;
use mareader_library::{BookId, Spot};
use mareader_pdf::Bounds;
use mareader_session::{
    Anchor, DocId, Document, Entry, PageKey, Raster, RenderError, Selection, Typography, Want,
};
use mareader_settings::PaperArea;
use mareader_workspace::PaneId;

use crate::gpu::{Gpu, Quad};
use crate::loading_mark;
use crate::pages::{PageCache, PageTexture};
use crate::pdf_view::{self, Demand, PdfView, Sinks, Urgency, ViewContext};
use crate::selection::Sheet;
use crate::study::Gloss;
use crate::theme::Tokens;
use crate::thumbnails::{self, ThumbState};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::zoom::ZoomState;

/// One pane's document as navigation sees it, as of last frame.
#[derive(Clone, Debug)]
pub struct Reading {
    pub pane: PaneId,
    pub doc: DocId,
    pub document: Arc<Document>,
    pub current: usize,
    pub mode: ViewMode,
    pub progress: f32,
    pub name: String,
    /// The page read, on screen last frame.
    pub page: Option<Rect>,
}

impl Reading {
    pub fn count(&self) -> usize {
        self.document.display_page_count()
    }

    pub fn global_current(&self) -> usize {
        self.document.display_page(self.current)
    }
}

/// A count of what the documents hold; see [`DocumentsView::meter`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Meter {
    pub documents: usize,
    pub pages: usize,
    /// Parts of pages too big for one texture.
    pub tiles: usize,
    /// The rasters' bytes, as the cache counts them against its cap.
    pub page_bytes: usize,
    pub texts: usize,
    /// Jobs queued or running.
    pub jobs: usize,
}

/// Seconds without input or work before caches shrink to the pages in view.
const REST_AFTER: f64 = 3.0;
/// Air above the line read when a book set anew lands on it.
const LAND_AIR: f32 = 8.0;

/// Front-end ephemeral state for open document panes: textures, views, GPU.
pub struct DocumentsUi {
    pages: PageCache,
    views: BTreeMap<PaneId, Box<PdfView>>,
    demand: Demand,
    gpu: Option<Gpu>,
    drawn: Vec<Quad>,
    /// What each pane's pages told the study tools this frame.
    gloss: Vec<(PaneId, Gloss)>,
    /// When input or work last came, and whether caches shrank since.
    busy_at: f64,
    rested: bool,
}

impl Default for DocumentsUi {
    fn default() -> Self {
        Self {
            pages: PageCache::default(),
            views: BTreeMap::new(),
            demand: Demand::new(),
            gpu: None,
            drawn: Vec::new(),
            gloss: Vec::new(),
            busy_at: 0.0,
            rested: true,
        }
    }
}

/// Combined read-only view over engine document state and UI-side views.
#[derive(Clone, Copy)]
pub struct DocumentsView<'a> {
    state: &'a State,
    ui: &'a DocumentsUi,
}

impl<'a> DocumentsView<'a> {
    pub fn new(state: &'a State, ui: &'a DocumentsUi) -> Self {
        Self { state, ui }
    }

    pub(crate) fn pane_view(&self, pane: PaneId) -> Option<&'a PdfView> {
        self.ui.views.get(&pane).map(Box::as_ref)
    }

    pub fn opening(&self) -> bool {
        self.state.opening()
    }

    pub fn title(&self, pane: PaneId) -> Option<&'a str> {
        self.state.title(pane)
    }

    pub fn path(&self, pane: PaneId) -> Option<&'a Path> {
        self.state.path(pane)
    }

    pub fn book(&self, pane: PaneId) -> Option<BookId> {
        self.state.book(pane)
    }

    pub fn typography(&self, pane: PaneId) -> Option<Arc<Typography>> {
        self.state.typography(pane)
    }

    pub fn document(&self, doc: DocId) -> Option<&'a Arc<Document>> {
        self.state.document(doc)
    }

    pub fn mode(&self, pane: PaneId) -> Option<ViewMode> {
        self.state.mode(pane)
    }

    pub fn current(&self, pane: PaneId) -> Option<usize> {
        self.state.current(pane)
    }

    pub fn visible(&self, pane: PaneId) -> Option<Range<usize>> {
        self.state.visible(pane)
    }

    pub fn reflowable(&self, pane: PaneId) -> bool {
        self.state.reflowable(pane)
    }

    pub fn page_count(&self, pane: PaneId) -> Option<usize> {
        self.state.page_count(pane)
    }

    pub fn text_layers(&self) -> usize {
        self.state.text_layers()
    }

    pub fn selection(&self, pane: PaneId) -> Option<Selection> {
        self.state.selection(pane)
    }

    pub fn gpu(&self) -> Option<&'a Gpu> {
        self.ui.gpu.as_ref()
    }

    pub fn drifting(&self, pane: PaneId) -> Option<bool> {
        self.state.place(pane)?;
        Some(self.pane_view(pane)?.drifting())
    }

    pub fn zoom_state(&self, pane: PaneId) -> Option<ZoomState> {
        self.state.place(pane)?;
        Some(self.pane_view(pane)?.zoom.state())
    }

    pub fn offset(&self, pane: PaneId) -> Option<Vec2> {
        self.pane_view(pane).map(PdfView::offset)
    }

    pub fn sheets(&self, pane: PaneId) -> &'a [Sheet] {
        self.pane_view(pane).map_or(&[], |v| v.sheets.as_slice())
    }

    pub fn read_marker(&self, pane: PaneId) -> Option<mareader_speech::Spot> {
        let m = self.pane_view(pane)?.marker.as_ref()?;
        (!m.leaving()).then_some(m.spot)
    }

    pub fn paper(&self, pane: PaneId, area: PaperArea) -> Option<Rgb> {
        let place = self.state.place(pane)?;
        let view = self.pane_view(pane)?;
        self.state.papers.at(place.doc, area, view.paper_at(place))
    }

    pub fn reading(&self, pane: PaneId) -> Option<Reading> {
        let place = self.state.place(pane)?;
        let entry = self.state.entry(place.doc)?;
        let page = self
            .pane_view(pane)
            .and_then(|v| v.sheets.iter().find(|s| s.page == place.current))
            .map(|s| s.rect);
        Some(Reading {
            pane,
            doc: place.doc,
            document: Arc::clone(&entry.doc),
            current: place.current,
            mode: place.mode,
            progress: place.progress,
            name: entry.name.clone(),
            page,
        })
    }

    pub fn place_read(&self, pane: PaneId) -> Option<Anchor> {
        let view = self.pane_view(pane)?;
        let place = self.state.place(pane)?;
        let doc = self.state.document(place.doc).map(Arc::as_ref);
        reading_place(view, place, doc, &self.state.texts, place.doc)
    }

    pub fn place(&self, pane: PaneId) -> Option<DocumentPlace> {
        self.state
            .resume_place(pane, |place| self.pane_view(pane)?.place(place))
    }

    pub fn meter(&self) -> Meter {
        Meter {
            documents: self.state.doc_count(),
            pages: self.ui.pages.len(),
            tiles: self.ui.pages.tiles(),
            page_bytes: self.ui.pages.bytes(),
            texts: self.state.text_layers(),
            jobs: self.state.pending(),
        }
    }

    pub fn stats(&self) -> (usize, usize) {
        (self.state.doc_count(), self.ui.pages.len())
    }

    pub fn asked_at(&self, pane: PaneId, page: usize, chars: Range<usize>) -> Option<Asked> {
        let layer = self.state.texts.get(self.state.place(pane)?.doc, page)?;
        words::asked_at(&layer, page, chars)
    }

    pub fn spans_rect(&self, pane: PaneId, spans: &[(usize, Range<usize>)]) -> Option<Rect> {
        spans
            .iter()
            .filter_map(|(page, range)| self.hit_box(pane, *page, range.clone()))
            .reduce(|a, b| a.union(b))
    }

    pub fn highlighted(&self, pane: PaneId) -> Option<(Spot, String)> {
        self.state.highlighted(pane)
    }

    pub fn hit_box(&self, pane: PaneId, page: usize, chars: Range<usize>) -> Option<Rect> {
        let view = self.pane_view(pane)?;
        let sheet = view.sheets.iter().find(|s| s.page == page)?;
        let layer = self.state.layer(pane, page)?;
        let spot = layer.text.line_boxes(chars).reduce(Bounds::union)?;
        Some(sheet.to_screen(spot))
    }

    pub fn menu_pin(
        &self,
        pane: PaneId,
        page: usize,
        at: egui::Pos2,
        caret: Option<usize>,
    ) -> Option<crate::page_ai::Pin> {
        let view = self.pane_view(pane)?;
        let doc = self.state.place(pane)?.doc;
        let sheet = view.sheets.iter().find(|s| s.page == page)?;
        let (x, y) = sheet.to_page(at);
        let layer = self.state.texts.get(doc, page);
        let span = (layer.as_ref().zip(caret))
            .and_then(|(l, c)| crate::read_aloud::marker::word_span(&l.text, c));
        let word = (layer.as_ref().zip(span.clone()))
            .and_then(|(l, s)| l.text.line_boxes(s).reduce(Bounds::union));
        let text = (layer.as_ref().zip(span)).map(|(l, s)| words::text_of(l, s));
        Some(crate::page_ai::Pin {
            point: egui::pos2(x, y),
            word,
            text,
        })
    }

    pub fn scripts_idle(&self, pane: PaneId) -> bool {
        let Some(view) = self.pane_view(pane) else {
            return false;
        };
        let paged = &view.triggers.script;
        !paged.pages.is_empty() && paged.asks.is_empty()
    }

    pub fn scripts_state(&self, pane: PaneId) -> String {
        let Some(view) = self.pane_view(pane) else {
            return "no book in the pane".into();
        };
        let p = &view.triggers.script;
        let (pages, asks, changes) = (p.pages.len(), p.asks.len(), p.changes);
        format!(
            "{changes} changes, {pages} pages, {asks} asks; {:?}",
            p.errors
        )
    }
}

struct UiSink<'a> {
    ui: &'a mut DocumentsUi,
    ctx: Option<&'a Context>,
}

impl Sink for UiSink<'_> {
    fn opened(
        &mut self,
        pane: PaneId,
        place: &mut Place,
        page: usize,
        count: usize,
        anchor: Option<Anchor>,
    ) {
        let mut view = PdfView::new();
        if page > 0 {
            view.go_to(place, page, count);
        }
        view.hold_place(place, anchor);
        place.zoom = view.zoom.sharp();
        self.ui.views.insert(pane, Box::new(view));
    }

    fn closed(&mut self, pane: PaneId) {
        self.ui.views.remove(&pane);
    }

    fn rendered(&mut self, key: PageKey, result: Result<Raster, RenderError>) {
        match result {
            Ok(raster) => {
                let texture = match &self.ui.gpu {
                    Some(gpu) => gpu.upload(&raster).map(PageTexture::Gpu),
                    None => self.ctx.map(|ctx| PageTexture::egui(ctx, key, &raster)),
                };
                match texture {
                    Some(t) => self.ui.pages.insert(key, t, raster.bytes()),
                    None => self.ui.pages.fail(key),
                }
            }
            Err(_) => self.ui.pages.fail(key),
        }
    }

    fn forget_pages(&mut self, doc: DocId) {
        self.ui.pages.forget(doc);
    }

    fn adopt_pages(&mut self, old: DocId, new: DocId, was: &Document, is: &Document) {
        self.ui
            .pages
            .adopt(old, new, |page| was.same_page(page, is));
    }

    fn reloaded_fixed(&mut self, pane: PaneId, place: &mut Place, new: DocId, count: usize) {
        if let Some(view) = self.ui.views.get_mut(&pane) {
            if !view.respliced(place, new, count, |p| (p < count).then_some(p)) {
                view.retypeset(place, new, count, count, None);
            }
        } else {
            place.doc = new;
            place.selection = None;
            place.go_to(place.current, count);
        }
    }

    fn retarget(
        &mut self,
        pane: PaneId,
        place: &mut Place,
        old: DocId,
        new: DocId,
        was: Option<&Document>,
        is: Option<&Document>,
        texts: &TextLayers,
        said: Option<(PaneId, Anchor)>,
    ) {
        let Some(view) = self.ui.views.get_mut(&pane) else {
            return;
        };
        let count = is.map_or(0, Document::page_count);
        if let (Some(was), Some(is)) = (was, is)
            && view.respliced(place, new, count, |p| was.same_page(p, is))
        {
            return;
        }
        let said = said.filter(|(p, _)| *p == pane).map(|(_, a)| a);
        let heard = said.filter(|a| {
            was.and_then(|d| d.locate(*a))
                .is_some_and(|p| place.visible.contains(&p))
        });
        let old_place = heard.or_else(|| reading_place(view, place, was, texts, old));
        let stable = old_place.and_then(|at| was.and_then(|d| d.spine_place(at)));
        let mapped_place = stable
            .as_ref()
            .and_then(|at| is.and_then(|d| d.anchor_spine(at)))
            .or_else(|| {
                was.is_some_and(Document::is_complete)
                    .then_some(old_place)
                    .flatten()
            });
        let page = stable
            .as_ref()
            .and_then(|at| is.and_then(|d| d.locate_spine(at)))
            .or_else(|| mapped_place.and_then(|at| is?.locate(at)));
        view.retypeset(
            place,
            new,
            page.unwrap_or(place.current),
            count,
            mapped_place,
        );
        if let (Some(doc), Some(at), Some(page)) = (is, mapped_place, page)
            && let Ok(layer) = doc.text_layer(page)
            && let Some(y) = reading_place::top_of(&layer, at)
        {
            let band = doc.bands().and_then(|b| b.get(page)).map_or(0.0, |b| b.y);
            let air = if heard.is_some() {
                pdf_view::FIND_CLEARANCE
            } else {
                LAND_AIR
            };
            view.land(place, page, y - band, air);
        }
    }
}

impl PaneDriver for DocumentsUi {
    fn go_to(&mut self, pane: PaneId, place: &mut Place, page: usize, count: usize) {
        if let Some(view) = self.views.get_mut(&pane) {
            view.go_to(place, page, count);
        } else {
            place.go_to(page, count);
        }
    }

    fn reveal_said(
        &mut self,
        pane: PaneId,
        place: &mut Place,
        entry: &Entry,
        texts: &TextLayers,
        page: usize,
        chars: Range<usize>,
        clear: f32,
    ) -> bool {
        let Some(view) = self.views.get_mut(&pane) else {
            return true;
        };
        if view.gliding() {
            return false;
        }
        bring_in_view(view, place, entry, texts, page, chars, clear)
    }
}

impl DocumentsUi {
    pub fn as_view<'a>(&'a self, state: &'a State) -> DocumentsView<'a> {
        DocumentsView::new(state, self)
    }

    fn sink<'a>(&'a mut self, ctx: Option<&'a Context>) -> UiSink<'a> {
        UiSink { ui: self, ctx }
    }

    pub fn view_anchor(&self, pane: PaneId, place: &Place) -> Option<Anchor> {
        self.views.get(&pane)?.place(place)
    }

    pub fn set_gpu(&mut self, gpu: Gpu) {
        self.gpu = Some(gpu);
    }

    pub fn gpu(&self) -> Option<&Gpu> {
        self.gpu.as_ref()
    }

    pub fn take_drawn(&mut self) -> Vec<Quad> {
        std::mem::take(&mut self.drawn)
    }

    pub fn take_gloss(&mut self) -> Vec<(PaneId, Gloss)> {
        std::mem::take(&mut self.gloss)
    }

    pub fn on_opened(
        &mut self,
        session: &mut Session,
        pane: PaneId,
        page: usize,
        count: usize,
        anchor: Option<Anchor>,
    ) {
        session.with_document_place(pane, |place, _| {
            let mut view = PdfView::new();
            if page > 0 {
                view.go_to(place, page, count);
            }
            view.hold_place(place, anchor);
            place.zoom = view.zoom.sharp();
            self.views.insert(pane, Box::new(view));
        });
    }

    pub fn on_closed(&mut self, pane: PaneId) {
        self.views.remove(&pane);
    }

    pub fn forget_pages(&mut self, doc: DocId) {
        self.pages.forget(doc);
    }

    pub fn sync_text(&mut self, session: &mut Session) {
        session.sync_documents_text(&mut self.sink(None));
    }

    pub fn set_inserts(
        &mut self,
        session: &mut Session,
        pane: PaneId,
        inserts: Arc<mareader_session::Inserts>,
    ) {
        session.set_document_inserts(pane, inserts, &mut self.sink(None));
    }

    pub fn receive(&mut self, ctx: &Context, session: &mut Session, out: &mut Commands) {
        for toast in session.poll_documents(UPLOADS, &mut self.sink(Some(ctx))) {
            out.push(Sent::Ui(UiCommand::Notify(toast)));
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        pane: PaneId,
        cx: &ViewContext,
        use_vocab: bool,
    ) -> bool {
        let mut sheet_moved = None;
        let shown = session.show_document_pane(pane, |shown, vocab| match shown {
            PaneShow::Empty => false,
            PaneShow::Opening => {
                opening(ui, &cx.tokens);
                true
            }
            PaneShow::Open {
                entry,
                place,
                texts,
                papers,
            } => {
                let Some(view) = self.views.get_mut(&pane) else {
                    return false;
                };
                let mut gloss = Vec::new();
                let io = Sinks {
                    pages: &mut self.pages,
                    demand: &mut self.demand,
                    texts,
                    drawn: &mut self.drawn,
                    pass: self.gpu.as_ref().map(|g| &g.pass),
                    gloss: &mut gloss,
                    vocab: if use_vocab { Some(vocab) } else { None },
                };
                pdf_view::show(ui, view, place, &entry.doc, io, cx);
                let moved = std::mem::take(&mut view.sheet_moved);
                if moved {
                    sheet_moved = Some(view.sheet());
                }
                self.gloss.extend(gloss.into_iter().map(|g| (pane, g)));
                if cx.sample {
                    let count = entry.doc.page_count();
                    for page in papers.wanted(place.doc, place.visible.clone(), count) {
                        let want = Want::Paper {
                            doc: place.doc,
                            page,
                        };
                        self.demand.push((Urgency::Paper, want));
                    }
                }
                true
            }
        });
        if let Some(sheet) = sheet_moved {
            session.set_document_sheet(pane, sheet, &mut self.sink(None));
        }
        shown
    }

    pub fn thumbnails(
        &mut self,
        ui: &mut Ui,
        reading: Option<&Reading>,
        texts: &mut TextLayers,
        state: &mut ThumbState,
        recentre: bool,
        out: &mut Commands,
    ) -> bool {
        let Some(reading) = reading else {
            state.forget_book();
            return false;
        };
        let io = Sinks {
            pages: &mut self.pages,
            demand: &mut self.demand,
            texts,
            drawn: &mut Vec::new(),
            pass: self.gpu.as_ref().map(|g| &g.pass),
            gloss: &mut Vec::new(),
            vocab: None,
        };
        thumbnails::show(ui, reading, io, state, recentre, out);
        true
    }

    pub fn end_frame(&mut self, ctx: &Context, session: &mut Session) {
        let mut demand = std::mem::take(&mut self.demand);
        demand.sort_by_key(|(urgency, _)| *urgency);
        session.end_document_frame(demand.into_iter().map(|(_, want)| want));
        self.settle(ctx, session);
        self.pages.end_frame();
    }

    fn settle(&mut self, ctx: &Context, session: &mut Session) {
        let (now, input) = ctx.input(|i| {
            let p = &i.pointer;
            (
                i.time,
                !i.events.is_empty() || p.is_moving() || p.any_down(),
            )
        });
        let working = session.documents().pending() > 0;
        if input || working {
            self.busy_at = now;
            self.rested = false;
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(REST_AFTER));
        } else if !self.rested && now - self.busy_at >= REST_AFTER {
            self.rested = true;
            session.rest_documents();
            self.pages.rest();
            give_back();
        }
    }

    pub fn open_transcript(&mut self, pane: PaneId, card: usize, track: Option<usize>) -> bool {
        let Some(view) = self.views.get_mut(&pane) else {
            return false;
        };
        view.transcripts.open(card, track);
        true
    }

    pub fn transcripts(
        &mut self,
        state: &State,
        pane: PaneId,
        wanted: &[String],
    ) -> Option<(u64, Vec<Arc<mareader_epub::Insert>>)> {
        let view = self.views.get_mut(&pane)?;
        if view.transcripts.is_empty() {
            return None;
        }
        let place = state.place(pane)?;
        let Document::Epub { book, .. } = state.document(place.doc)?.as_ref() else {
            return None;
        };
        Some(view.transcripts.inserts(book, wanted))
    }

    pub fn triggered(
        &mut self,
        state: &State,
        pane: PaneId,
    ) -> Option<(u64, Vec<Arc<mareader_epub::Insert>>)> {
        let view = self.views.get_mut(&pane)?;
        let place = state.place(pane)?;
        let Document::Epub { book, .. } = state.document(place.doc)?.as_ref() else {
            return None;
        };
        view.triggers.inserts(book)
    }

    pub fn scripted(
        &mut self,
        state: &State,
    ) -> Vec<(PaneId, Arc<Document>, &mut mareader_engine::scripts::Paged)> {
        self.views
            .iter_mut()
            .filter_map(|(pane, view)| {
                let place = state.place(*pane)?;
                let doc = state.document(place.doc)?;
                let book = matches!(&**doc, Document::Epub { .. });
                book.then_some((*pane, Arc::clone(doc), &mut view.triggers.script))
            })
            .collect()
    }

    pub fn pick_script(&mut self, pane: PaneId, selector: &str) -> bool {
        let Some(view) = self.views.get_mut(&pane) else {
            return false;
        };
        let paged = &mut view.triggers.script;
        let Some(&chapter) = paged.pages.keys().next() else {
            return false;
        };
        let ask = mareader_engine::scripts::Ask::Pick(selector.to_owned());
        paged.asks.push((chapter, ask));
        true
    }

    pub fn set_trigger(&mut self, pane: PaneId, target: usize, visible: bool) -> bool {
        let Some(view) = self.views.get_mut(&pane) else {
            return false;
        };
        view.triggers.set(target, visible);
        true
    }

    pub fn take_clips(&mut self, state: &State) -> Vec<(Arc<Document>, ClipAction)> {
        let mut out = Vec::new();
        for (pane, view) in &mut self.views {
            let clips = std::mem::take(&mut view.triggers.clips);
            if clips.is_empty() {
                continue;
            }
            if let Some(doc) = state.document_of(*pane) {
                out.extend(clips.into_iter().map(|c| (Arc::clone(&doc), c)));
            }
        }
        out
    }

    pub fn reveal_hit(
        &mut self,
        session: &mut Session,
        pane: PaneId,
        page: usize,
        chars: Range<usize>,
    ) -> bool {
        session.show_document_pane(pane, |shown, _| match shown {
            PaneShow::Open {
                entry,
                place,
                texts,
                ..
            } => match self.views.get_mut(&pane) {
                Some(view) => bring_in_view(view, place, entry, texts, page, chars, 0.0),
                None => true,
            },
            _ => true,
        })
    }

    pub fn place_read_marker(&mut self, pane: PaneId, spot: mareader_speech::Spot, now: f64) {
        self.dismiss_read_markers(now);
        if let Some(view) = self.views.get_mut(&pane) {
            view.marker = Some(crate::read_aloud::marker::Marker::new(spot));
        }
    }

    pub fn reveal_read_markers(&mut self, session: &mut Session) {
        let due: Vec<(PaneId, mareader_speech::Spot)> = self
            .views
            .iter()
            .filter_map(|(pane, view)| {
                view.marker
                    .as_ref()
                    .filter(|m| !m.revealed && !m.leaving())
                    .map(|m| (*pane, m.spot))
            })
            .collect();
        for (pane, spot) in due {
            let Some(doc) = session.documents().place(pane).map(|p| p.doc) else {
                continue;
            };
            let layer = session.documents().texts.get(doc, spot.page);
            let span = layer.and_then(|l| crate::read_aloud::marker::word_span(&l.text, spot.char));
            let chars = span.unwrap_or(spot.char..spot.char + 1);
            if session.reveal_pane_said(pane, spot.page, chars, 0.0, self)
                && let Some(view) = self.views.get_mut(&pane)
                && let Some(m) = &mut view.marker
            {
                m.revealed = true;
            }
        }
    }

    pub fn dismiss_read_markers(&mut self, now: f64) -> bool {
        let mut any = false;
        for view in self.views.values_mut() {
            if let Some(m) = &mut view.marker {
                any |= m.dismiss(now);
            }
        }
        any
    }

    pub fn zoom(&mut self, session: &mut Session, pane: PaneId, command: ZoomCommand, now: f64) {
        let Some(view) = self.views.get_mut(&pane) else {
            return;
        };
        session.with_document_place(pane, |place, _| view.command(place, command, now));
    }

    pub fn apply_mode(&mut self, session: &mut Session, pane: PaneId, mode: ViewMode, now: f64) {
        let Some(view) = self.views.get_mut(&pane) else {
            return;
        };
        session.with_document_place(pane, |place, _| view.apply_mode(place, mode, now));
    }

    pub fn arrow(&mut self, session: &mut Session, pane: PaneId, arrow: Arrow, now: f64) {
        let Some(view) = self.views.get_mut(&pane) else {
            return;
        };
        session.with_document_place(pane, |place, count| view.arrow(place, arrow, count, now));
    }

    pub fn page_key(&mut self, session: &mut Session, pane: PaneId, forward: bool) {
        let Some(view) = self.views.get_mut(&pane) else {
            return;
        };
        session.with_document_place(pane, |place, count| view.page_key(place, forward, count));
    }

    pub fn go_to(&mut self, session: &mut Session, pane: PaneId, page: usize) {
        let Some(view) = self.views.get_mut(&pane) else {
            return;
        };
        session.with_document_place(pane, |place, count| view.go_to(place, page, count));
    }

    pub fn toggle_drift(&mut self, session: &mut Session, pane: PaneId, now: f64) -> bool {
        let Some(view) = self.views.get_mut(&pane) else {
            return false;
        };
        session
            .with_document_place(pane, |place, _| view.toggle_drift(place, now))
            .unwrap_or(false)
    }

    pub fn stop_drift(&mut self, pane: PaneId) -> bool {
        self.views
            .get_mut(&pane)
            .is_some_and(|view| view.stop_drift())
    }
}

fn bring_in_view(
    view: &mut PdfView,
    place: &mut Place,
    entry: &Entry,
    texts: &TextLayers,
    page: usize,
    chars: Range<usize>,
    clear: f32,
) -> bool {
    let layer = texts.get(place.doc, page);
    let spot = layer.and_then(|l| l.text.line_boxes(chars).reduce(Bounds::union));
    let band = entry.doc.bands().and_then(|b| b.get(page));
    let band = band.map_or(Vec2::ZERO, |b| egui::vec2(b.x, b.y));
    view.seek(place, page, spot, band, entry.doc.page_count(), clear)
}

/// Where `view` reads in `doc`, to find once the book is set anew.
fn reading_place(
    view: &PdfView,
    place: &Place,
    doc: Option<&Document>,
    texts: &TextLayers,
    id: DocId,
) -> Option<Anchor> {
    let doc = doc?;
    if let Some(at) = view.place(place) {
        return Some(at);
    }
    let line = view.reading_line(place).and_then(|(page, y)| {
        let band = doc.bands().and_then(|b| b.get(page)).map_or(0.0, |b| b.y);
        let held = texts.get(id, page);
        let layer = held.or_else(|| doc.text_layer(page).ok().map(Arc::new))?;
        reading_place::line_at(&layer, y + band)
    });
    line.or_else(|| doc.anchor(place.current))
}

const OPENING_MARK: f32 = 48.0;

fn opening_mark_rect(rect: Rect) -> Rect {
    Rect::from_center_size(rect.center(), Vec2::splat(OPENING_MARK))
}

fn opening(ui: &mut Ui, t: &Tokens) {
    let mark = opening_mark_rect(ui.max_rect());
    loading_mark::paint(ui, mark, t.muted);
}

#[cfg(test)]
mod opening_mark_tests {
    use super::*;

    #[test]
    fn the_opening_mark_is_large_and_centered() {
        let pane = Rect::from_min_size(egui::pos2(20.0, 30.0), Vec2::new(400.0, 300.0));
        let mark = opening_mark_rect(pane);
        assert_eq!(mark.center(), pane.center());
        assert_eq!(mark.size(), Vec2::splat(OPENING_MARK));
    }
}
