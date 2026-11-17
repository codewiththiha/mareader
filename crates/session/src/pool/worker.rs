//! Worker loop and job execution, reusing hayro caches across PDF jobs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use mareader_epub::{OpenBook, OpenStep, OpenTarget};
use mareader_pdf::RenderSession;
use mareader_reflow::Typography;

use super::Shared;
use super::types::{BgJob, Job, Output, Primed, RetypesetTask};
use crate::document::TextFile;
use crate::key::{Part, Want};
use crate::{Document, OpenError, Prime, Source};

/// Consecutive jobs served by one PDF render cache before rebuilding it.
const PDF_SESSION_CAP: usize = 64;
/// How long an idle worker keeps a PDF render cache warm between bursts.
const PDF_SESSION_IDLE: Duration = Duration::from_secs(2);

pub(super) fn work(shared: &Shared) {
    let mut pending = None;
    loop {
        let Some(job) = pending.take().or_else(|| take_job(shared)) else {
            return;
        };
        match job {
            Job::Work { want, doc } if doc.as_pdf().is_some() => {
                pending = work_pdf(shared, doc, want);
            }
            other => finish(shared, other),
        }
    }
}

/// Background thread for full-book layout and progressive EPUB completion.
pub(super) fn work_bg(shared: &Shared) {
    loop {
        let Some(job) = take_bg_job(shared) else {
            return;
        };
        if let Some(out) = run_bg_job(job) {
            commit(shared, out);
        }
        shared.lock().bg_cancel = None;
    }
}

