//! Workers fed by demand restated each frame; stale work is dropped.

mod worker;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mareader_epub::{Inserts, OpenBook, OpenTarget};
use mareader_pdf::{Paper, Raster, RenderError, TextLayer};
use mareader_reflow::Typography;

use crate::document::TextFile;
use crate::key::{DocId, PageKey, Want};
use crate::{Document, OpenError, Source};

/// Finished results wait here; past it, workers pause until a poll.
const DONE_ITEMS: usize = 32;
const DONE_BYTES: usize = 32 << 20;
/// Settle delay before background full-book layout starts after a preview.
const RETYPESET_SETTLE: Duration = Duration::from_millis(220);

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

pub(crate) enum Job {
    Open {
        ticket: u64,
        source: Source,
        typo: Arc<Typography>,
        /// Set when the file is text to convert first.
        text: Option<TextFile>,
        target: Option<OpenTarget>,
    },
    ContinueOpen {
        ticket: u64,
        state: Box<OpenBook>,
    },
    /// `doc`, open as `id`, set again in `typo`, maybe with `inserts`.
    Retypeset(RetypesetTask),
    /// Finishes full-book layout after a local retypeset preview rendered.
    ContinueRetypeset(RetypesetTask),
    Work {
        want: Want,
        doc: Arc<Document>,
    },
}

pub(crate) struct RetypesetTask {
    pub ticket: u64,
    pub id: DocId,
    pub doc: Arc<Document>,
    pub typo: Arc<Typography>,
    pub inserts: Option<Arc<Inserts>>,
    pub target: Option<OpenTarget>,
    pub cancel: Arc<AtomicBool>,
}

impl Job {
    /// Opens and re-sets jump the queue and its backlog limit.
    fn urgent(&self) -> bool {
        matches!(self, Self::Open { .. } | Self::Retypeset { .. })
    }
}

pub(crate) enum Output {
    Opened {
        ticket: u64,
        result: Result<Document, OpenError>,
    },
    OpenPreview {
        ticket: u64,
        doc: Document,
        state: Box<OpenBook>,
    },
    OpenProgress {
        ticket: u64,
        state: Box<OpenBook>,
    },
    OpenComplete {
        ticket: u64,
        doc: Document,
    },
    OpenFailed {
        ticket: u64,
        error: OpenError,
    },
    /// Instant local preview around the reading focus; full layout follows.
    RetypesetPreview {
        ticket: u64,
        id: DocId,
        doc: Document,
    },
    /// None when the book wouldn't set; the old one stays.
    Retypeset {
        ticket: u64,
        id: DocId,
        result: Option<Document>,
    },
    Rendered {
        key: PageKey,
        result: Result<Raster, RenderError>,
    },
    Read {
        doc: DocId,
        page: usize,
        result: Result<TextLayer, RenderError>,
    },
    Paper {
        doc: DocId,
        page: usize,
        result: Result<Paper, RenderError>,
    },
}

impl Output {
    pub(crate) fn bytes(&self) -> usize {
        match self {
            Self::Rendered { result: Ok(r), .. } => r.bytes(),
            Self::Read { result: Ok(t), .. } => t.bytes(),
            _ => 0,
        }
    }

    /// The demand this answers; opens answer none.
    pub(super) fn want(&self) -> Option<Want> {
        match self {
            Self::Rendered { key, .. } => Some(Want::Raster(*key)),
            Self::Read { doc, page, .. } => Some(Want::Text {
                doc: *doc,
                page: *page,
            }),
            Self::Paper { doc, page, .. } => Some(Want::Paper {
                doc: *doc,
                page: *page,
            }),
            Self::Opened { .. }
            | Self::OpenPreview { .. }
            | Self::OpenProgress { .. }
            | Self::OpenComplete { .. }
            | Self::OpenFailed { .. }
            | Self::RetypesetPreview { .. }
            | Self::Retypeset { .. } => None,
        }
    }
}

pub(super) struct PendingBg {
    pub ready_at: Instant,
    pub task: RetypesetTask,
}

