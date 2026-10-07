//! The open handshake both pipelines share.

use std::sync::Arc;

use leptos::prelude::*;

use ai_core::gloss::GlossMark;
use reader_core::document::{DocStatus, PageSize};
use reader_core::format::Format;
use reader_core::outline::OutlineNode;
use reader_core::zoom_math::{FitMode, clamp_scale};

use crate::context::ReaderContext;
use crate::zoom::target::FitDims;

/// Which document is open, in the fields both formats have.
pub(super) struct DocumentIdentity {
    pub format: Format,
    pub path: String,
    pub title: Option<String>,
    pub author: Option<String>,
    /// The size fixed-geometry surfaces use before a page has rendered.
    pub page1_size: PageSize,
    pub outline: Option<Arc<Vec<OutlineNode>>>,
}

/// Write the document's identity; the format flips here, not in the
/// tails.
pub(super) fn identity(ctx: &ReaderContext, doc: DocumentIdentity) {
    let document = &ctx.reader.document;
    document.format.set(doc.format);
    document.path.set(Some(doc.path));
    // The title the reader shows: the document's, else the library's
    // name.
    let title = match doc.title.as_deref().map(str::trim) {
        Some(own) if reader_core::filename::is_usable_title(own) => doc.title.clone(),
        // The library's name rides the launch descriptor.
        _ => ctx
            .launch
            .with(|l| l.display_name.clone())
            .or(doc.title.clone()),
    };
    document.title.set(title);
    document.author.set(doc.author);
    document
        .outline
        .set(doc.outline.clone().unwrap_or_else(|| Arc::new(Vec::new())));
    document.outline_pending.set(doc.outline.is_none());
    document
        .content
        .metrics
        .page1_size
        .set(Some(doc.page1_size));
}

/// This document's gloss highlights, into a freshly reset ctx.
pub(super) fn load_marks(ctx: &ReaderContext) {
    ctx.reader.gloss.reset();
    let key = crate::services::document::gloss_key(*ctx);
    if key.is_empty() {
        return;
    }
    let marks: Vec<GlossMark> = storage::load_gloss().remove(&key).unwrap_or_default();
    ctx.reader.gloss.marks.set(marks);
}

/// The page to resume at, clamped to the book that opened.
pub(super) fn resume_page(saved_page: u32, num_pages: u32) -> u32 {
    saved_page.clamp(1, num_pages.max(1))
}

/// The scale to seed a fresh document at, and its fit mode.
pub(super) fn startup_scale(ctx: &ReaderContext, page_size: (f64, f64)) -> (FitMode, f64) {
    // The pane's `initial_zoom` wins over every fit, once.
    if let Some(zoom) = ctx.pane.take_initial_zoom() {
        return (FitMode::None, clamp_scale(zoom));
    }
    // The startup fit mode is a user setting, always real here.
    let startup_fit = ctx.settings.with_untracked(|s| s.layout.default_fit);
    let scale = {
        // The container is unmounted at seed time; the PANE's box is not.
        let (cw, ch) = ctx
            .reader
            .dom
            .measured_size()
            .unwrap_or_else(|| window_budget(ctx));
        FitDims::from_geometry(
            ctx.reader.viewer.mode.get_untracked(),
            (cw.max(1.0), ch.max(1.0)),
            ctx.reader.viewer.page_margin.get_untracked(),
            page_size,
        )
        .map_or(1.0, |dims| dims.fit(startup_fit, 1.0))
    };
    (startup_fit, scale)
}

/// The box an unmeasured pane will get, from the window.
fn window_budget(ctx: &ReaderContext) -> (f64, f64) {
    const DOCKED_RAIL_W: f64 = 288.0;
    let (vw, vh) = app_chrome::hooks::use_viewport::viewport_size();
    let docked = !ctx.settings.with_untracked(|s| s.layout.sidebar_overlay)
        && ctx.ui.sidebar.get_untracked() != app_state::state::SidebarMode::None;
    (vw - if docked { DOCKED_RAIL_W } else { 0.0 }, vh)
}

/// The document is open: flip the route LAST.
pub(super) fn enter_ready(state: crate::context::ReaderContext) {
    state.reader.document.error.set(None);
    state.reader.document.status.set(DocStatus::Ready);
    state.ui.toast.set(None);
    state.reader.search.reset();
}
