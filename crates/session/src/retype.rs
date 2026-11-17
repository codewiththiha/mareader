//! Progressive, cancellable EPUB retypesetting orchestration on [`Session`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mareader_epub::{Book, Inserts, OpenBook, OpenTarget};
use mareader_reflow::Typography;

use crate::pool::{Primed, RETYPESET_SETTLE, RetypesetTask};
use crate::{DocId, Document, Entry, Event, PageKey, Part, Prime, Session};

impl Document {
    pub(crate) fn can_preview_reset(&self, typo: &Typography) -> bool {
        matches!(self, Self::Epub { book, .. } if book.can_preview_reset(typo))
    }

    pub(crate) fn reset_cancellable(
        &self,
        typo: &Typography,
        inserts: Option<&Arc<Inserts>>,
        cancel: &AtomicBool,
    ) -> Option<Self> {
        self.wrap_reset(|book| book.reset_cancellable(typo, inserts, cancel))
    }

    pub(crate) fn reset_preview(
        &self,
        typo: &Typography,
        inserts: Option<&Arc<Inserts>>,
        target: &OpenTarget,
        cancel: &AtomicBool,
    ) -> Option<Self> {
        let focus = self
            .target_anchor(target)
            .or_else(|| self.local_page(target.page).and_then(|p| self.anchor(p)))
            .unwrap_or_default();
        let Self::Epub { book, .. } = self else {
            return None;
        };
        let preview = book.reset_preview(typo, inserts, focus, cancel)?;
        Some(Self::from_preview(preview, Some(target)))
    }

    pub(crate) fn wrap_reset(&self, build: impl FnOnce(&Book) -> Option<Book>) -> Option<Self> {
        let Self::Epub {
            book,
            page_offset,
            page_total,
            ..
        } = self
        else {
            return None;
        };
        let mut reset = Self::from_book(build(book)?);
        if !reset.is_complete()
            && let Self::Epub {
                page_offset: offset,
                page_total: total,
                ..
            } = &mut reset
        {
            (*offset, *total) = (*page_offset, *page_total);
        }
        Some(reset)
    }
}

pub(crate) struct Typesetting {
    pub ticket: u64,
    pub doc: DocId,
    pub typo: Arc<Typography>,
    pub inserts: Option<Arc<Inserts>>,
    pub cancel: Arc<AtomicBool>,
    pub preview_doc: Option<DocId>,
    pub continuing: bool,
}

impl Session {
    /// Sets book `id` again in `typo`; a newer request in `slot` wins.
    pub fn retypeset(&mut self, id: DocId, typo: Arc<Typography>, slot: u64) {
        self.reset_at(id, typo, None, slot, None);
    }

    /// Sets book `id` in `typo` with `inserts`; none keeps its own.
    pub fn reset(
        &mut self,
        id: DocId,
        typo: Arc<Typography>,
        inserts: Option<Arc<Inserts>>,
        slot: u64,
    ) {
        self.reset_at(id, typo, inserts, slot, None);
    }

    /// Sets book `id` in `typo`, previewing locally around `target` when large.
    pub fn reset_at(
        &mut self,
        id: DocId,
        typo: Arc<Typography>,
        inserts: Option<Arc<Inserts>>,
        slot: u64,
        target: Option<OpenTarget>,
    ) {
        self.reset_primed(id, typo, inserts, slot, (target, None));
    }

    /// Like [`Self::reset_at`], optionally priming visible pages on the worker.
    pub fn reset_primed(
        &mut self,
        id: DocId,
        typo: Arc<Typography>,
        inserts: Option<Arc<Inserts>>,
        slot: u64,
        (target, prime): (Option<OpenTarget>, Option<Prime>),
    ) {
        let Some(entry) = self.docs.get(&id) else {
            return;
        };
        let doc = Arc::clone(&entry.doc);
        let ticket = self.bump();
        let cancel = Arc::new(AtomicBool::new(false));
        let set = Typesetting {
            ticket,
            doc: id,
            typo: Arc::clone(&typo),
            inserts: inserts.clone(),
            cancel: Arc::clone(&cancel),
            preview_doc: None,
            continuing: false,
        };
        let mut paused = false;
        for opening in self.opening.values_mut() {
            if opening.doc == Some(id) {
                opening.typo = Arc::clone(&typo);
                opening.pause.store(true, Ordering::Relaxed);
                opening.ready = false;
                opening.delay = RETYPESET_SETTLE;
                paused = true;
            }
        }
        if paused {
            self.pool.pause_open();
        }
        let stale = self.typesetting.insert(slot, set).map(|prev| {
            prev.cancel.store(true, Ordering::Relaxed);
            prev.ticket
        });
        let task = RetypesetTask {
            ticket,
            id,
            doc,
            typo,
            inserts,
            target,
            prime,
            cancel,
        };
        self.pool.retypeset(task, stale);
    }