#[derive(Default)]
pub(super) struct State {
    pub(super) queue: VecDeque<Job>,
    pub(super) bg_retypeset: VecDeque<PendingBg>,
    pub(super) bg_cancel: Option<Arc<AtomicBool>>,
    pub(super) running: HashSet<Want>,
    pub(super) wanted: HashSet<Want>,
    pub(super) done: VecDeque<Output>,
    pub(super) done_bytes: usize,
    pub(super) quit: bool,
}

impl State {
    fn full(&self) -> bool {
        self.done.len() + self.running.len() >= DONE_ITEMS || self.done_bytes >= DONE_BYTES
    }

    /// The next job a worker may take: opens always, the rest if room.
    pub(super) fn next(&mut self) -> Option<Job> {
        let urgent = self.queue.front().is_some_and(Job::urgent);
        if urgent || !self.full() {
            self.queue.pop_front()
        } else {
            None
        }
    }
}

pub(super) struct Shared {
    state: Mutex<State>,
    pub(super) work: Condvar,
    pub(super) bg_work: Condvar,
    pub(super) wake: Wake,
}

impl Shared {
    pub(super) fn lock(&self) -> MutexGuard<'_, State> {
        // Documents catch their own panics, so poison is only noise.
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Memory tracks the viewport: unwanted jobs and results are dropped.
pub(crate) struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    bg_worker: Option<JoinHandle<()>>,
}

