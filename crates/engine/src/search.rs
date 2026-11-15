//! Document search: each index builds on the first query, off the frame.

use std::sync::Arc;

use mareader_search::{Glyph, Hit, MAX_HITS, Needle, Page, hits};
use mareader_session::{DocId, Document, TextLayer};

use crate::session::Waker;

/// Documents whose indexes are kept; the least recently searched goes.
const KEEP: usize = 3;

/// What a document's search has found for the query it last asked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    /// The query these hits answer.
    pub query: String,
    /// In page order; shared with the pages that paint them.
    pub hits: Arc<Vec<Hit>>,
    /// More hits than `MAX_HITS` turned up; the rest aren't kept.
    pub more: bool,
    pub indexed: usize,
    pub pages: usize,
}

impl Found {
    /// Every page is in: what isn't found isn't there.
    pub fn done(&self) -> bool {
        self.indexed >= self.pages
    }
}

/// A page's text layer, made searchable.
pub fn page_of(layer: &TextLayer) -> Page {
    Page::new(layer.text.chars().iter().map(|c| Glyph {
        ch: c.ch,
        line: c.line,
        space_before: c.space_before,
    }))
}

/// The hits a pane shows, and which is current.
#[derive(Clone, Debug, PartialEq)]
pub struct Lit {
    pub hits: Arc<Vec<Hit>>,
    pub active: Option<usize>,
}

/// One document's search, keyed by the very document it reads.
struct Job {
    doc: DocId,
    /// The `Document` it was built from: a re-typeset book is another.
    key: usize,
    asked: String,
    asks: u64,
    found: Found,
    work: Work,
}

/// Every open document's search; the last few searched are kept.
#[derive(Default)]
pub struct Search {
    jobs: Vec<Job>,
    inline: bool,
}

impl Search {
    /// Builds and answers on this thread, as tests need.
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    /// `document`'s hits for `query`, as far as it has read.
    pub fn found(&self, doc: DocId, document: &Arc<Document>) -> Option<&Found> {
        let key = Arc::as_ptr(document).addr();
        let job = self.jobs.iter().find(|j| j.doc == doc && j.key == key)?;
        Some(&job.found)
    }

    /// Asks `document` for `query`; the index starts on the first ask.
    pub fn ask(&mut self, wake: Waker, doc: DocId, document: &Arc<Document>, query: &str) {
        let key = Arc::as_ptr(document).addr();
        let at = self.jobs.iter().position(|j| j.doc == doc && j.key == key);
        let mut job = match at {
            Some(i) => self.jobs.remove(i),
            None => Job {
                doc,
                key,
                asked: String::new(),
                asks: 0,
                found: Found {
                    pages: document.page_count(),
                    ..Found::default()
                },
                work: Work::start(wake, document, self.inline),
            },
        };
        if job.asked != query || job.asks == 0 {
            job.asked = query.to_owned();
            job.asks += 1;
            job.work.ask(job.asks, query, &mut job.found);
        }
        self.jobs.push(job);
        if self.jobs.len() > KEEP {
            self.jobs.remove(0);
        }
    }

    /// Drops searches of documents closed or set again, and their threads.
    pub fn forget_closed<'a>(&mut self, open: impl Fn(DocId) -> Option<&'a Arc<Document>>) {
        let live = |j: &Job| open(j.doc).is_some_and(|d| Arc::as_ptr(d).addr() == j.key);
        self.jobs.retain(live);
    }

    /// Searches kept, each holding its document's text; for meters.
    pub fn len(&self) -> usize {
        self.jobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// Whether any search index is still catching up with its pages.
    pub fn busy(&self) -> bool {
        self.jobs.iter().any(|j| !j.found.done())
    }

    /// Takes in what the indexes found; true if anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        for job in &mut self.jobs {
            changed |= job.work.poll(job.asks, &mut job.found);
        }
        changed
    }
}

/// What the search thread is told: the latest query, numbered.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct Ask(u64, String);

/// What it tells back.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
enum Told {
    /// The query's hits over the pages read so far, replacing any.
    Answer(u64, String, Vec<Hit>, bool),
    /// Hits on pages read since, for the latest query.
    More(u64, Vec<Hit>, bool),
    Indexed(usize),
}

/// The pages read so far and the query they answer.
struct Index {
    document: Arc<Document>,
    pages: Vec<Page>,
    needle: Option<Needle>,
    kept: usize,
}

impl Index {
    fn new(document: Arc<Document>) -> Self {
        Self {
            pages: Vec::with_capacity(document.page_count()),
            document,
            needle: None,
            kept: 0,
        }
    }

    fn done(&self) -> bool {
        self.pages.len() >= self.document.page_count()
    }

