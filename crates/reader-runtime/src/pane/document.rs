//! The universal document pane, the production pane runtime: one pane,
//! one document session.

use std::cell::Cell;

use leptos::prelude::*;
use leptos::tachys::reactive_graph::OwnedView;
use runtime_contract::boundary::LaunchDocument;

use crate::context::ReaderContext;
use crate::host::contract::{
    PaneAppearance, PaneCommand, PaneEnv, PaneResourceCounts, PaneRuntime, PaneSite, PaneSurface,
    PaneTeardown,
};
use crate::host::model::{
    DocumentId, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId, PaneLifecycle,
};
use crate::pane::handle::PaneHandle;
use reader_core::document::DocStatus;
use reader_core::format::{Format, format_of};

/// The format tag a path names, for the host's descriptor.
pub fn classify(path: &str) -> PaneFormat {
    if path.is_empty() {
        return PaneFormat::Pending;
    }
    tag(format_of(path))
}

fn tag(format: Format) -> PaneFormat {
    match format {
        Format::Pdf => PaneFormat::Pdf,
        Format::Markdown => PaneFormat::Markdown,
        Format::Text => PaneFormat::Text,
    }
}

/// One document pane.
pub struct DocumentPane {
    id: PaneId,
    /// The pane's reactive owner, a child of the host's: everything it
    /// installs dies in dispose.
    owner: Owner,
    ctx: ReaderContext,
    env: PaneEnv,
    surface: PaneSurface,
    /// The format the pane was last ASKED to show, until a document lands.
    requested: Cell<PaneFormat>,
    /// `mount` builds the pane's effects and virtualizers exactly once.
    mounted: Cell<bool>,
}

impl DocumentPane {
    /// Build the pane: its owner, its reader state and context, its first
    /// open.
    pub fn create(
        env: PaneEnv,
        descriptor: PaneDescriptor,
        launch: Option<LaunchDocument>,
    ) -> Self {
        let id = descriptor.pane_id;
        let handle = PaneHandle::new(id, env.runtime);
        handle.seed_initial_zoom(descriptor.initial_zoom);
        let launch = launch
            .filter(|_| descriptor.document.is_some())
            .map(|mut launch| {
                launch.resume_page = descriptor.initial_page.max(1);
                launch
            });
        let owner = Owner::new();
        let (ctx, surface) = owner.with(|| {
            let (ctx, surface) = crate::pane::base::contexts(
                env,
                handle,
                launch.unwrap_or_else(crate::pane::base::empty_launch),
            );

            // The paper settings follow every PDF session this pane holds.
            #[cfg(feature = "pdf")]
            crate::effects::reader::blend_backdrop::paper_settings(ctx);

            // The launch the pane was created for, opened before any report.
            let first = ctx.launch.get_untracked();
            if !first.path.is_empty() {
                crate::services::document::open::open_with_launch(ctx, first);
            }
            (ctx, surface)
        });
        Self {
            id,
            owner,
            ctx,
            env,
            surface,
            requested: Cell::new(descriptor.format),
            mounted: Cell::new(false),
        }
    }

    /// The pane's reader context: a pane frame reads and writes its state.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn context(&self) -> ReaderContext {
        self.ctx
    }

    /// Build a view under a fresh child owner; the view holds it.
    fn owned(&self, site: PaneSite, build: impl FnOnce() -> AnyView) -> AnyView {
        let child = self.owner.child();
        let view = child.with(|| {
            if let Some(title_bar) = site.title_bar {
                provide_context(title_bar);
            }
            untrack(build)
        });
        OwnedView::new_with_owner(view, child).into_any()
    }
}

/// The launch that shows this document again, at the page the pane is on.
pub(crate) fn view_again(ctx: ReaderContext) -> Option<LaunchDocument> {
    let mut launch = ctx.launch.try_get_untracked()?;
    if launch.path.is_empty() {
        return None;
    }
    launch.resume_page = ctx
        .reader
        .viewer
        .page
        .try_get_untracked()
        .unwrap_or(1)
        .max(1);
    launch.saved_fraction = None;
    launch.blend_override = false;
    Some(launch)
}

/// A launch with no document, for a pane waiting for one.
impl PaneRuntime for DocumentPane {
    fn id(&self) -> PaneId {
        self.id
    }

    fn format(&self) -> PaneFormat {
        let document = &self.ctx.reader.document;
        if document
            .path
            .try_with_untracked(Option::is_none)
            .unwrap_or(true)
        {
            // Nothing landed yet: the format it was asked to open.
            return self.requested.get();
        }
        document
            .format
            .try_get_untracked()
            .map_or(PaneFormat::Pending, tag)
    }

    fn document(&self) -> Option<DocumentId> {
        let document = &self.ctx.reader.document;
        let path = document.path.try_get_untracked().flatten()?;
        let book_id = document.book_id.try_get_untracked().flatten();
        DocumentId::from_launch(book_id.as_deref(), &path)
    }

