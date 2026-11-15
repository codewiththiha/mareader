//! Polling the document worker pool, file watch reloads, and open notices.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mareader_library::BookId;
use mareader_session::{
    Anchor, Budget, DocId, Document, Event as DocEvent, OpenError, OpenTarget, Source, Ticket,
};
use mareader_workspace::PaneId;

use super::state::{Sink, Slot, State};
use crate::reading::Place;
use crate::toast::Toast;

/// Uploads per frame, so a burst of pages never stalls a scroll.
pub const UPLOADS: Budget = Budget {
    items: 8,
    bytes: 16 << 20,
};

impl State {
    /// Polls finished worker events, applies transitions, and watches files.
    pub fn poll(
        &mut self,
        budget: Budget,
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) -> Vec<Toast> {
        let mut toasts = Vec::new();
        let events = match &mut self.session {
            Some(session) => session.poll(budget),
            None => return toasts,
        };
        let mut transitions = Vec::new();
        let mut reloaded = Vec::new();
        for event in events {
            self.handle_event(event, sink, &mut toasts, &mut transitions, &mut reloaded);
        }
        for (old, new, path, again) in reloaded {
            if let Some(new) = new {
                self.reloaded(old, new, said, sink);
            }
            if again {
                self.reload(&path);
            }
        }
        for (is_retype, old, new) in transitions {
            if is_retype {
                self.retyped(old, new, said, sink);
            } else {
                self.adopt_page_rasters(old, new, sink);
                self.reloaded(old, new, said, sink);
            }
        }
        self.follow_files();
        toasts
    }

    fn handle_event(
        &mut self,
        event: DocEvent,
        sink: &mut dyn Sink,
        toasts: &mut Vec<Toast>,
        transitions: &mut Vec<(bool, DocId, DocId)>,
        reloaded: &mut Vec<(DocId, Option<DocId>, PathBuf, bool)>,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        match event {
            DocEvent::Rendered { key, result } => sink.rendered(key, result),
            DocEvent::Text { doc, page, result } => self.texts.insert(doc, page, result),
            DocEvent::Paper { doc, page, result } => self.papers.insert(doc, page, result),
            DocEvent::Retypeset { old, new, .. } => transitions.push((true, old, new)),
            DocEvent::Preview { ticket, doc } => {
                if let Some((pane, _, target, book)) = waiting(&self.panes, ticket) {
                    let entry = session.get(doc);
                    let count = entry.map_or(0, |e| e.doc.page_count());
                    let page = landing(entry.map(|e| &*e.doc), &target);
                    let anchor = entry.and_then(|e| e.doc.target_anchor(&target));
                    let mut place = Place::new(doc);
                    place.book = book;
                    sink.opened(pane, &mut place, page, count, anchor);
                    self.panes.insert(pane, Slot::Open(place));
                } else {
                    session.close(doc);
                }
            }
            DocEvent::Completed { old, new, .. } => transitions.push((false, old, new)),
            DocEvent::Incomplete { doc, error, .. } => {
                let name = session
                    .get(doc)
                    .map_or_else(|| "document".to_owned(), |e| e.name.clone());
                toasts.push(open_incomplete(&name, &error));
            }
            DocEvent::Opened { ticket, result } if self.reloads.has(&ticket) => {
                if let Some((old, path, again)) = self.reloads.finish(&ticket) {
                    reloaded.push((old, result.ok(), path, again));
                }
            }
            DocEvent::Opened { ticket, result } => match (waiting(&self.panes, ticket), result) {
                (Some((pane, name, target, book)), Ok(doc)) => {
                    let entry = session.get(doc);
                    if entry.is_some_and(|e| e.doc.cut_short()) {
                        toasts.push(cut_short(&name));
                    }
                    let count = entry.map_or(0, |e| e.doc.page_count());
                    let page = landing(entry.map(|e| &*e.doc), &target);
                    let anchor = entry.and_then(|e| e.doc.target_anchor(&target));
                    let mut place = Place::new(doc);
                    place.book = book;
                    sink.opened(pane, &mut place, page, count, anchor);
                    self.panes.insert(pane, Slot::Open(place));
                }
                (Some((pane, name, _, _)), Err(e)) => {
                    self.panes.remove(&pane);
                    sink.closed(pane);
                    toasts.push(open_failed(&name, &e));
                }
                (None, Ok(doc)) => session.close(doc),
                (None, Err(_)) => {}
            },
        }
    }

    /// Hands the watch the files shown, and opens again those saved since.
    fn follow_files(&mut self) {
        let (Some(session), Some(watch)) = (&self.session, &mut self.watch) else {
            return;
        };
        let shown = || {
            self.panes.values().filter_map(|s| match s {
                Slot::Open(place) => Some(place.doc),
                Slot::Opening(_) => None,
            })
        };
        if !shown().eq(self.followed.iter().copied()) {
            self.followed = shown().collect();
            let files = self.followed.iter();
            watch.follow(
                files
                    .filter_map(|d| session.get(*d)?.path.clone())
                    .collect(),
            );
        }
        for path in watch.changed() {
            self.reload(&path);
        }
    }

