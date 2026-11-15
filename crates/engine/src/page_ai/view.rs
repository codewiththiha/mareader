//! What a page's AI run sees above it, and what only the front end answers.
//!
//! A run works from [`AiView`], one pane's document as of the last frame, and
//! shows what it is doing as a [`Pill`]. Two things it cannot know itself: how
//! a pane's pages read now, which [`Views`] answers, and how many tasks at once
//! the attached model takes, which [`Models`] answers. Both are the front end's
//! to give, so a run takes them as traits and never names a window.

use std::ops::Range;
use std::sync::Arc;

use mareader_library::{AiTask, PageSpan};
use mareader_session::{DocId, Document, TextLayer, Typography};
use mareader_settings::{ModelPreset, PageTask};
use mareader_workspace::PaneId;

use super::extract::{self, Source};
use super::state::Handle;

/// One pane's document as the page AI sees it, as of last frame.
#[derive(Clone, Debug)]
pub struct AiView {
    pub doc: DocId,
    /// Names the pages the AI counts in: a book's own, results aside.
    pub layout: u64,
    pub document: Arc<Document>,
    /// What a result is set in.
    pub typo: Arc<Typography>,
    pub visible: Range<usize>,
    /// The page read now.
    pub current: usize,
    pub zoom: f32,
}

/// How a continuous run's ring stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ring {
    /// The stay on this page so far, 0 to 1.
    Fill(f32),
    /// This page is being written.
    Spin,
    /// Nothing to wait for: the pages ahead are asked.
    Full,
    Paused,
}

/// A pane's continuous run, shown at its bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Pill {
    pub text: String,
    /// What it is doing, in a sentence.
    pub tip: String,
    pub ring: Ring,
    /// It asks whether to carry on here.
    pub ask: bool,
}

/// What a press on the pill asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillPress {
    Stop,
    Continue,
}

/// The panes' documents, as the front end has them this frame.
pub trait Views {
    /// One pane's document as a run sees it; none if the pane shows no book.
    fn ai_view(&self, pane: PaneId) -> Option<AiView>;

    /// A page's own text, once it has been read; none while it is loading.
    fn text_layer(&self, doc: DocId, page: usize) -> Option<Arc<TextLayer>>;
}

/// The model a run asks, which only the service above knows.
pub trait Models {
    /// How many tasks at once the attached model takes; none without one.
    fn capacity(&self, preset: &ModelPreset) -> Option<u32>;

    /// Starts `task` over `source`'s `pages`; the writer answers as it goes.
    fn start(&self, task: &AiTask, source: &Source, pages: usize) -> Result<Handle, &'static str>;
}

/// The settings' task a kept task belongs to.
pub fn page_task(task: &AiTask) -> PageTask {
    match task {
        AiTask::Summary | AiTask::Points => PageTask::Summarize,
        AiTask::Translate { .. } => PageTask::Translate,
        AiTask::Simplify => PageTask::Simplify,
    }
}

/// A book's own pages `from` to `to` as one span of its text.
pub fn group_span(document: &Document, (from, to): (usize, usize)) -> Option<[[usize; 2]; 2]> {
    let Document::Epub { book, .. } = document else {
        return None;
    };
    let excerpts: Vec<_> = (from..=to).flat_map(|p| book.base_excerpts(p)).collect();
    extract::span(&excerpts, book.blocks())
}

/// Own page `page`'s text, to send or show beside a result.
pub fn source_of(
    view: &AiView,
    documents: &dyn Views,
    page: usize,
) -> Result<(Source, PageSpan), &'static str> {
    match &*view.document {
        Document::Epub { book, .. } => {
            let blocks = book.blocks();
            let span = extract::span(&book.base_excerpts(page), blocks).ok_or("blank")?;
            let source = extract::book(blocks, span);
            let [from, to] = span;
            Ok((source, PageSpan::Blocks { from, to }))
        }
        Document::Pdf(_) => {
            let layer = documents.text_layer(view.doc, page).ok_or("loading")?;
            let near: Vec<_> = [page.checked_sub(1), Some(page + 1)]
                .into_iter()
                .flatten()
                .filter_map(|p| documents.text_layer(view.doc, p))
                .collect();
            let near: Vec<_> = near.iter().map(|l| &l.text).collect();
            Ok((extract::pdf(&layer.text, &near), PageSpan::Page { page }))
        }
    }
}