    /// A new query over every page read; true if hits were cut.
    fn answer(&mut self, query: &str) -> (Vec<Hit>, bool) {
        self.needle = Needle::new(query);
        self.kept = 0;
        let mut out = Vec::new();
        let mut more = false;
        for n in 0..self.pages.len() {
            let (found, cut) = self.hits_on(n);
            out.extend(found);
            more |= cut;
        }
        (out, more)
    }

    /// Reads the next page; its hits for the query asked.
    fn read_next(&mut self) -> (Vec<Hit>, bool) {
        let n = self.pages.len();
        // A page that can't be read has nothing to find.
        let page = self.document.text_layer(n).map(|l| page_of(&l));
        self.pages.push(page.unwrap_or_default());
        self.hits_on(n)
    }

    fn hits_on(&mut self, n: usize) -> (Vec<Hit>, bool) {
        let (Some(needle), Some(page)) = (&self.needle, self.pages.get(n)) else {
            return (Vec::new(), false);
        };
        let room = MAX_HITS.saturating_sub(self.kept);
        let mut found = hits(n, page, needle, room + 1);
        let cut = found.len() > room;
        found.truncate(room);
        self.kept += found.len();
        (found, cut)
    }
}

/// The index, on its own thread or here.
enum Work {
    Here(Box<Index>),
    #[cfg(not(target_arch = "wasm32"))]
    Thread {
        ask: std::sync::mpsc::Sender<Ask>,
        told: std::sync::mpsc::Receiver<Told>,
    },
}

impl Work {
    fn start(wake: Waker, document: &Arc<Document>, inline: bool) -> Self {
        let index = Index::new(Arc::clone(document));
        #[cfg(not(target_arch = "wasm32"))]
        if !inline {
            let (ask, asks) = std::sync::mpsc::channel();
            let (tell, told) = std::sync::mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("search".into())
                .spawn(move || work(index, &asks, &tell, &wake));
            if spawned.is_ok() {
                return Self::Thread { ask, told };
            }
            return Self::Here(Box::new(Index::new(Arc::clone(document))));
        }
        let _ = (wake, inline);
        Self::Here(Box::new(index))
    }

    fn ask(&mut self, asks: u64, query: &str, found: &mut Found) {
        match self {
            // Here every page is read at once: tests and the browser.
            Self::Here(index) => {
                while !index.done() {
                    index.read_next();
                }
                let (hits, more) = index.answer(query);
                *found = Found {
                    query: query.to_owned(),
                    hits: Arc::new(hits),
                    more,
                    indexed: index.pages.len(),
                    pages: found.pages,
                };
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Thread { ask, .. } => {
                let _ = ask.send(Ask(asks, query.to_owned()));
            }
        }
        let _ = asks;
    }

    fn poll(&mut self, asks: u64, found: &mut Found) -> bool {
        let _ = (asks, &found);
        #[cfg(not(target_arch = "wasm32"))]
        if let Self::Thread { told, .. } = self {
            let mut changed = false;
            while let Ok(t) = told.try_recv() {
                changed = true;
                match t {
                    Told::Answer(n, query, hits, more) if n == asks => {
                        found.query = query;
                        found.hits = Arc::new(hits);
                        found.more = more;
                    }
                    Told::More(n, hits, more) if n == asks => {
                        Arc::make_mut(&mut found.hits).extend(hits);
                        found.more |= more;
                    }
                    Told::Indexed(pages) => found.indexed = pages,
                    Told::Answer(..) | Told::More(..) => {}
                }
            }
            return changed;
        }
        false
    }
}

/// Pages told about at once while reading, so the frame isn't woken for each.
#[cfg(not(target_arch = "wasm32"))]
const TELL_EVERY: usize = 16;

/// The search thread: reads pages until done, answering the latest query.
#[cfg(not(target_arch = "wasm32"))]
fn work(
    mut index: Index,
    asks: &std::sync::mpsc::Receiver<Ask>,
    tell: &std::sync::mpsc::Sender<Told>,
    wake: &Waker,
) {
    use std::sync::mpsc::TryRecvError;
    let mut asked = 0;
    loop {
        // Waits only once every page is read.
        let next = if index.done() {
            asks.recv().ok()
        } else {
            match asks.try_recv() {
                Ok(a) => Some(a),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => return,
            }
        };
        if index.done() && next.is_none() {
            return;
        }
        if let Some(mut latest) = next {
            // Only the newest of several queued queries is answered.
            while let Ok(newer) = asks.try_recv() {
                latest = newer;
            }
            let Ask(n, query) = latest;
            asked = n;
            let (hits, more) = index.answer(&query);
            let _ = tell.send(Told::Answer(n, query, hits, more));
            wake();
            continue;
        }
        let (hits, more) = index.read_next();
        let read = index.pages.len();
        let found = !hits.is_empty() || more;
        if found {
            let _ = tell.send(Told::More(asked, hits, more));
        }
        if found || read.is_multiple_of(TELL_EVERY) || index.done() {
            if tell.send(Told::Indexed(read)).is_err() {
                return;
            }
            wake();
        }
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