    /// Opens `path`'s documents again; panes keep the old ones meanwhile.
    pub fn reload(&mut self, path: &Path) {
        let Some(session) = &mut self.session else {
            return;
        };
        let mut docs: Vec<DocId> = (self.panes.values())
            .filter_map(|s| match s {
                Slot::Open(place) => Some(place.doc),
                Slot::Opening(_) => None,
            })
            .filter(|d| {
                session
                    .get(*d)
                    .is_some_and(|e| e.path.as_deref() == Some(path))
            })
            .collect();
        docs.sort();
        docs.dedup();
        for doc in docs {
            if !self.reloads.should_start(doc) {
                continue;
            }
            let Some(entry) = session.get(doc) else {
                continue;
            };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
            let typo = Arc::clone(&entry.typo);
            let ticket = session.open(Source::Path(path.to_path_buf()), name, typo);
            self.reloads.started(ticket, doc, path.to_path_buf());
        }
    }

    fn reloaded(
        &mut self,
        old: DocId,
        new: DocId,
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) {
        let panes: Vec<PaneId> = (self.panes.iter())
            .filter(|(_, s)| matches!(s, Slot::Open(place) if place.doc == old))
            .map(|(pane, _)| *pane)
            .collect();
        let count = (self.session.as_ref())
            .and_then(|s| s.get(new))
            .filter(|e| !e.doc.is_reflowable())
            .map(|e| e.doc.page_count());
        for pane in panes {
            match (count, self.panes.get_mut(&pane)) {
                (Some(count), Some(Slot::Open(place))) => {
                    sink.reloaded_fixed(pane, place, new, count);
                }
                _ => self.retarget(pane, old, new, said, sink),
            }
        }
        self.release(new, sink);
        self.release(old, sink);
        self.retype(said, sink);
    }

    fn retyped(
        &mut self,
        old: DocId,
        new: DocId,
        said: Option<(PaneId, Anchor)>,
        sink: &mut dyn Sink,
    ) {
        let session = self.session.as_ref();
        let Some((typo, key)) = session
            .and_then(|s| s.get(new))
            .map(|e| (Arc::clone(&e.typo), e.doc.inserts_key()))
        else {
            return;
        };
        let waiting: Vec<PaneId> = (self.panes.iter())
            .filter(|(pane, s)| {
                let on_old = matches!(s, Slot::Open(place) if place.doc == old);
                let wants = self.inserts.get(*pane).map_or(0, |i| i.key) == key;
                on_old && wants && *self.want(**pane) == *typo
            })
            .map(|(pane, _)| *pane)
            .collect();
        for pane in waiting {
            self.retarget(pane, old, new, said, sink);
        }
        self.hand_over(old, new, sink);
        self.release(new, sink);
        self.release(old, sink);
    }
}

fn waiting(
    panes: &std::collections::BTreeMap<PaneId, Slot>,
    ticket: Ticket,
) -> Option<(PaneId, String, OpenTarget, Option<BookId>)> {
    panes.iter().find_map(|(id, slot)| match slot {
        Slot::Opening(o) if o.ticket == ticket => Some((
            *id,
            o.name.clone(),
            OpenTarget {
                page: o.page,
                place: o.place,
                spine: o.spine.clone(),
                pages: o.pages,
            },
            o.book,
        )),
        _ => None,
    })
}

pub(super) fn landing(doc: Option<&Document>, target: &OpenTarget) -> usize {
    let resolved = doc.and_then(|d| d.target_page(target));
    resolved.unwrap_or(target.page)
}

fn open_failed(name: &str, e: &OpenError) -> Toast {
    Toast::error(match e.hint() {
        Some(hint) => format!("Couldn't open {name}: {e}. {hint}"),
        None => format!("Couldn't open {name}: {e}."),
    })
}

fn open_incomplete(name: &str, e: &OpenError) -> Toast {
    Toast::warning(format!(
        "Couldn't finish loading {name}: {e}. Pages already opened remain available."
    ))
}

fn cut_short(name: &str) -> Toast {
    Toast::warning(format!(
        "{name} looks cut short; some pages may be missing."
    ))
}

#[cfg(test)]
mod open_notes {
    use super::*;
    use mareader_pdf::OpenError as Pdf;
    use mareader_session::OpenError;

    #[test]
    fn a_failure_says_why_and_what_to_try() {
        let locked = open_failed("deed.pdf", &OpenError::Pdf(Pdf::Locked));
        assert!(
            locked
                .message
                .starts_with("Couldn't open deed.pdf: the PDF is locked")
        );
        assert!(locked.message.ends_with("save an unlocked copy."));
        let none = open_failed("a.pdf", &OpenError::Pdf(Pdf::NoPages));
        assert_eq!(none.message, "Couldn't open a.pdf: the PDF has no pages.");
        assert_eq!(cut_short("a.pdf").tone, crate::toast::Tone::Warning);
    }
}
