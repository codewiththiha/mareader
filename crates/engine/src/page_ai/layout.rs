//! Page-AI layout indexing, reflow inserts, and span helpers on [`Ledger`].

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use mareader_library::{AiPageId, AiTask, BookId, Library, PageSpan};
use mareader_session::{Anchor, Document, Insert, Inserts};
use mareader_study::page::Rewritten;
use mareader_workspace::PaneId;

use super::Slot;
use super::extract;
use super::ledger::Ledger;
use super::state::{Index, Key};
use super::view::{AiView, Views};
use crate::stable_hash::hash;

/// Results kept laid out for a book's pages before they are made again.
const MAX_MADE: usize = 64;

/// A kept span's end, as a place in the book.
pub(super) fn anchor([block, offset]: [usize; 2]) -> Anchor {
    Anchor { block, offset }
}

/// The own page of a book showing `at`.
pub fn base_page(document: &Document, at: [usize; 2]) -> Option<usize> {
    match document {
        Document::Epub { book, .. } => Some(book.base_locate(anchor(at))),
        Document::Pdf(_) => None,
    }
}

/// The own pages a span covers, first and last.
pub fn pages_of(document: &Document, span: &PageSpan) -> Option<(usize, usize)> {
    match (document, span) {
        (_, PageSpan::Page { page }) => Some((*page, *page)),
        (_, PageSpan::Pages { from, to }) => Some((*from, *to)),
        (Document::Epub { book, .. }, PageSpan::Blocks { from, to }) => {
            let first = book.base_locate(anchor(*from));
            // An end is just past its last character, maybe on the next page.
            let [block, offset] = *to;
            let end = Anchor {
                block,
                offset: offset.saturating_sub(1),
            };
            Some((first, book.base_locate(end).max(first)))
        }
        (Document::Pdf(_), PageSpan::Blocks { .. }) => None,
    }
}

impl Ledger {
    /// A kept result, read once.
    pub fn result(&mut self, main: &Library, id: AiPageId) -> Option<Arc<Rewritten>> {
        if let Some(r) = self.parsed.get(&id) {
            return r.clone();
        }
        let page = self.saved(main, id)?;
        let read = serde_json::from_str::<Rewritten>(&page.result)
            .ok()
            .map(Arc::new);
        self.parsed.insert(id, read.clone());
        read
    }

    /// Each result's page in `view`'s layout, read again on change.
    pub fn index(
        &mut self,
        main: &Library,
        view: &AiView,
        (book, name): (Option<BookId>, &str),
    ) -> Arc<HashMap<usize, Vec<AiPageId>>> {
        let (lib, id) = self.lib(main, book, name);
        let stamp = (lib.ai_pages_stamp(), id);
        let kept = self.index.get(&view.layout);
        if let Some(index) = kept.filter(|i| i.stamp == stamp) {
            return Arc::clone(&index.pages);
        }
        let pages = lib.ai_pages(id);
        let mut h = DefaultHasher::new();
        pages.iter().for_each(|p| p.id.hash(&mut h));
        let key = h.finish();
        let found = self.index.get(&view.layout);
        if found.is_some_and(|i| i.key == key) {
            let index = self.index.get_mut(&view.layout);
            return index
                .map(|i| {
                    i.stamp = stamp;
                    Arc::clone(&i.pages)
                })
                .unwrap_or_default();
        }
        let mut map: HashMap<usize, Vec<AiPageId>> = HashMap::new();
        let mut spans = Vec::new();
        let mut covers = Vec::new();
        for p in &pages {
            match p.span {
                PageSpan::Blocks { from, to } => spans.push((p.id, (anchor(from), anchor(to)))),
                PageSpan::Page { page } => covers.push((p.id, (page, page))),
                PageSpan::Pages { from, to } => covers.push((p.id, (from, to))),
            }
            let page = match p.span {
                PageSpan::Page { page } | PageSpan::Pages { from: page, .. } => Some(page),
                PageSpan::Blocks { from, .. } => base_page(&view.document, from),
            };
            if let Some(page) = page.filter(|p| *p < view.document.base_count()) {
                map.entry(page).or_default().push(p.id);
            }
        }
        if self.index.len() > 8 {
            self.index.clear();
            self.spans.clear();
            self.covers.clear();
        }
        self.spans.extend(spans);
        self.covers.extend(covers);
        let pages = Arc::new(map);
        let index = Index {
            key,
            stamp,
            pages: Arc::clone(&pages),
        };
        self.index.insert(view.layout, index);
        pages
    }

    /// Groups shown over their pages: first and last own page, and the slot.
    pub fn spread_groups(
        &self,
        main: &Library,
        pane: PaneId,
        view: &AiView,
    ) -> Vec<(usize, usize, Slot)> {
        // A book lays kept groups in; only runs still writing spread.
        if view.document.is_reflowable() {
            return self
                .showing
                .iter()
                .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
                .filter(|(_, s)| self.carded(pane, view, s.slot))
                .filter_map(|(_, s)| {
                    let Slot::Run(id) = s.slot else {
                        return None;
                    };
                    let run = self.runs.iter().find(|r| r.id == id)?;
                    (run.last > run.page).then_some((run.page, run.last, s.slot))
                })
                .collect();
        }
        let span = |slot: Slot| match slot {
            Slot::Saved(id) => self.saved(main, id).map(|p| p.span.clone()),
            Slot::Run(id) => self
                .runs
                .iter()
                .find(|r| r.id == id)
                .map(|r| r.span.clone()),
        };
        self.showing
            .iter()
            .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
            .filter_map(|(_, s)| match span(s.slot)? {
                PageSpan::Pages { from, to } => Some((from, to, s.slot)),
                _ => None,
            })
            .collect()
    }

