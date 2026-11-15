//! Worker loop and job execution, reusing hayro caches across PDF jobs.

use std::sync::{Arc, PoisonError};
use std::time::Duration;

use mareader_epub::{OpenBook, OpenStep, OpenTarget};
use mareader_pdf::RenderSession;
use mareader_reflow::Typography;

use super::{Job, Output, Shared};
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
        } => match open_document(source, &typo, text.as_ref(), target) {
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
        },
        Job::ContinueOpen { ticket, state } => continue_open(ticket, state),
        Job::Retypeset {
            ticket,
            id,
            doc,
            typo,
            inserts,
        } => Output::Retypeset {
            ticket,
            id,
            result: doc.reset(&typo, inserts.as_ref()),
        },
        Job::Work {
            want: Want::Raster(key),
            doc,
        } => Output::Rendered {
            key,
            result: match key.part {
                Part::Whole => doc.render(key.page, key.scale.value()),
                Part::Tile { col, row } => {
                    let (col, row) = (u32::from(col), u32::from(row));
                    doc.render_tile(key.page, key.scale.value(), col, row)
                }
            },
        },
        Job::Work {
            want: Want::Text { doc: id, page },
            doc,
        } => Output::Read {
            doc: id,
            page,
            result: doc.text_layer(page),
        },
        Job::Work {
            want: Want::Paper { doc: id, page },
            doc,
        } => Output::Paper {
            doc: id,
            page,
            result: doc.paper(page),
        },
    };
    commit(shared, out);
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