impl Pool {
    /// Zero workers runs jobs inline from `poll`, for threadless targets.
    pub(crate) fn new(workers: usize, wake: Wake) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::default(),
            work: Condvar::new(),
            bg_work: Condvar::new(),
            wake,
        });
        let threads = (0..workers)
            .filter_map(|i| {
                let shared = Arc::clone(&shared);
                std::thread::Builder::new()
                    .name(format!("mareader-render-{i}"))
                    .spawn(move || worker::work(&shared))
                    .ok()
            })
            .collect();
        let bg_worker = if workers > 0 {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("mareader-retype-bg".to_owned())
                .spawn(move || worker::work_retype_bg(&shared))
                .ok()
        } else {
            None
        };
        Self {
            shared,
            workers: threads,
            bg_worker,
        }
    }

    pub(crate) fn open(
        &self,
        ticket: u64,
        source: Source,
        typo: Arc<Typography>,
        text: Option<TextFile>,
        target: Option<OpenTarget>,
    ) {
        self.urgent(Job::Open {
            ticket,
            source,
            typo,
            text,
            target,
        });
    }

    pub(crate) fn continue_open(&self, ticket: u64, state: Box<OpenBook>) {
        self.shared
            .lock()
            .queue
            .push_back(Job::ContinueOpen { ticket, state });
        if self.workers.is_empty() {
            (self.shared.wake)();
        } else {
            self.shared.work.notify_one();
        }
    }

    pub(crate) fn cancel_open(&self, ticket: u64) {
        self.shared.lock().queue.retain(
            |job| !matches!(job, Job::ContinueOpen { ticket: queued, .. } if *queued == ticket),
        );
    }

    pub(crate) fn cancel_retypeset(&self, ticket: u64) {
        let mut s = self.shared.lock();
        s.queue.retain(|job| {
            !matches!(
                job,
                Job::Retypeset(task) | Job::ContinueRetypeset(task) if task.ticket == ticket
            )
        });
        s.bg_retypeset
            .retain(|pending| pending.task.ticket != ticket);
        drop(s);
        self.shared.bg_work.notify_one();
    }

    /// Replaces the `stale` re-set if still queued.
    pub(crate) fn retypeset(&self, task: RetypesetTask, stale: Option<u64>) {
        if let Some(ticket) = stale {
            self.cancel_retypeset(ticket);
        }
        self.urgent(Job::Retypeset(task));
    }

    pub(crate) fn continue_retypeset(&self, task: RetypesetTask) {
        if self.bg_worker.is_some() {
            let ready_at = Instant::now() + RETYPESET_SETTLE;
            self.shared
                .lock()
                .bg_retypeset
                .push_back(PendingBg { ready_at, task });
            self.shared.bg_work.notify_one();
            return;
        }
        self.shared
            .lock()
            .queue
            .push_back(Job::ContinueRetypeset(task));
        if self.workers.is_empty() {
            (self.shared.wake)();
        } else {
            self.shared.work.notify_one();
        }
    }

    /// Opening and re-setting outrank drawing: the reader is waiting.
    fn urgent(&self, job: Job) {
        self.shared.lock().queue.push_front(job);
        if self.workers.is_empty() {
            (self.shared.wake)();
        } else {
            self.shared.work.notify_one();
        }
    }

    /// Replaces the demand; `jobs` come most urgent first.
    pub(crate) fn want(&self, jobs: Vec<(Want, Arc<Document>)>) {
        let mut s = self.shared.lock();
        let s = &mut *s;
        s.wanted = jobs.iter().map(|(k, _)| *k).collect();
        let mut priority = VecDeque::new();
        let mut background = VecDeque::new();
        for job in s.queue.drain(..) {
            if job.urgent() {
                priority.push_back(job);
            } else if matches!(
                &job,
                Job::ContinueOpen { .. } | Job::ContinueRetypeset { .. }
            ) {
                background.push_back(job);
            }
        }
        s.queue = priority;
        s.done
            .retain(|o| o.want().is_none_or(|w| s.wanted.contains(&w)));
        s.done_bytes = s.done.iter().map(Output::bytes).sum();
        let ready: HashSet<Want> = s.done.iter().filter_map(Output::want).collect();
        let fresh = jobs
            .into_iter()
            .filter(|(k, _)| !s.running.contains(k) && !ready.contains(k));
        s.queue
            .extend(fresh.map(|(want, doc)| Job::Work { want, doc }));
        s.queue.extend(background);
        let more = !s.queue.is_empty();
        if more && self.workers.is_empty() {
            // Inline work runs from the next poll: ask for a frame.
            (self.shared.wake)();
        } else if more {
            self.shared.work.notify_all();
        }
    }

    /// Takes finished work: at least one item, then up to the budget.
    pub(crate) fn poll(&self, max_items: usize, max_bytes: usize) -> Vec<Output> {
        if self.workers.is_empty() {
            self.run_inline();
        }
        let mut s = self.shared.lock();
        let mut out = Vec::new();
        let mut bytes = 0;
        while let Some(next) = s.done.front() {
            let size = next.bytes();
            let full = out.len() >= max_items || bytes + size > max_bytes;
            if !out.is_empty() && full {
                break;
            }
            bytes += size;
            s.done_bytes -= size;
            out.extend(s.done.pop_front());
        }
        if !s.done.is_empty() {
            (self.shared.wake)();
        }
        drop(s);
        // Room again: workers paused on a full queue may continue.
        self.shared.work.notify_all();
        out
    }

    /// Without threads: one job per poll keeps frames short.
    fn run_inline(&self) {
        let job = self.shared.lock().next();
        if let Some(job) = job {
            worker::finish(&self.shared, job);
        }
        if !self.shared.lock().queue.is_empty() {
            (self.shared.wake)();
        }
    }

    /// Jobs not yet finished, queued or running, for tests and meters.
    pub(crate) fn pending(&self) -> usize {
        let s = self.shared.lock();
        s.queue.len() + s.running.len() + s.bg_retypeset.len() + usize::from(s.bg_cancel.is_some())
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        {
            let mut s = self.shared.lock();
            s.quit = true;
            if let Some(cancel) = s.bg_cancel.as_ref() {
                cancel.store(true, Ordering::Relaxed);
            }
        }
        self.shared.work.notify_all();
        self.shared.bg_work.notify_all();
        for w in self.workers.drain(..) {
            let _ = w.join();
        }
        if let Some(bg) = self.bg_worker.take() {
            let _ = bg.join();
        }
    }
}
