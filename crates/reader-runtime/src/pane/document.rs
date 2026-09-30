//! The universal document pane: the production pane runtime. One pane holds
//! ONE document session — PDF, Markdown or plain text, through the reader's
//! one pipeline — with its own reader state, virtualizers, zoom, search,
//! overlays and listeners, all under the pane's own reactive owner.
//!
//! It implements [`PaneRuntime`] and is handed to the host only through
//! [`factory`], which the session's composition root injects: the host
//! never names this type.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;
use leptos::tachys::reactive_graph::OwnedView;
use runtime_contract::boundary::LaunchDocument;

use crate::context::ReaderContext;
use crate::host::contract::{
    ChromeSlot, PaneAppearance, PaneCommand, PaneDocStatus, PaneEnv, PaneFactory,
    PaneResourceCounts, PaneRuntime, PaneSite, PaneSurface, PaneTeardown,
};
use crate::host::model::{
    DocumentId, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId, PaneLifecycle,
};
use crate::pane::handle::PaneHandle;
use crate::state::ReaderState;
use pdf_engine::types::DocStatus;
use reader_core::format::{Format, format_of};

/// The factory the composition root hands the host.
pub fn factory() -> PaneFactory {
    Rc::new(build)
}

/// The format tag a path names, for the host's descriptor: the host asks
/// through the injected [`crate::host::contract::PaneClassifier`] and never
/// reads an extension itself.
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

fn build(
    env: PaneEnv,
    descriptor: PaneDescriptor,
    launch: Option<LaunchDocument>,
) -> Rc<dyn PaneRuntime> {
    Rc::new(DocumentPane::create(env, descriptor, launch))
}

/// One document pane.
pub(crate) struct DocumentPane {
    id: PaneId,
    /// The pane's reactive owner, a child of the host's: every signal,
    /// effect, listener and timer the pane installs lives here and dies in
    /// [`PaneRuntime::dispose`] — explicitly, not whenever a view happens to
    /// unmount.
    owner: Owner,
    ctx: ReaderContext,
    env: PaneEnv,
    surface: PaneSurface,
    /// The format the pane was last ASKED to show (its descriptor's, then
    /// each in-place open's): what [`PaneRuntime::format`] answers while no
    /// document has landed yet.
    requested: Cell<PaneFormat>,
    /// `mount` builds the pane's effects and virtualizers exactly once.
    mounted: Cell<bool>,
}