    /// Cancels any queued or running retypeset in `slot`.
    pub fn cancel_retypeset(&mut self, slot: u64) {
        if let Some(prev) = self.typesetting.remove(&slot) {
            prev.cancel.store(true, Ordering::Relaxed);
            self.pool.cancel_retypeset(prev.ticket);
            let resumes: Vec<_> = self
                .opening
                .iter()
                .filter_map(|(ticket, opening)| {
                    (opening.doc == Some(prev.doc) && opening.ready).then_some(*ticket)
                })
                .collect();
            for ticket in resumes {
                self.resume_opening(ticket);
            }
        }
    }

    pub(crate) fn has_active_retypeset(&self, doc: DocId) -> bool {
        self.typesetting.values().any(|t| t.doc == doc)
    }

    pub(crate) fn close_typesetting(&mut self, id: DocId) {
        let slots: Vec<_> = self
            .typesetting
            .iter()
            .filter_map(|(slot, t)| (t.doc == id).then_some(*slot))
            .collect();
        for slot in slots {
            self.cancel_retypeset(slot);
        }
    }

    pub(crate) fn accept_retypeset_preview(
        &mut self,
        ticket: u64,
        id: DocId,
        doc: Document,
        primed: Primed,
        out: &mut Vec<Event>,
    ) {
        let Some(slot) = self
            .typesetting
            .iter()
            .find_map(|(slot, t)| (t.ticket == ticket).then_some(*slot))
        else {
            return;
        };
        let Some(old) = self.docs.get(&id) else {
            self.typesetting.remove(&slot);
            return;
        };
        let (name, path, previous) = (old.name.clone(), old.path.clone(), Arc::clone(&old.doc));
        let Some(typo) = self.typesetting.get(&slot).map(|t| Arc::clone(&t.typo)) else {
            return;
        };
        let new = DocId(self.bump());
        self.docs.insert(
            new,
            Entry {
                doc: Arc::new(doc),
                name,
                path,
                typo,
            },
        );
        if let Some(state) = self.typesetting.get_mut(&slot) {
            state.doc = new;
            state.preview_doc = Some(new);
            state.continuing = false;
        }
        self.rebind_opening_doc(id, new);
        let has_primed = !primed.rasters.is_empty();
        out.push(Event::Retypeset {
            old: id,
            new,
            previous,
        });
        push_primed(new, primed, out);
        if has_primed {
            self.resume_retypeset(new);
        }
    }

    pub(crate) fn accept_retypeset(
        &mut self,
        ticket: u64,
        id: DocId,
        result: Option<Document>,
        primed: Primed,
        out: &mut Vec<Event>,
    ) {
        let Some(slot) = self
            .typesetting
            .iter()
            .find_map(|(slot, t)| (t.ticket == ticket).then_some(*slot))
        else {
            return;
        };
        let Some(set) = self.typesetting.remove(&slot) else {
            return;
        };
        let Some(mut doc) = result else {
            let resumes = self.mark_opening_ready(id);
            for ticket in resumes {
                self.resume_opening(ticket);
            }
            return;
        };
        for opening in self.opening.values() {
            if opening.doc == Some(id)
                && let Some(target) = opening.target.as_ref()
            {
                doc.set_preview_target(target);
            }
        }
        let Some(old) = self.docs.get(&id) else {
            return;
        };
        let entry = Entry {
            doc: Arc::new(doc),
            name: old.name.clone(),
            path: old.path.clone(),
            typo: set.typo,
        };
        let previous = Arc::clone(&old.doc);
        let new = DocId(self.bump());
        self.docs.insert(new, entry);
        let has_primed = !primed.rasters.is_empty();
        self.rebind_opening_doc(id, new);
        if has_primed {
            let resumes = self.mark_opening_ready(new);
            for ticket in resumes {
                self.resume_opening(ticket);
            }
        } else {
            let resumes: Vec<_> = self
                .opening
                .iter()
                .filter_map(|(ticket, opening)| {
                    (opening.doc == Some(new) && opening.ready).then_some(*ticket)
                })
                .collect();
            for ticket in resumes {
                self.resume_opening(ticket);
            }
        }
        out.push(Event::Retypeset {
            old: id,
            new,
            previous,
        });
        push_primed(new, primed, out);
    }

