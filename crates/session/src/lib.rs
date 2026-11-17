//! Open documents shared by every view, and their worker pool (L3).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod cover;
mod document;
mod key;
mod pool;
mod retype;
mod selection;
mod typeset;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mareader_epub::OpenBook;
pub use mareader_epub::{OpenTarget, SpinePlace};

pub use cover::{cover, describe, pictures};
pub use document::Document;
pub use key::{DocId, PageKey, Part, Scale, Want};
pub use mareader_epub::{Insert, Inserts};
pub use mareader_pdf::{PageSize, Paper, Passage, Raster, RenderError, TextLayer, Tiling};
pub use mareader_reflow::{
    Anchor, Band, FACES, Face, InteractiveItem, ItemPixmap, Kind, Transform2d, Typography,
    has_family,
};
pub use selection::{Caret, Selection};
pub use typeset::{bundled, typography};

use document::TextFile;
use pool::{Output, Pool};
use retype::Typesetting;

/// Where a document's bytes come from; workers read paths.
#[derive(Clone)]
pub enum Source {
    Bytes(Arc<Vec<u8>>),
    Path(PathBuf),
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bytes(b) => write!(f, "Bytes({} B)", b.len()),
            Self::Path(p) => write!(f, "Path({})", p.display()),
        }
    }
}

impl PartialEq for Source {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bytes(a), Self::Bytes(b)) => a == b,
            (Self::Path(a), Self::Path(b)) => a == b,
            _ => false,
        }
    }
}

impl Source {
    fn read(self) -> Result<Arc<Vec<u8>>, OpenError> {
        match self {
            Self::Bytes(b) => Ok(b),
            Self::Path(p) => std::fs::read(p)
                .map(Arc::new)
                .map_err(|e| OpenError::Unreadable(e.kind().to_string())),
        }
    }

    /// The path's text, for naming the document.
    fn file_name(&self) -> Option<String> {
        match self {
            Self::Bytes(_) => None,
            Self::Path(p) => Some(p.to_string_lossy().into_owned()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("the file couldn't be read ({0})")]
    Unreadable(String),
    #[error(transparent)]
    Pdf(#[from] mareader_pdf::OpenError),
    #[error(transparent)]
    Epub(#[from] mareader_epub::OpenError),
}

impl OpenError {
    /// What the reader can do about it, as a sentence.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Self::Unreadable(_) => Some("Check that it still exists and can be read."),
            Self::Pdf(e) => e.hint(),
            Self::Epub(_) => None,
        }
    }
}

/// Names one `open` call, to match its `Event::Opened`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ticket(u64);

#[derive(Debug)]
pub enum Event {
    Opened {
        ticket: Ticket,
        result: Result<DocId, OpenError>,
    },
    Preview {
        ticket: Ticket,
        doc: DocId,
    },
    Completed {
        ticket: Ticket,
        old: DocId,
        new: DocId,
    },
    Incomplete {
        ticket: Ticket,
        doc: DocId,
        error: OpenError,
    },
    Rendered {
        key: PageKey,
        result: Result<Raster, RenderError>,
    },
    /// A page's text and links, shared: views and copies read it.
    Text {
        doc: DocId,
        page: usize,
        result: Result<Arc<TextLayer>, RenderError>,
    },
    /// A page's paper colour, read from a small drawing of it.
    Paper {
        doc: DocId,
        page: usize,
        result: Result<Paper, RenderError>,
    },
    /// `old` set again in new type as `new`; `old` stays open.
    Retypeset {
        old: DocId,
        new: DocId,
        previous: Arc<Document>,
    },
}

/// One open document and what to call it.
#[derive(Debug, Clone)]
pub struct Entry {
    pub doc: Arc<Document>,
    pub name: String,
    /// The file it came from; none for bytes, such as a browser's drop.
    pub path: Option<PathBuf>,
    /// What a book is set in; documents with pages ignore it.
    pub typo: Arc<Typography>,
}

/// How much finished work one `poll` hands over.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub items: usize,
    pub bytes: usize,
}

/// Visible-page rendering hint to prime a retypeset before publishing it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prime {
    pub scale: Scale,
    pub pages: usize,
    pub spread: bool,
}

pub struct Session {
    pool: Pool,
    docs: BTreeMap<DocId, Entry>,
    /// Each open in flight: its fallback name, file and type.
    opening: BTreeMap<u64, Opening>,
    /// The newest re-set asked in each slot; older ones are dropped.
    typesetting: BTreeMap<u64, Typesetting>,
    next: u64,
}

struct Opening {
    name: Option<String>,
    path: Option<PathBuf>,
    typo: Arc<Typography>,
    target: Option<OpenTarget>,
    target_page: usize,
    doc: Option<DocId>,
    resume: Option<Box<OpenBook>>,
    completed: Option<Document>,
    pause: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    delay: Duration,
    ready: bool,
    continuing: bool,
}