impl DocumentPane {
    /// Build the pane: its owner, its own reader state and context, and its
    /// first open. The manager calls the factory inside the host's owner, so
    /// the handle's slot lands in the host's arena and the pane's owner is
    /// the host's child.
    ///
    /// The DESCRIPTOR is what the pane was asked for: the launch opens only
    /// when the descriptor names a document, it resumes at the descriptor's
    /// page, and the descriptor's zoom (if any) seeds the first document in
    /// place of the settings' fit.
    fn create(env: PaneEnv, descriptor: PaneDescriptor, launch: Option<LaunchDocument>) -> Self {
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
            // The pane's handle, for the components that register what they
            // create with the pane (the reflow stream's and the thumbnail
            // rail's virtualizers).
            provide_context(handle);
            let reader = ReaderState::new(handle);
            let launch = RwSignal::new(launch.unwrap_or_else(empty_launch));
            let ctx = ReaderContext {
                reader,
                pane: handle,
                settings: env.settings,
                ui: env.ui,
                api: env.api,
                launch,
                id: env.session_id,
                chrome: env.chrome,
                open: env.open,
                can_split: env.can_split,
            };
            let status = reader.document.status;
            let error = reader.document.error;
            let surface = PaneSurface {
                status: Signal::derive(move || neutral_status(status.get())),
                error: Signal::derive(move || error.get()),
                page: reader.viewer.page.into(),
                reflowable: Signal::derive(move || reader.reflowable()),
                search_visible: reader.search.visible.into(),
                name: Signal::derive(move || reader.document.display_name()),
            };

            // The paper settings follow every change onto whatever PDF
            // session the pane holds; each new session is configured by the
            // open flow's seed before its first frame.
            crate::effects::reader::blend_backdrop::paper_settings(ctx);

            // The launch the pane was created for: opened by the pane that
            // owns the document, before the host's first status report, so
            // the session's first word to the Shell is `Opening`.
            let first = launch.get_untracked();
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

    /// Build a view under a fresh child of the pane's owner, re-providing
    /// the placement site's chrome context. The view holds the child owner:
    /// dropping the view (the host re-placed the region) releases it, and
    /// the pane's dispose releases it too.
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

/// The launch that shows `ctx`'s document again, at the page the pane is on:
/// the Split menu's "beside" opens this in a NEW pane, which runs its own
/// session. `None` while the pane holds no document.
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
fn empty_launch() -> LaunchDocument {
    LaunchDocument {
        book_id: None,
        path: String::new(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: None,
    }
}

/// The engine's status in the host's words.
fn neutral_status(status: DocStatus) -> PaneDocStatus {
    match status {
        DocStatus::Idle => PaneDocStatus::Idle,
        DocStatus::Opening => PaneDocStatus::Opening,
        DocStatus::Ready => PaneDocStatus::Ready,
        DocStatus::Error => PaneDocStatus::Error,
    }
}

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
        // Idle thumbnail prefetch follows the HOST's suspension (which
        // follows the frame's slot): a suspended pane is KEPT (document
        // loaded, for an instant reopen), but a rail nobody can see must not
        // render while the shelf is being revealed — suspending abandons
        // queued and in-flight prefetches, resuming lets them run. A warm
        // session's pane goes Ready then straight to Suspended, so it starts
        // parked. The switch is THIS pane's session's: another pane's
        // prefetch is untouched. (The view is taken after the lifecycle was
        // published, so a resume sees the pane already admitting work.)
        match lifecycle {
            PaneLifecycle::Suspended => self.ctx.pane.pdf().suspend_prefetches(),
            PaneLifecycle::Ready => {
                // The pane in front presents: its session's paper is the one
                // the root backdrop shows. With several panes Ready (a split
                // resuming together), only the ACTIVE one does; another pane
                // presents when it takes focus (`focus`).
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
        // The pane's effects and virtualizers belong to the PANE's owner —
        // they live until the pane's dispose, whatever the host does with
        // the view.
        let rv = self
            .owner
            .with(|| untrack(|| crate::pane::view::install_pane_effects(ctx, active)));
        self.owned(site, move || {
            crate::pane::view::pane_content(ctx, rv, request_focus).into_any()
        })
    }

    fn chrome(&self, slot: ChromeSlot, site: PaneSite) -> Option<AnyView> {
        let ctx = self.ctx;
        let settings_open = self.env.settings_open;
        let view = match slot {
            ChromeSlot::TitleCenter => self.owned(site, move || {
                view! {
                    <crate::components::shell::titlebar::document_title::CenteredDocTitle
                        state=ctx
                    />
                }
                .into_any()
            }),
            ChromeSlot::TitleTrailing => self.owned(site, move || {
                view! {
                    <crate::components::menus::reader_menu::ReaderMenu
                        state=ctx
                        settings_open=settings_open
                    />
                }
                .into_any()
            }),
            ChromeSlot::Rail => self.owned(site, move || {
                let shell =
                    expect_context::<app_ui::components::shell::controller::ShellController>();
                view! { <crate::features::rail::ReaderRail state=ctx shell=shell /> }.into_any()
            }),
            ChromeSlot::Settings => self.owned(site, move || {
                view! {
                    <crate::components::settings::modal::SettingsModal
                        state=ctx
                        open=settings_open
                    />
                }
                .into_any()
            }),
        };
        Some(view)
    }

    fn resize(&self, bounds: PaneBounds) {
        // The host's box for this pane: its root is sized to it, the
        // startup fit of the next open budgets against it, and the chrome it
        // paints over (the floating title) re-measures on it. Everything
        // INSIDE is the pane's own geometry: its viewport measures itself
        // and the zoom follow re-fits from that measurement.
        self.ctx.reader.dom.set_bounds(bounds);
    }

    fn focus(&self) {
        // The keyboard arm and every other active-only behaviour read the
        // host's derived `active` signal; nothing to copy here. What does
        // follow focus is the realm's one presented session: the root
        // backdrop shows the paper of the pane in front. (A pane not Ready
        // has nothing to present; its Ready presents if it is still active.)
        self.ctx.pane.pdf().present();
    }

    fn blur(&self) {
        // A pane losing focus stops its holds (an auto-scroll, a held
        // key): the keyboard arm already stands down on the derived signal,
        // so the keyup that would have ended a hold goes elsewhere.
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
        // The pane's own look (independent themes): the pane root repaints
        // its tokens from this — or removes them, back to inheritance.
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

    /// The pane's teardown, in dependency order:
    ///
    /// 1. the durable read point, written while the pane's state still
    ///    exists (the Shell owns the library blob);
    /// 2. the pane's document generation claimed (an open still in flight
    ///    can no longer land) and its format session taken out of the pane
    ///    and disposed — the same dispose every format and every view mode
    ///    ends through: from here the session refuses every call (a
    ///    Markdown/text session has already released its content; a PDF
    ///    session has stopped accepting, invalidated its paper and retained
    ///    its search index), and what is still in flight — the PDF engine
    ///    session's destroy, its rasters, lanes, workers and page
    ///    registrations with it — is the `Retiring` handed to the tail;
    /// 3. the virtualizers taken out of the registry — from here the tail
    ///    alone owns them;
    /// 4. the pane's owner cleaned up: every effect, listener (the keyboard
    ///    arm's window listeners, which end a key hold still gliding),
    ///    observer, timer and signal the pane installed is released NOW,
    ///    with its view's child owners — and the per-pane memos (the gloss
    ///    spot memo, the measurement inbox) with them: they are the pane's
    ///    state, not thread-locals a recycled frame would carry over.
    ///    The owner is PAUSED first: a render effect lives with its mounted
    ///    view, not with the owner, and the pane's view stays mounted in the
    ///    host's slot until the host's next render. One the teardown above
    ///    notified (a closed session clears what the view reads) would
    ///    still run then, against the purged arena; paused, it never runs
    ///    again. A whole-reader dispose unmounts first; a single pane's
    ///    close, with the workspace living on, is the case this covers;
    /// 5. the tail: the session's release awaited, the virtualizers'
    ///    final dispose, the completion reported, and the pane's handle slot
    ///    released (its gates read `Disposed` from then).
    ///
    /// The tail captures only what it still has to release — the session's
    /// teardown, the virtualizers, the generation, the pane handle (Copy) — never
    /// the pane itself: the manager drops the pane object as soon as this
    /// returns, so its owner shell and context map go with the sync half.
    fn dispose(&self) -> PaneTeardown {
        let ctx = self.ctx;
        // (1) Only while the pane's state is readable: a pane whose owner
        // the session's unmount already reached has nothing left to say.
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
            crate::diagnostics::note_reader_runtime_dispose_begin(generation);
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
