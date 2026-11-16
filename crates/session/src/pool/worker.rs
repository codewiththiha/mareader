//! Worker loop and job execution, reusing hayro caches across PDF jobs.

use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use mareader_epub::{OpenBook, OpenStep, OpenTarget};
use mareader_pdf::RenderSession;
use mareader_reflow::Typography;

use super::{Job, Output, RetypesetTask, Shared};
use crate::document::TextFile;
use crate::key::{Part, Want};
use crate::{Document, OpenError, Source};

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

/// Background thread for full-book layout after retypeset previews.
pub(super) fn work_retype_bg(shared: &Shared) {
    loop {
        let Some(task) = take_bg_retypeset(shared) else {
            return;
        };
        if let Some(out) = continue_retypeset(task) {
            commit(shared, out);
        }
        shared.lock().bg_cancel = None;
    }
}

fn take_bg_retypeset(shared: &Shared) -> Option<RetypesetTask> {
    let mut s = shared.lock();
    loop {
        if s.quit {
            return None;
        }
        while s
            .bg_retypeset
            .front()
            .is_some_and(|p| p.task.cancel.load(Ordering::Relaxed))
        {
            s.bg_retypeset.pop_front();
        }
        let Some(ready_at) = s.bg_retypeset.front().map(|p| p.ready_at) else {
            s = shared
                .bg_work
                .wait(s)
                .unwrap_or_else(PoisonError::into_inner);
            continue;
        };
        let wait = ready_at.saturating_duration_since(Instant::now());
        if !wait.is_zero() {
            (s, _) = shared
                .bg_work
                .wait_timeout(s, wait)
                .unwrap_or_else(PoisonError::into_inner);
            continue;
        }
        if let Some(pending) = s.bg_retypeset.pop_front() {
            if pending.task.cancel.load(Ordering::Relaxed) {
                continue;
            }
            s.bg_cancel = Some(Arc::clone(&pending.task.cancel));
            return Some(pending.task);
        }
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
        Job::ContinueOpen { ticket, state } => Some(continue_open(ticket, state)),
        Job::Retypeset(task) => retypeset_job(task),
        Job::ContinueRetypeset(task) => continue_retypeset(task),
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
            return Some(Output::RetypesetPreview {
                ticket: task.ticket,
                id: task.id,
                doc,
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
    Some(Output::Retypeset {
        ticket: task.ticket,
        id: task.id,
        result,
    })
}

fn continue_open(ticket: u64, mut state: Box<OpenBook>) -> Output {
    let target = state.target().cloned();
    match state.step() {
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