    fn lifecycle_changed(&self, lifecycle: PaneLifecycle) {
        self.ctx.pane.publish_lifecycle(lifecycle);
        // Idle thumbnail prefetch follows the host's suspension: a rail nobody
        // can see must not render.
        #[cfg(feature = "pdf")]
        match lifecycle {
            PaneLifecycle::Suspended => self.ctx.pane.pdf().suspend_prefetches(),
            PaneLifecycle::Ready => {
                // The pane in front presents: its paper is the root backdrop's.
                let pdf = self.ctx.pane.pdf();
                if self.env.active.try_get_untracked().unwrap_or(false) {
                    pdf.present();
                }
                pdf.resume_prefetches();
            }
            _ => {}
        }
    }

    fn surface(&self) -> PaneSurface {
        self.surface
    }

    fn mount(&self, bounds: PaneBounds, site: PaneSite) -> AnyView {
        if self.mounted.replace(true) {
            // A pane mounts once; a second placement would install every
            // effect twice.
            return ().into_any();
        }
        let ctx = self.ctx;
        ctx.reader.dom.set_bounds(bounds);
        let active = self.env.active;
        let request_focus = self.env.request_focus;
        // Effects and virtualizers belong to the PANE's owner, not the view.
        let rv = self
            .owner
            .with(|| untrack(|| crate::pane::view::install_pane_effects(ctx, active)));
        self.owned(site, move || {
            crate::pane::view::pane_content(ctx, rv, request_focus).into_any()
        })
    }

    fn resize(&self, bounds: PaneBounds) {
        // The host's box for this pane: the root is sized to it.
        self.ctx.reader.dom.set_bounds(bounds);
    }

    fn focus(&self) {
        // The keyboard arm reads the host's derived `active` signal; focus
        // also presents this pane's paper.
        #[cfg(feature = "pdf")]
        self.ctx.pane.pdf().present();
    }

    fn blur(&self) {
        // A pane losing focus stops its holds (an auto-scroll, a held key).
        let _ = self.ctx.reader.viewer.auto_scroll.try_set(false);
        crate::effects::reader::shortcuts::end_key_hold();
    }

    fn appearance(&self, appearance: PaneAppearance) {
        let motion = self.ctx.reader.viewer.motion;
        if motion
            .try_get_untracked()
            .is_some_and(|current| current != appearance.motion)
        {
            motion.set(appearance.motion);
        }
        // The pane's own look (independent themes): the root repaints its
        // tokens.
        let look = self.ctx.reader.viewer.look;
        if look.try_get_untracked() != Some(appearance.look) {
            look.set(appearance.look);
        }
    }

    fn command(&self, command: PaneCommand) -> Result<(), PaneError> {
        if !self.ctx.pane.lifecycle().is_live() {
            return Err(PaneError::Gone(self.id));
        }
        match command {
            PaneCommand::Open(launch) => {
                self.requested.set(classify(&launch.path));
                crate::services::document::open::open_with_launch(self.ctx, *launch);
            }
            PaneCommand::PrepareLeave => crate::services::document::prepare_leave(&self.ctx),
        }
        Ok(())
    }

    fn resources(&self) -> PaneResourceCounts {
        PaneResourceCounts {
            virtualizers: self.ctx.pane.virtualizer_count(),
            document_session: self.ctx.pane.holds_document_session(),
            zoom: self
                .ctx
                .reader
                .viewer
                .zoom
                .display
                .try_get_untracked()
                .unwrap_or(1.0),
        }
    }

    /// The pane's teardown in order: flush, end the document, virtualizers,
    /// owner cleanup, tail.
    fn dispose(&self) -> PaneTeardown {
        let ctx = self.ctx;
        // (1) Only while the pane's state is readable.
        if ctx.reader.document.status.try_get_untracked() == Some(DocStatus::Ready) {
            crate::services::document::flush_read_point(&ctx);
        }
        // (2)
        let handle = ctx.pane;
        let doc_open = ctx
            .reader
            .document
            .status
            .try_get_untracked()
            .is_some_and(|status| status != DocStatus::Idle);
        let (generation, teardown) = handle.end_document();
        if doc_open {
            crate::diagnostics::note_reader_runtime_dispose_begin();
        }
        // (3)
        let virtualizers = handle.take_virtualizers();
        // (4)
        self.owner.pause();
        self.owner.cleanup();
        crate::diagnostics::note_pane_dispose();
        // (5)
        Box::pin(async move {
            teardown.settled().await;
            for v in virtualizers {
                v.dispose();
            }
            if doc_open {
                crate::diagnostics::note_reader_runtime_dispose_complete(generation);
                app_state::memory::log_heap("close");
            }
            handle.release();
        })
    }
}