    fn rebind_opening_doc(&mut self, old: DocId, new: DocId) {
        let target_doc = self.docs.get(&new).map(|entry| &*entry.doc);
        for opening in self.opening.values_mut() {
            if opening.doc == Some(old) {
                opening.doc = Some(new);
                opening.target_page = opening
                    .target
                    .as_ref()
                    .and_then(|target| target_doc.and_then(|doc| doc.target_page(target)))
                    .unwrap_or(opening.target_page);
            }
        }
    }

    fn mark_opening_ready(&mut self, doc: DocId) -> Vec<u64> {
        let mut resumes = Vec::new();
        for (ticket, opening) in &mut self.opening {
            if opening.doc == Some(doc) {
                opening.ready = true;
                resumes.push(*ticket);
            }
        }
        resumes
    }

    pub(crate) fn resume_retypeset(&mut self, doc: DocId) {
        let ready: Vec<u64> = self
            .typesetting
            .iter()
            .filter_map(|(slot, t)| (t.preview_doc == Some(doc) && !t.continuing).then_some(*slot))
            .collect();
        for slot in ready {
            let Some(entry) = self.docs.get(&doc) else {
                continue;
            };
            let Some(state) = self.typesetting.get_mut(&slot) else {
                continue;
            };
            state.continuing = true;
            let task = RetypesetTask {
                ticket: state.ticket,
                id: doc,
                doc: Arc::clone(&entry.doc),
                typo: Arc::clone(&state.typo),
                inserts: state.inserts.clone(),
                target: None,
                prime: None,
                cancel: Arc::clone(&state.cancel),
            };
            self.pool.continue_retypeset(task);
        }
    }

    pub(crate) fn resume_opening(&mut self, ticket: u64) {
        let active = self
            .opening
            .get(&ticket)
            .and_then(|o| o.doc)
            .is_some_and(|id| self.has_active_retypeset(id));
        if active {
            return;
        }
        enum Next {
            Open(Box<OpenBook>),
            Reset(Box<Document>, Arc<Typography>),
        }
        let next = {
            let Some(opening) = self.opening.get_mut(&ticket) else {
                return;
            };
            if (opening.doc.is_some() && !opening.ready) || opening.continuing {
                return;
            }
            let work = if let Some(doc) = opening.completed.take() {
                Next::Reset(Box::new(doc), Arc::clone(&opening.typo))
            } else if let Some(mut state) = opening.resume.take() {
                state.set_typography(opening.typo.as_ref());
                Next::Open(state)
            } else {
                return;
            };
            opening.pause.store(false, Ordering::Relaxed);
            opening.continuing = true;
            let flags = (Arc::clone(&opening.pause), Arc::clone(&opening.closed));
            let delay = std::mem::take(&mut opening.delay);
            (work, flags, delay)
        };
        match next.0 {
            Next::Open(state) => self.pool.continue_open(ticket, state, next.1, next.2),
            Next::Reset(doc, typo) => self.pool.complete_reset(ticket, *doc, typo, next.1, next.2),
        }
    }
}

fn push_primed(doc: DocId, primed: Primed, out: &mut Vec<Event>) {
    for (page, scale, result) in primed.rasters {
        out.push(Event::Rendered {
            key: PageKey {
                doc,
                page,
                scale,
                part: Part::Whole,
            },
            result,
        });
    }
    for (page, result) in primed.texts {
        out.push(Event::Text {
            doc,
            page,
            result: result.map(Arc::new),
        });
    }
}

#[cfg(test)]
#[path = "retype_tests.rs"]
mod tests;
