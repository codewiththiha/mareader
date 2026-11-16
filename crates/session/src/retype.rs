//! Progressive, cancellable EPUB retypesetting orchestration on [`Session`].

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mareader_epub::{Book, Inserts, OpenTarget};
use mareader_reflow::Typography;

use crate::pool::RetypesetTask;
use crate::{DocId, Document, Entry, Event, Session};

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
        for opening in self.opening.values_mut() {
            if opening.doc == Some(id) {
                opening.typo = Arc::clone(&typo);
            }
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
            cancel,
        };
        self.pool.retypeset(task, stale);
    }

    /// Cancels any queued or running retypeset in `slot`.
    pub fn cancel_retypeset(&mut self, slot: u64) {
        if let Some(prev) = self.typesetting.remove(&slot) {
            prev.cancel.store(true, Ordering::Relaxed);
            self.pool.cancel_retypeset(prev.ticket);
        }
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
    ) -> Option<Event> {
        let slot = self
            .typesetting
            .iter()
            .find_map(|(slot, t)| (t.ticket == ticket).then_some(*slot))?;
        let Some(old) = self.docs.get(&id) else {
            self.typesetting.remove(&slot);
            return None;
        };
        let (name, path, previous) = (old.name.clone(), old.path.clone(), Arc::clone(&old.doc));
        let typo = self.typesetting.get(&slot).map(|t| Arc::clone(&t.typo))?;
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
        Some(Event::Retypeset {
            old: id,
            new,
            previous,
        })
    }

    pub(crate) fn accept_retypeset(
        &mut self,
        ticket: u64,
        id: DocId,
        result: Option<Document>,
    ) -> Option<Event> {
        let slot = self
            .typesetting
            .iter()
            .find_map(|(slot, t)| (t.ticket == ticket).then_some(*slot))?;
        let set = self.typesetting.remove(&slot)?;
        let mut doc = result?;
        for opening in self.opening.values() {
            if opening.doc == Some(id)
                && let Some(target) = opening.target.as_ref()
            {
                doc.set_preview_target(target);
            }
        }
        let old = self.docs.get(&id)?;
        let entry = Entry {
            doc: Arc::new(doc),
            name: old.name.clone(),
            path: old.path.clone(),
            typo: set.typo,
        };
        let previous = Arc::clone(&old.doc);
        let new = DocId(self.bump());
        self.docs.insert(new, entry);
        let target_doc = self.docs.get(&new).map(|entry| &*entry.doc);
        let mut resumes = Vec::new();
        for (ticket, opening) in &mut self.opening {
            if opening.doc == Some(id) {
                opening.doc = Some(new);
                opening.target_page = opening
                    .target
                    .as_ref()
                    .and_then(|target| target_doc.and_then(|doc| doc.target_page(target)))
                    .unwrap_or(opening.target_page);
                if opening.ready {
                    resumes.push(*ticket);
                }
            }
        }
        for ticket in resumes {
            self.resume_opening(ticket);
        }
        Some(Event::Retypeset {
            old: id,
            new,
            previous,
        })
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
                cancel: Arc::clone(&state.cancel),
            };
            self.pool.continue_retypeset(task);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, PageKey, Part, Scale, Source};

    const ALL: Budget = Budget {
        items: 64,
        bytes: usize::MAX,
    };

    fn key(doc: DocId, page: usize) -> PageKey {
        PageKey {
            doc,
            page,
            scale: Scale::covering(0.5),
            part: Part::Whole,
        }
    }

    fn drain_until(session: &mut Session, done: impl Fn(&Event) -> bool) -> Vec<Event> {
        let mut out = Vec::new();
        for _ in 0..200 {
            let batch = session.poll(ALL);
            let hit = batch.iter().any(&done);
            out.extend(batch);
            if hit {
                break;
            }
        }
        out
    }

    #[test]
    fn large_epub_retypesets_with_local_preview_then_completes() -> Result<(), &'static str> {
        let mut s = Session::new(0, || {});
        let bytes = Arc::new(mareader_epub::sample::epub(80, "Novel"));
        s.open(Source::Bytes(bytes), None, Arc::default());
        let events = drain_until(&mut s, |e| matches!(e, Event::Opened { .. }));
        let doc = events
            .iter()
            .find_map(|e| match e {
                Event::Opened { result: Ok(id), .. } => Some(*id),
                _ => None,
            })
            .ok_or("book did not open")?;
        let entry = s.get(doc).ok_or("missing open entry")?;
        let page = entry.doc.page_count() / 2;
        let anchor = entry.doc.anchor(page);
        let spine = anchor.and_then(|a| entry.doc.spine_place(a));
        let target = OpenTarget {
            page,
            pages: entry.doc.page_count(),
            place: anchor,
            spine,
        };
        let big = Arc::new(Typography {
            font_size: 24.0,
            ..Typography::default()
        });
        s.reset_at(doc, Arc::clone(&big), None, 7, Some(target));
        let events = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
        let preview = events
            .iter()
            .find_map(|e| match e {
                Event::Retypeset { old, new, .. } if *old == doc => Some(*new),
                _ => None,
            })
            .ok_or("preview was not produced")?;
        assert!(s.get(preview).is_some_and(|e| !e.doc.is_complete()));
        // Closing the superseded doc keeps full layout alive for the preview.
        s.close(doc);
        assert_eq!(s.pending(), 0, "full layout waits for preview render");
        let local = anchor
            .and_then(|a| s.get(preview).and_then(|e| e.doc.locate(a)))
            .unwrap_or(0);
        s.want([key(preview, local)]);
        let rendered = s.poll(ALL);
        assert!(rendered.iter().any(|e| matches!(e, Event::Rendered { .. })));
        let done = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
        let full = done.iter().find_map(|e| match e {
            Event::Retypeset { old, new, .. } if *old == preview => Some(*new),
            _ => None,
        });
        assert!(
            full.and_then(|id| s.get(id))
                .is_some_and(|e| e.doc.is_complete())
        );
        Ok(())
    }

    #[test]
    fn rapid_retypeset_cancels_superseded_preview_continuation() -> Result<(), &'static str> {
        let mut s = Session::new(0, || {});
        let bytes = Arc::new(mareader_epub::sample::epub(80, "Drag"));
        s.open(Source::Bytes(bytes), None, Arc::default());
        let events = drain_until(&mut s, |e| matches!(e, Event::Opened { .. }));
        let doc = events
            .iter()
            .find_map(|e| match e {
                Event::Opened { result: Ok(id), .. } => Some(*id),
                _ => None,
            })
            .ok_or("book did not open")?;
        let target = OpenTarget::default();
        let t1 = Arc::new(Typography {
            font_size: 20.0,
            ..Typography::default()
        });
        let t2 = Arc::new(Typography {
            font_size: 26.0,
            ..Typography::default()
        });
        s.reset_at(doc, t1, None, 1, Some(target.clone()));
        let first = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
        let p1 = first
            .iter()
            .find_map(|e| match e {
                Event::Retypeset { new, .. } => Some(*new),
                _ => None,
            })
            .ok_or("first preview missing")?;
        s.close(doc);
        s.want([key(p1, 0)]);
        let _ = s.poll(ALL);
        // Dragging again cancels p1's full layout and previews t2 immediately.
        s.reset_at(p1, Arc::clone(&t2), None, 1, Some(target));
        let second = drain_until(&mut s, |e| matches!(e, Event::Retypeset { .. }));
        let p2 = second.iter().find_map(|e| match e {
            Event::Retypeset { old, new, .. } if *old == p1 => Some(*new),
            _ => None,
        });
        assert!(p2.is_some_and(|id| {
            s.get(id)
                .is_some_and(|e| e.typo == t2 && !e.doc.is_complete())
        }));
        Ok(())
    }
}
