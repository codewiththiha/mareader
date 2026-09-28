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
use crate::services::document::session;
use crate::state::ReaderState;
use pdf_engine::types::DocStatus;
use reader_core::format::Format;

/// The factory the composition root hands the host.
pub fn factory() -> PaneFactory {
    Rc::new(build)
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
    /// `mount` builds the pane's effects and virtualizers exactly once.
    mounted: Cell<bool>,
}

impl DocumentPane {
    /// Build the pane: its owner, its own reader state and context, and its
    /// first open. The manager calls the factory inside the host's owner, so
    /// the handle's slot lands in the host's arena and the pane's owner is
    /// the host's child.
    fn create(env: PaneEnv, descriptor: PaneDescriptor, launch: Option<LaunchDocument>) -> Self {
        let id = descriptor.pane_id;
        let handle = PaneHandle::new(id, env.runtime);
        let owner = Owner::new();
        let (ctx, surface) = owner.with(|| {
            // The pane's handle, for the components that register what they
            // create with the pane (the reflow stream's and the thumbnail
            // rail's virtualizers).
            provide_context(handle);
            let reader = ReaderState::default();
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
            };
            let status = reader.document.status;
            let error = reader.document.error;
            let surface = PaneSurface {
                status: Signal::derive(move || neutral_status(status.get())),
                error: Signal::derive(move || error.get()),
                page: reader.viewer.page.into(),
                reflowable: Signal::derive(move || reader.reflowable()),
                search_visible: reader.search.visible.into(),
            };

            // The paper session's blend switch and detection area, sent
            // BEFORE the first open: the first book's first frame publishes
            // only if the session already knows `blend_on`.
            crate::effects::reader::blend_backdrop::paper_settings(ctx);

            // Idle thumbnail prefetch follows the frame's slot. A closed
            // reader is KEPT (document loaded, for an instant reopen), but a
            // rail nobody can see must not render while the shelf is being
            // revealed: leaving the screen abandons queued and in-flight
            // prefetches, coming back lets them run. The first run applies the
            // slot this session booted into — a warm session starts parked.
            let frame_active = app_chrome::hooks::frame_active::use_frame_active();
            Effect::new(move |_| {
                if frame_active.get() {
                    pdf_engine::api::resume_prefetches();
                } else {
                    pdf_engine::api::suspend_prefetches();
                }
            });

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
            return PaneFormat::Pending;
        }
        match document.format.try_get_untracked() {
            Some(Format::Pdf) => PaneFormat::Pdf,
            Some(Format::Markdown) => PaneFormat::Markdown,
            Some(Format::Text) => PaneFormat::Text,
            None => PaneFormat::Pending,
        }
    }

    fn document(&self) -> Option<DocumentId> {
        let document = &self.ctx.reader.document;
        let path = document.path.try_get_untracked().flatten()?;
        let book_id = document.book_id.try_get_untracked().flatten();
        DocumentId::from_launch(book_id.as_deref(), &path)
    }

    fn lifecycle_changed(&self, lifecycle: PaneLifecycle) {
        self.ctx.pane.publish_lifecycle(lifecycle);
    }

    fn surface(&self) -> PaneSurface {
        self.surface
    }

    fn mount(&self, _bounds: PaneBounds, site: PaneSite) -> AnyView {
        if self.mounted.replace(true) {
            // A pane mounts once; a second placement would install every
            // effect twice.
            return ().into_any();
        }
        let ctx = self.ctx;
        let active = self.env.active;
        // The pane's effects and virtualizers belong to the PANE's owner —
        // they live until the pane's dispose, whatever the host does with
        // the view.
        let rv = self
            .owner
            .with(|| untrack(|| crate::pane::view::install_pane_effects(ctx, active)));
        self.owned(site, move || {
            crate::pane::view::pane_content(ctx, rv).into_any()
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

    fn resize(&self, _bounds: PaneBounds) {
        // The pane fills the box the host placed it in (`absolute inset-0`
        // inside the host's entry). Everything inside is the pane's own
        // geometry: its viewport measures itself and the zoom follow
        // re-fits from that measurement — the host's number is never a
        // second source for it.
    }

    fn focus(&self) {
        // The keyboard arm and every other active-only behaviour read the
        // host's derived `active` signal; nothing to copy here.
    }

    fn blur(&self) {
        // A pane losing focus stops its holds (an auto-scroll, a held
        // key): the keyboard arm already stands down on the derived signal.
        let _ = self.ctx.reader.viewer.auto_scroll.try_set(false);
    }

    fn appearance(&self, appearance: PaneAppearance) {
        let motion = self.ctx.reader.viewer.motion;
        if motion
            .try_get_untracked()
            .is_some_and(|current| current != appearance.motion)
        {
            motion.set(appearance.motion);
        }
    }

    fn command(&self, command: PaneCommand) -> Result<(), PaneError> {
        if !self.ctx.pane.lifecycle().is_live() {
            return Err(PaneError::Gone(self.id));
        }
        match command {
            PaneCommand::Open(launch) => {
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
        }
    }

    /// The pane's teardown, in dependency order:
    ///
    /// 1. the durable read point, written while the pane's state still
    ///    exists (the Shell owns the library blob);
    /// 2. the document session claimed (a newer open's stale tail can no
    ///    longer land) and the paper session closed;
    /// 3. the per-document memos outside the reactive tree forgotten;
    /// 4. the virtualizers taken out of the registry — from here the tail
    ///    alone owns them;
    /// 5. the pane's owner cleaned up: every effect, listener (the keyboard
    ///    arm's window listeners), observer, timer and signal the pane
    ///    installed is released NOW, with its view's child owners;
    /// 6. the tail: the engine destroy awaited, the sweeps, the
    ///    virtualizers' final dispose, the completion reported.
    fn dispose(&self) -> PaneTeardown {
        let ctx = self.ctx;
        // (1) Only while the pane's state is readable: a pane whose owner
        // the session's unmount already reached has nothing left to say.
        if ctx.reader.document.status.try_get_untracked() == Some(DocStatus::Ready) {
            crate::services::document::flush_read_point(&ctx);
        }
        // (2)
        let doc_open = ctx
            .reader
            .document
            .status
            .try_get_untracked()
            .is_some_and(|status| status != DocStatus::Idle);
        let stamp = doc_open.then(session::claim);
        if let Some(stamp) = stamp {
            crate::diagnostics::note_reader_runtime_dispose_begin(stamp);
        }
        pdf_engine::backdrop::document_close();
        // (3) In a hosted frame the thread-locals survive the session (the
        // frame is recycled, not reloaded): a memo nobody clears is memory
        // the next pane pays for without using.
        crate::components::ai::reflow_anchor::forget_parsed_spots();
        // (4)
        let virtualizers = ctx.pane.take_virtualizers();
        let pdf = ctx.pane.pdf();
        let held = ctx.pane.holds_document_session();
        ctx.pane.note_document_session(false);
        // (5)
        self.owner.cleanup();
        crate::diagnostics::note_pane_dispose();
        // (6)
        Box::pin(async move {
            if stamp.is_some() || held {
                pdf.destroy().await;
                pdf.sweep();
                pdf.sweep_snapshots();
            }
            for v in virtualizers {
                v.dispose();
            }
            if let Some(stamp) = stamp {
                crate::diagnostics::note_reader_runtime_dispose_complete(stamp);
                app_state::memory::log_heap("close");
            }
        })
    }
}