    /// Whether `slot` shows as a card: a book's, until laid in.
    pub fn carded(&self, pane: PaneId, view: &AiView, slot: Slot) -> bool {
        if !view.document.is_reflowable() {
            return true;
        }
        let id = match slot {
            Slot::Saved(id) => id,
            // A redo writes behind the laid result; its bubble turns.
            Slot::Run(run) => {
                let run = self.runs.iter().find(|r| r.id == run);
                match run.and_then(|r| r.redo_of) {
                    Some(old) if view.document.shows(old.0) => return false,
                    _ => return true,
                }
            }
        };
        let asked = self.wants.get(&pane).map_or(0, |w| w.key);
        let laid = view.document.inserts_key() == asked;
        !laid && !view.document.shows(id.0)
    }

    /// Re-counts `pane`'s shown results once new type moves its pages.
    pub fn align(
        &mut self,
        main: &Library,
        pane: PaneId,
        view: &AiView,
        who: (Option<BookId>, &str),
    ) {
        let old = self.aligned.insert(pane, view.layout);
        if old.is_none_or(|o| o == view.layout) {
            return;
        }
        let (lib, book) = self.lib(main, who.0, who.1);
        let page_of = |span: &PageSpan| match span {
            PageSpan::Page { page } | PageSpan::Pages { from: page, .. } => Some(*page),
            PageSpan::Blocks { from, .. } => base_page(&view.document, *from),
        };
        let moved: Vec<(Key, Option<usize>)> = self
            .showing
            .iter()
            .filter(|((p, l, _), _)| *p == pane && *l != view.layout)
            .map(|(key, shown)| {
                let page = match shown.slot {
                    Slot::Saved(id) => {
                        let p = lib.ai_page(id).filter(|p| p.book.value == book);
                        p.and_then(|p| page_of(&p.span))
                    }
                    Slot::Run(id) => self
                        .runs
                        .iter()
                        .find(|r| r.id == id && r.book == who.0 && r.name == who.1)
                        .and_then(|r| page_of(&r.span)),
                };
                (*key, page)
            })
            .collect();
        for (key, page) in moved {
            let Some(shown) = self.showing.remove(&key) else {
                continue;
            };
            let Some(page) = page else {
                continue;
            };
            if let Slot::Run(id) = shown.slot
                && let Some(r) = self.runs.iter_mut().find(|r| r.id == id)
            {
                (r.layout, r.page) = (view.layout, page);
            }
            self.showing.insert((pane, view.layout, page), shown);
        }
    }

    /// The kept results `pane`'s book lays in; none for a PDF.
    pub fn inserts(
        &mut self,
        main: &Library,
        pane: PaneId,
        documents: &dyn Views,
        who: (Option<BookId>, &str),
    ) -> Option<Arc<Inserts>> {
        let view = documents.ai_view(pane)?;
        let Document::Epub { book, .. } = &*view.document else {
            self.wants.remove(&pane);
            return None;
        };
        self.prune(main);
        self.align(main, pane, &view, who);
        // A redo keeps the result it redoes in the pages until done.
        let mut ids: Vec<AiPageId> = self
            .showing
            .iter()
            .filter(|((p, l, _), _)| *p == pane && *l == view.layout)
            .filter_map(|(_, s)| match s.slot {
                Slot::Saved(id) => Some(id),
                Slot::Run(id) => self.runs.iter().find(|r| r.id == id)?.redo_of,
            })
            .collect();
        ids.sort_unstable();
        ids.dedup();
        let key = match ids.is_empty() {
            true => 0,
            false => hash(("inserts", &ids)).max(1),
        };
        if let Some(w) = self.wants.get(&pane).filter(|w| w.key == key) {
            return Some(Arc::clone(w));
        }
        let mut list = Vec::new();
        for id in ids {
            if let Some(made) = self.made.get(&id) {
                list.push(Arc::clone(made));
                continue;
            }
            let Some(p) = self.saved(main, id) else {
                continue;
            };
            let (task, span) = (p.task.clone(), p.span.clone());
            let PageSpan::Blocks { from, to } = span else {
                continue;
            };
            let Some(result) = self.result(main, id) else {
                continue;
            };
            let source = extract::book(book.blocks(), [from, to]);
            let blocks = extract::blocks(&result, &source, book.blocks());
            let language = match task {
                AiTask::Translate { language } => Some(language),
                _ => None,
            };
            let made = Arc::new(Insert {
                id: id.0,
                from: anchor(from),
                to: anchor(to),
                blocks: Arc::new(blocks),
                language,
            });
            if self.made.len() > MAX_MADE {
                self.made.clear();
            }
            self.made.insert(id, Arc::clone(&made));
            list.push(made);
        }
        let inserts = Arc::new(Inserts { key, list });
        self.wants.insert(pane, Arc::clone(&inserts));
        Some(inserts)
    }
}