impl Opening {
    fn pending(
        name: Option<String>,
        path: Option<PathBuf>,
        typo: Arc<Typography>,
        target: Option<OpenTarget>,
    ) -> Self {
        let target_page = target.as_ref().map_or(0, |t| t.page);
        Self {
            name,
            path,
            typo,
            target,
            target_page,
            doc: None,
            resume: None,
            completed: None,
            pause: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
            delay: Duration::ZERO,
            ready: false,
            continuing: false,
        }
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session({} documents)", self.docs.len())
    }
}

impl Session {
    /// `wake` runs on a worker when results are ready: request a frame.
    pub fn new(workers: usize, wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            pool: Pool::new(workers, Arc::new(wake)),
            docs: BTreeMap::new(),
            opening: BTreeMap::new(),
            typesetting: BTreeMap::new(),
            next: 1,
        }
    }

    /// Workers for this machine: two at most, leaving the UI a core.
    pub fn default_workers() -> usize {
        std::thread::available_parallelism().map_or(1, |n| n.get().clamp(2, 3) - 1)
    }

    /// Starts opening; `file_name` names it if its title won't.
    ///
    /// A book is set in `typo`.
    pub fn open(
        &mut self,
        source: Source,
        file_name: Option<String>,
        typo: Arc<Typography>,
    ) -> Ticket {
        self.start_open(source, file_name, typo, None)
    }

    /// Opens progressively toward `target` when the source is an EPUB.
    pub fn open_at(
        &mut self,
        source: Source,
        file_name: Option<String>,
        typo: Arc<Typography>,
        target: OpenTarget,
    ) -> Ticket {
        self.start_open(source, file_name, typo, Some(target))
    }

    fn start_open(
        &mut self,
        source: Source,
        file_name: Option<String>,
        typo: Arc<Typography>,
        target: Option<OpenTarget>,
    ) -> Ticket {
        let ticket = self.bump();
        let path = match &source {
            Source::Path(p) => Some(p.clone()),
            Source::Bytes(_) => None,
        };
        let name = file_name.or_else(|| source.file_name());
        let text = name.as_deref().and_then(|n| {
            Some(TextFile {
                format: mareader_epub::Format::of_name(n)?,
                name: n.to_owned(),
                folder: path
                    .clone()
                    .or_else(|| Some(PathBuf::from(n)).filter(|p| p.is_absolute()))
                    .and_then(|p| p.parent().map(Path::to_path_buf)),
            })
        });
        self.opening.insert(
            ticket,
            Opening::pending(name, path, Arc::clone(&typo), target.clone()),
        );
        self.pool.open(ticket, source, typo, text, target);
        Ticket(ticket)
    }

    pub fn close(&mut self, id: DocId) {
        self.docs.remove(&id);
        self.close_typesetting(id);
        let tickets: Vec<_> = self
            .opening
            .iter()
            .filter_map(|(ticket, opening)| (opening.doc == Some(id)).then_some(*ticket))
            .collect();
        for ticket in tickets {
            if let Some(opening) = self.opening.remove(&ticket) {
                opening.closed.store(true, Ordering::Relaxed);
                opening.pause.store(true, Ordering::Relaxed);
            }
            self.pool.cancel_open(ticket);
        }
    }

    /// At rest: every open book keeps only what its pages in view need.
    pub fn rest(&self) {
        self.docs.values().for_each(|e| e.doc.rest());
    }

    pub fn get(&self, id: DocId) -> Option<&Entry> {
        self.docs.get(&id)
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Replaces the demand, most urgent first.
    ///
    /// Work for closed documents is ignored.
    pub fn want(&mut self, wants: impl IntoIterator<Item = impl Into<Want>>) {
        let jobs = wants
            .into_iter()
            .map(Into::into)
            .filter_map(|w: Want| self.docs.get(&w.doc()).map(|e| (w, Arc::clone(&e.doc))))
            .collect();
        self.pool.want(jobs);
    }

    /// Finished work, within `budget` (always at least one item).
    pub fn poll(&mut self, budget: Budget) -> Vec<Event> {
        let done = self.pool.poll(budget.items, budget.bytes);
        let mut events = Vec::with_capacity(done.len());
        for out in done {
            self.accept(out, &mut events);
        }
        events
    }

    /// Jobs queued or running.
    pub fn pending(&self) -> usize {
        self.pool.pending()
    }

    /// Whether a document is still opening.
    pub fn opening(&self) -> bool {
        !self.opening.is_empty()
    }

    /// Files a document that finished opening and hands back its id.
    fn admit(
        &mut self,
        doc: Document,
        name: String,
        path: Option<PathBuf>,
        typo: Arc<Typography>,
    ) -> DocId {
        let id = DocId(self.bump());
        self.docs.insert(
            id,
            Entry {
                doc: Arc::new(doc),
                name,
                path,
                typo,
            },
        );
        id
    }

    fn accept(&mut self, out: Output, events: &mut Vec<Event>) {
        match out {
            Output::Opened { ticket, result } => {
                let Some(Opening {
                    name, path, typo, ..
                }) = self.opening.remove(&ticket)
                else {
                    return;
                };
                let result = result.map(|doc| {
                    let name = mareader_pdf::display_name(doc.raw_title(), name.as_deref())
                        .unwrap_or_else(|| "Untitled".to_string());
                    self.admit(doc, name, path, typo)
                });
                events.push(Event::Opened {
                    ticket: Ticket(ticket),
                    result,
                });
            }
            Output::OpenPreview { ticket, doc, state } => {
                let Some(opening) = self.opening.get(&ticket) else {
                    return;
                };
                let name = mareader_pdf::display_name(doc.raw_title(), opening.name.as_deref())
                    .unwrap_or_else(|| "Untitled".to_string());
                let path = opening.path.clone();
                let typo = Arc::clone(&opening.typo);
                let target_page = opening
                    .target
                    .as_ref()
                    .and_then(|target| doc.target_page(target))
                    .unwrap_or(opening.target_page);
                let id = self.admit(doc, name, path, typo);
                let Some(opening) = self.opening.get_mut(&ticket) else {
                    return;
                };
                opening.doc = Some(id);
                opening.target_page = target_page;
                opening.resume = Some(state);
                opening.ready = false;
                opening.continuing = false;
                events.push(Event::Preview {
                    ticket: Ticket(ticket),
                    doc: id,
                });
            }
            Output::OpenProgress { ticket, mut state } => {
                let Some(opening) = self.opening.get_mut(&ticket) else {
                    return;
                };
                state.set_typography(opening.typo.as_ref());
                opening.resume = Some(state);
                opening.continuing = false;
                self.resume_opening(ticket);
            }
            Output::OpenComplete { ticket, doc } => {
                let matches = self
                    .opening
                    .get(&ticket)
                    .map(|o| doc.has_typography(o.typo.as_ref()));
                let Some(matches) = matches else {
                    return;
                };
                if !matches {
                    if let Some(opening) = self.opening.get_mut(&ticket) {
                        opening.completed = Some(doc);
                        opening.continuing = false;
                    }
                    self.resume_opening(ticket);
                    return;
                }
                let Some(opening) = self.opening.remove(&ticket) else {
                    return;
                };
                let name = mareader_pdf::display_name(doc.raw_title(), opening.name.as_deref())
                    .unwrap_or_else(|| "Untitled".to_string());
                let new = self.admit(doc, name, opening.path, opening.typo);
                match opening.doc {
                    Some(old) => {
                        self.close_typesetting(old);
                        events.push(Event::Completed {
                            ticket: Ticket(ticket),
                            old,
                            new,
                        });
                    }
                    None => events.push(Event::Opened {
                        ticket: Ticket(ticket),
                        result: Ok(new),
                    }),
                }
            }
            Output::OpenCompleteStale { ticket, doc } => {
                let Some(opening) = self.opening.get_mut(&ticket) else {
                    return;
                };
                opening.completed = Some(doc);
                opening.continuing = false;
                self.resume_opening(ticket);
            }
            Output::OpenFailed { ticket, error } => {
                let Some(opening) = self.opening.remove(&ticket) else {
                    return;
                };
                match opening.doc {
                    Some(doc) => events.push(Event::Incomplete {
                        ticket: Ticket(ticket),
                        doc,
                        error,
                    }),
                    None => events.push(Event::Opened {
                        ticket: Ticket(ticket),
                        result: Err(error),
                    }),
                }
            }
            Output::RetypesetPreview {
                ticket,
                id,
                doc,
                primed,
            } => self.accept_retypeset_preview(ticket, id, doc, primed, events),
            Output::Retypeset {
                ticket,
                id,
                result,
                primed,
            } => self.accept_retypeset(ticket, id, result, primed, events),
            Output::Rendered { key, result } => {
                if !self.docs.contains_key(&key.doc) {
                    return;
                }
                let ticket = self.opening.iter().find_map(|(ticket, opening)| {
                    (opening.doc == Some(key.doc) && opening.target_page == key.page)
                        .then_some(*ticket)
                });
                if let Some(ticket) = ticket {
                    if let Some(opening) = self.opening.get_mut(&ticket) {
                        opening.ready = true;
                    }
                    self.resume_opening(ticket);
                }
                self.resume_retypeset(key.doc);
                events.push(Event::Rendered { key, result });
            }
            Output::Read { doc, page, result } => {
                if self.docs.contains_key(&doc) {
                    events.push(Event::Text {
                        doc,
                        page,
                        result: result.map(Arc::new),
                    });
                }
            }
            Output::Paper { doc, page, result } => {
                if self.docs.contains_key(&doc) {
                    events.push(Event::Paper { doc, page, result });
                }
            }
        }
    }

    fn bump(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

#[cfg(test)]
mod tests;