fn take_bg_job(shared: &Shared) -> Option<BgJob> {
    let mut s = shared.lock();
    loop {
        if s.quit {
            return None;
        }
        s.bg_jobs.retain(|p| match &p.job {
            BgJob::Retypeset(t) => !t.cancel.load(Ordering::Relaxed),
            BgJob::Open { closed, .. } | BgJob::CompleteReset { closed, .. } => {
                !closed.load(Ordering::Relaxed)
            }
        });
        if let Some(idx) = s.bg_jobs.iter().position(|p| match &p.job {
            BgJob::Open { pause, .. } | BgJob::CompleteReset { pause, .. } => {
                pause.load(Ordering::Relaxed)
            }
            BgJob::Retypeset(_) => false,
        }) && let Some(pending) = s.bg_jobs.remove(idx)
        {
            return Some(pending.job);
        }
        let now = Instant::now();
        let ready = s
            .bg_jobs
            .iter()
            .position(|p| matches!(p.job, BgJob::Retypeset(_)) && p.ready_at <= now)
            .or_else(|| s.bg_jobs.iter().position(|p| p.ready_at <= now));
        if let Some(idx) = ready
            && let Some(pending) = s.bg_jobs.remove(idx)
        {
            let flag = match &pending.job {
                BgJob::Retypeset(t) => Arc::clone(&t.cancel),
                BgJob::Open { pause, .. } | BgJob::CompleteReset { pause, .. } => Arc::clone(pause),
            };
            s.bg_cancel = Some(flag);
            return Some(pending.job);
        }
        let Some(earliest) = s.bg_jobs.iter().map(|p| p.ready_at).min() else {
            s = shared
                .bg_work
                .wait(s)
                .unwrap_or_else(PoisonError::into_inner);
            continue;
        };
        let wait = earliest.saturating_duration_since(now);
        if !wait.is_zero() {
            (s, _) = shared
                .bg_work
                .wait_timeout(s, wait)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
}

fn run_bg_job(job: BgJob) -> Option<Output> {
    match job {
        BgJob::Retypeset(task) => continue_retypeset(task),
        BgJob::Open {
            ticket,
            state,
            pause,
            closed,
        } => (!closed.load(Ordering::Relaxed)).then(|| continue_open(ticket, state, &pause)),
        BgJob::CompleteReset {
            ticket,
            doc,
            typo,
            pause,
            closed,
        } => (!closed.load(Ordering::Relaxed)).then(|| complete_reset(ticket, doc, &typo, &pause)),
    }
}

fn take_job(shared: &Shared) -> Option<Job> {
    let mut s = shared.lock();
    loop {
        if s.quit {
            return None;
        }
        if let Some(job) = s.next() {
            if let Job::Work { want, .. } = &job {
                s.running.insert(*want);
            }
            return Some(job);
        }
        s = shared.work.wait(s).unwrap_or_else(PoisonError::into_inner);
    }
}

fn work_pdf(shared: &Shared, doc: Arc<Document>, first: Want) -> Option<Job> {
    let Some(pdf) = doc.as_pdf() else {
        finish(shared, Job::Work { want: first, doc });
        return None;
    };
    let mut session = pdf.session();
    let mut want = first;
    let mut runs = 0usize;
    loop {
        let out = run_pdf(&mut session, want);
        commit(shared, out);
        runs += 1;
        if runs >= PDF_SESSION_CAP || session.poisoned() || Arc::strong_count(&doc) <= 1 {
            return None;
        }
        let mut s = shared.lock();
        if s.quit {
            return None;
        }
        let next = match s.next() {
            Some(job) => job,
            None => {
                (s, _) = shared
                    .work
                    .wait_timeout(s, PDF_SESSION_IDLE)
                    .unwrap_or_else(PoisonError::into_inner);
                if s.quit {
                    return None;
                }
                s.next()?
            }
        };
        if let Job::Work { want: w, doc: d } = &next {
            s.running.insert(*w);
            if Arc::ptr_eq(d, &doc) {
                want = *w;
                continue;
            }
        }
        return Some(next);
    }
}

fn run_pdf(session: &mut RenderSession<'_>, want: Want) -> Output {
    match want {
        Want::Raster(key) => Output::Rendered {
            key,
            result: match key.part {
                Part::Whole => session.render(key.page, key.scale.value()),
                Part::Tile { col, row } => {
                    let (col, row) = (u32::from(col), u32::from(row));
                    session.render_tile(key.page, key.scale.value(), col, row)
                }
            },
        },
        Want::Text { doc, page } => Output::Read {
            doc,
            page,
            result: session.text_layer(page),
        },
        Want::Paper { doc, page } => Output::Paper {
            doc,
            page,
            result: session.paper(page),
        },
    }
}

pub(super) fn finish(shared: &Shared, job: Job) {
    let out = match job {
        Job::Open {
            ticket,
            source,
            typo,
            text,
            target,
        } => Some(match open_document(source, &typo, text.as_ref(), target) {
            Ok(Opened::Complete(doc)) => Output::Opened {
                ticket,
                result: Ok(doc),
            },
            Ok(Opened::Preview(doc, state)) => Output::OpenPreview { ticket, doc, state },
            Ok(Opened::Progress(state)) => Output::OpenProgress { ticket, state },
            Err(error) => Output::Opened {
                ticket,
                result: Err(error),
            },
        }),
        Job::Retypeset(task) => retypeset_job(task),
        Job::Background(bg) => run_bg_job(bg),
        Job::Work {
            want: Want::Raster(key),
            doc,
        } => Some(Output::Rendered {
            key,
            result: match key.part {
                Part::Whole => doc.render(key.page, key.scale.value()),
                Part::Tile { col, row } => {
                    let (col, row) = (u32::from(col), u32::from(row));
                    doc.render_tile(key.page, key.scale.value(), col, row)
                }
            },
        }),
        Job::Work {
            want: Want::Text { doc: id, page },
            doc,
        } => Some(Output::Read {
            doc: id,
            page,
            result: doc.text_layer(page),
        }),
        Job::Work {
            want: Want::Paper { doc: id, page },
            doc,
        } => Some(Output::Paper {
            doc: id,
            page,
            result: doc.paper(page),
        }),
    };
    if let Some(out) = out {
        commit(shared, out);
    }
}

fn retypeset_job(task: RetypesetTask) -> Option<Output> {
    if task.cancel.load(Ordering::Relaxed) {
        return None;
    }
    if let Some(target) = task.target.as_ref()
        && task.doc.can_preview_reset(&task.typo)
    {
        let preview =
            task.doc
                .reset_preview(&task.typo, task.inserts.as_ref(), target, &task.cancel);
        if let Some(doc) = preview {
            let primed = prime_pages(&doc, task.target.as_ref(), task.prime, &task.cancel)?;
            return Some(Output::RetypesetPreview {
                ticket: task.ticket,
                id: task.id,
                doc,
                primed,
            });
        }
        if task.cancel.load(Ordering::Relaxed) {
            return None;
        }
    }
    continue_retypeset(task)
}

fn continue_retypeset(task: RetypesetTask) -> Option<Output> {
    if task.cancel.load(Ordering::Relaxed) {
        return None;
    }
    let result = task
        .doc
        .reset_cancellable(&task.typo, task.inserts.as_ref(), &task.cancel);
    if task.cancel.load(Ordering::Relaxed) {
        return None;
    }
    let primed = match result.as_ref() {
        Some(doc) => prime_pages(doc, task.target.as_ref(), task.prime, &task.cancel)?,
        None => Primed::default(),
    };
    Some(Output::Retypeset {
        ticket: task.ticket,
        id: task.id,
        result,
        primed,
    })
}

fn prime_pages(
    doc: &Document,
    target: Option<&OpenTarget>,
    prime: Option<Prime>,
    cancel: &AtomicBool,
) -> Option<Primed> {
    let Some(prime) = prime else {
        return Some(Primed::default());
    };
    let count = doc.page_count();
    if count == 0 || prime.pages == 0 {
        return Some(Primed::default());
    }
    let focus = target
        .and_then(|t| doc.target_page(t))
        .unwrap_or(0)
        .min(count.saturating_sub(1));
    let start = if prime.spread && prime.pages > 1 {
        let lead = doc.lead();
        if focus < lead || focus == 0 {
            focus
        } else {
            lead + ((focus - lead) / 2) * 2
        }
    } else {
        focus
    };
    let end = start.saturating_add(prime.pages.min(2)).min(count);
    let mut out = Primed::default();
    for page in start..end {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let raster = doc.render(page, prime.scale.value());
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let text = doc.text_layer(page);
        out.rasters.push((page, prime.scale, raster));
        out.texts.push((page, text));
    }
    Some(out)
}

fn continue_open(ticket: u64, mut state: Box<OpenBook>, pause: &AtomicBool) -> Output {
    if pause.load(Ordering::Relaxed) {
        return Output::OpenProgress { ticket, state };
    }
    let target = state.target().cloned();
    match state.step_cancellable(pause) {
        Ok(OpenStep::Continue) => Output::OpenProgress { ticket, state },
        Ok(OpenStep::Preview(book)) => Output::OpenPreview {
            ticket,
            doc: Document::from_preview(book, target.as_ref()),
            state,
        },
        Ok(OpenStep::Complete(book)) => Output::OpenComplete {
            ticket,
            doc: Document::from_book(book),
        },
        Err(error) => Output::OpenFailed {
            ticket,
            error: OpenError::Epub(error),
        },
    }
}

fn complete_reset(ticket: u64, doc: Document, typo: &Typography, pause: &AtomicBool) -> Output {
    if pause.load(Ordering::Relaxed) {
        return Output::OpenCompleteStale { ticket, doc };
    }
    match doc.reset_cancellable(typo, None, pause) {
        Some(reset) if !pause.load(Ordering::Relaxed) => {
            Output::OpenComplete { ticket, doc: reset }
        }
        _ if pause.load(Ordering::Relaxed) => Output::OpenCompleteStale { ticket, doc },
        _ => Output::OpenFailed {
            ticket,
            error: OpenError::Epub(mareader_epub::OpenError::Invalid),
        },
    }
}

fn commit(shared: &Shared, out: Output) {
    let mut s = shared.lock();
    let keep = match out.want() {
        Some(want) => {
            s.running.remove(&want);
            s.wanted.contains(&want)
        }
        None => true,
    };
    if keep {
        s.done_bytes += out.bytes();
        s.done.push_back(out);
        drop(s);
        (shared.wake)();
    }
}

enum Opened {
    Complete(Document),
    Preview(Document, Box<OpenBook>),
    Progress(Box<OpenBook>),
}

fn open_document(
    source: Source,
    typo: &Typography,
    text: Option<&TextFile>,
    target: Option<OpenTarget>,
) -> Result<Opened, OpenError> {
    let bytes = source.read()?;
    if let Some(file) = text {
        return Document::convert(&bytes, file, typo).map(Opened::Complete);
    }
    if !mareader_epub::sniff(&bytes) {
        return Document::open(bytes, typo).map(Opened::Complete);
    }
    let mut state = OpenBook::new(bytes, typo, target)?;
    if !state.is_progressive() {
        loop {
            match state.step()? {
                OpenStep::Complete(book) => {
                    return Ok(Opened::Complete(Document::from_book(book)));
                }
                OpenStep::Continue | OpenStep::Preview(_) => {}
            }
        }
    }
    let target = state.target().cloned();
    match state.step()? {
        OpenStep::Continue => Ok(Opened::Progress(Box::new(state))),
        OpenStep::Preview(book) => Ok(Opened::Preview(
            Document::from_preview(book, target.as_ref()),
            Box::new(state),
        )),
        OpenStep::Complete(book) => Ok(Opened::Complete(Document::from_book(book))),
    }
}
