//! Workers fed by demand restated each frame; stale work is dropped.

mod types;
mod worker;

use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mareader_epub::{OpenBook, OpenTarget};
use mareader_reflow::Typography;

use crate::document::TextFile;
use crate::key::Want;
use crate::{Document, Source};

use types::{BgJob, Job, PendingBg};
pub(crate) use types::{Output, Primed, RetypesetTask};

/// Finished results wait here; past it, workers pause until a poll.
const DONE_ITEMS: usize = 32;
const DONE_BYTES: usize = 32 << 20;
/// Settle delay before background full-book layout starts after a preview.
pub(crate) const RETYPESET_SETTLE: Duration = Duration::from_millis(220);

pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    bg_jobs: VecDeque<PendingBg>,
    bg_cancel: Option<Arc<AtomicBool>>,
    running: HashSet<Want>,
    wanted: HashSet<Want>,
    done: VecDeque<Output>,
    done_bytes: usize,
    quit: bool,
}

impl State {
    fn full(&self) -> bool {
        self.done.len() + self.running.len() >= DONE_ITEMS || self.done_bytes >= DONE_BYTES
    }

    /// The next job a worker may take: opens always, the rest if room.
    fn next(&mut self) -> Option<Job> {
        let urgent = self.queue.front().is_some_and(Job::urgent);
        if urgent || !self.full() {
            self.queue.pop_front()
        } else {
            None
        }
    }
}

struct Shared {
    state: Mutex<State>,
    work: Condvar,
    bg_work: Condvar,
    wake: Wake,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
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
                .name("mareader-book-bg".to_owned())
                .spawn(move || worker::work_bg(&shared))
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

    pub(crate) fn continue_open(
        &self,
        ticket: u64,
        state: Box<OpenBook>,
        flags: (Arc<AtomicBool>, Arc<AtomicBool>),
        delay: Duration,
    ) {
        let (pause, closed) = flags;
        self.schedule_bg(
            BgJob::Open {
                ticket,
                state,
                pause,
                closed,
            },
            delay,
        );
    }

    pub(crate) fn complete_reset(
        &self,
        ticket: u64,
        doc: Document,
        typo: Arc<Typography>,
        flags: (Arc<AtomicBool>, Arc<AtomicBool>),
        delay: Duration,
    ) {
        let (pause, closed) = flags;
        self.schedule_bg(
            BgJob::CompleteReset {
                ticket,
                doc,
                typo,
                pause,
                closed,
            },
            delay,
        );
    }

    pub(crate) fn pause_open(&self) {
        self.shared.bg_work.notify_one();
    }

    pub(crate) fn cancel_open(&self, ticket: u64) {
        let mut s = self.shared.lock();
        s.queue
            .retain(|job| !matches!(job, Job::Background(bg) if bg.is_open_ticket(ticket)));
        s.bg_jobs.retain(|p| !p.job.is_open_ticket(ticket));
        drop(s);
        self.shared.bg_work.notify_one();
    }

    pub(crate) fn cancel_retypeset(&self, ticket: u64) {
        let mut s = self.shared.lock();
        s.queue.retain(|job| match job {
            Job::Retypeset(t) => t.ticket != ticket,
            Job::Background(bg) => !bg.is_retypeset_ticket(ticket),
            _ => true,
        });
        s.bg_jobs.retain(|p| !p.job.is_retypeset_ticket(ticket));
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
        self.schedule_bg(BgJob::Retypeset(task), RETYPESET_SETTLE);
    }

    fn schedule_bg(&self, job: BgJob, delay: Duration) {
        if self.bg_worker.is_some() {
            let ready_at = Instant::now() + delay;
            self.shared
                .lock()
                .bg_jobs
                .push_back(PendingBg { ready_at, job });
            self.shared.bg_work.notify_one();
            return;
        }
        self.shared.lock().queue.push_back(Job::Background(job));
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
            } else if matches!(&job, Job::Background(_)) {
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
        s.queue.len() + s.running.len() + s.bg_jobs.len() + usize::from(s.bg_cancel.is_some())
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
