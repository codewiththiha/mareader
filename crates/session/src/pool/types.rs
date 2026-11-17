//! Queue jobs, background tasks, and worker outputs for [`super::Pool`].

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use mareader_epub::{Inserts, OpenBook, OpenTarget};
use mareader_pdf::{Paper, Raster, RenderError, TextLayer};
use mareader_reflow::Typography;

use crate::document::TextFile;
use crate::key::{DocId, PageKey, Scale, Want};
use crate::{Document, OpenError, Prime, Source};

pub(crate) enum Job {
    Open {
        ticket: u64,
        source: Source,
        typo: Arc<Typography>,
        /// Set when the file is text to convert first.
        text: Option<TextFile>,
        target: Option<OpenTarget>,
    },
    /// `doc`, open as `id`, set again in `typo`, maybe with `inserts`.
    Retypeset(RetypesetTask),
    /// Inline fallback for background full-book layout when threadless.
    Background(BgJob),
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
    pub prime: Option<Prime>,
    pub cancel: Arc<AtomicBool>,
}

pub(crate) enum BgJob {
    Retypeset(RetypesetTask),
    Open {
        ticket: u64,
        state: Box<OpenBook>,
        pause: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
    },
    CompleteReset {
        ticket: u64,
        doc: Document,
        typo: Arc<Typography>,
        pause: Arc<AtomicBool>,
        closed: Arc<AtomicBool>,
    },
}

impl BgJob {
    pub(super) fn is_open_ticket(&self, ticket: u64) -> bool {
        matches!(
            self,
            Self::Open { ticket: q, .. } | Self::CompleteReset { ticket: q, .. } if *q == ticket
        )
    }

    pub(super) fn is_retypeset_ticket(&self, ticket: u64) -> bool {
        matches!(self, Self::Retypeset(t) if t.ticket == ticket)
    }
}

pub(super) struct PendingBg {
    pub ready_at: Instant,
    pub job: BgJob,
}

#[derive(Default)]
pub(crate) struct Primed {
    pub rasters: Vec<(usize, Scale, Result<Raster, RenderError>)>,
    pub texts: Vec<(usize, Result<TextLayer, RenderError>)>,
}

impl Primed {
    pub(crate) fn bytes(&self) -> usize {
        let rasters: usize = self
            .rasters
            .iter()
            .filter_map(|(_, _, r)| r.as_ref().ok().map(Raster::bytes))
            .sum();
        let texts: usize = self
            .texts
            .iter()
            .filter_map(|(_, t)| t.as_ref().ok().map(TextLayer::bytes))
            .sum();
        rasters.saturating_add(texts)
    }
}

impl Job {
    /// Opens and re-sets jump the queue and its backlog limit.
    pub(super) fn urgent(&self) -> bool {
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
    /// Full book already parsed, paused before matching a newer typography.
    OpenCompleteStale {
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
        primed: Primed,
    },
    /// None when the book wouldn't set; the old one stays.
    Retypeset {
        ticket: u64,
        id: DocId,
        result: Option<Document>,
        primed: Primed,
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
            Self::RetypesetPreview { primed, .. } | Self::Retypeset { primed, .. } => {
                primed.bytes()
            }
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
            | Self::OpenCompleteStale { .. }
            | Self::OpenFailed { .. }
            | Self::RetypesetPreview { .. }
            | Self::Retypeset { .. } => None,
        }
    }
}
