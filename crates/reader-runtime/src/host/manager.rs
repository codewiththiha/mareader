//! The pane manager: the reactive layer over [`PaneManagerCore`].
//!
//! The core decides every transition (ids, lifecycle, the one active pane,
//! the focus hand-over); this layer carries the decisions out on the pane
//! runtimes and publishes the two facts the host's views follow: which panes
//! are placed, and which one is active. Those two signals are PUBLICATIONS of
//! the core's answer, written only here — there is no second focus store.
//!
//! Borrow discipline: the shared state is a `RefCell`, and no borrow is ever
//! held across a call into a pane runtime (a pane may call back into the
//! host — a focus request, a diagnostics snapshot — from inside any of its
//! methods).
//!
//! Lifetime discipline: the manager never holds the session's reactive
//! owner, and it lets go of a pane the moment the pane's dispose returns.
//! The session's owner stays single-rooted — the unmount handle holds the
//! only strong reference, so the unmount alone releases the whole tree,
//! explicit disposal or not — and a disposing pane's tail owns only what
//! it still has to release, never the pane object itself.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use leptos::prelude::*;
use runtime_contract::boundary::LaunchDocument;
use wasm_bindgen_futures::spawn_local;

use super::contract::{PaneEnv, PaneFactory, PaneRuntime, PaneTeardown};
use super::model::{
    FocusChange, PaneBounds, PaneError, PaneId, PaneLifecycle, PaneManagerCore, PaneRequest,
};

/// The manager's plain-Rust state. Reached through an `Rc` so a disposal
/// tail can finish its bookkeeping after the host's arena is gone.
pub(crate) struct ManagerState {
    pub(crate) core: PaneManagerCore,
    /// The live pane runtimes, by id. A pane leaves this map the moment its
    /// dispose begins: from then on its teardown tail alone holds it.
    pub(crate) panes: BTreeMap<PaneId, Rc<dyn PaneRuntime>>,
    /// Disposal tails the session's runtime has yet to await (a workspace
    /// disposal hands them over; an in-session close spawns its own).
    teardowns: Vec<PaneTeardown>,
    factory: PaneFactory,
}

type Shared = Rc<RefCell<ManagerState>>;

/// The host's pane manager. Copy: views and callbacks capture it.
#[derive(Clone, Copy)]
pub struct PaneManager {
    shared: StoredValue<Shared, LocalStorage>,
    /// The active pane, as the core decided it. The ONE focus publication.
    active: RwSignal<Option<PaneId>>,
    /// The live panes in placement order, as the core holds them.
    placed: RwSignal<Vec<PaneId>>,
    /// Each live pane's box, as the core last recorded it: what the host's
    /// view positions each pane's entry by. Pruned with the placement.
    bounds: RwSignal<BTreeMap<PaneId, PaneBounds>>,
}

impl PaneManager {
    /// A manager whose panes the injected factory builds. Call inside the
    /// host's owner (its signals live in the host's arena).
    pub(crate) fn new(factory: PaneFactory) -> Self {
        let shared = Rc::new(RefCell::new(ManagerState {
            core: PaneManagerCore::new(),
            panes: BTreeMap::new(),
            teardowns: Vec::new(),
            factory,
        }));
        Self {
            shared: StoredValue::new_local(shared),
            active: RwSignal::new(None),
            placed: RwSignal::new(Vec::new()),
            bounds: RwSignal::new(BTreeMap::new()),
        }
    }

    /// The shared state, while the host's arena is alive.
    pub(crate) fn shared(&self) -> Option<Shared> {
        self.shared.try_get_value()
    }

    /// The active pane (tracked).
    pub fn active(&self) -> Option<PaneId> {
        self.active.try_get().flatten()
    }

    /// The active pane (untracked).
    pub fn active_untracked(&self) -> Option<PaneId> {
        self.active.try_get_untracked().flatten()
    }

    /// The live panes in placement order (tracked).
    pub fn placed(&self) -> Vec<PaneId> {
        self.placed.try_get().unwrap_or_default()
    }

    /// The box the host last handed `id` (tracked). `None` until the host
    /// measured one — the view falls back to filling the slot.
    pub fn bounds_of(&self, id: PaneId) -> Option<PaneBounds> {
        self.bounds
            .try_with(|bounds| bounds.get(&id).copied())
            .flatten()
    }

    /// The box the host last handed `id` (untracked).
    pub fn bounds_untracked(&self, id: PaneId) -> Option<PaneBounds> {
        self.bounds
            .try_with_untracked(|bounds| bounds.get(&id).copied())
            .flatten()
    }

    /// The live runtime for `id`, cloned out of the map (no borrow held).
    pub fn pane(&self, id: PaneId) -> Option<Rc<dyn PaneRuntime>> {
        self.shared()?.borrow().panes.get(&id).cloned()
    }

    /// The active pane's runtime (tracked on the active id).
    pub fn active_pane(&self) -> Option<Rc<dyn PaneRuntime>> {
        self.active().and_then(|id| self.pane(id))
    }

    /// Every live runtime, in placement order.
    pub fn live_panes(&self) -> Vec<Rc<dyn PaneRuntime>> {
        let Some(shared) = self.shared() else {
            return Vec::new();
        };
        let state = shared.borrow();
        state
            .core
            .live()
            .iter()
            .filter_map(|id| state.panes.get(id).cloned())
            .collect()
    }

    /// Republish the core's placement and active pane.
    fn publish(&self, shared: &Shared) {
        let (live, active) = {
            let state = shared.borrow();
            (state.core.live().to_vec(), state.core.active())
        };
        // A pane that left the placement leaves the bounds publication too.
        let stale = self
            .bounds
            .try_with_untracked(|bounds| bounds.keys().any(|id| !live.contains(id)))
            .unwrap_or(false);
        if stale {
            self.bounds
                .try_update(|bounds| bounds.retain(|id, _| live.contains(id)));
        }
        if self.placed.try_get_untracked().as_ref() != Some(&live) {
            self.placed.try_set(live);
        }
        if self.active.try_get_untracked() != Some(active) {
            self.active.try_set(active);
        }
    }

    /// Run one core transition for `id` and, when it succeeded, mirror the
    /// resulting lifecycle into the pane.
    fn transition(
        &self,
        shared: &Shared,
        id: PaneId,
        step: impl FnOnce(&mut PaneManagerCore) -> Result<(), PaneError>,
    ) -> Result<(), PaneError> {
        let lifecycle = {
            let mut state = shared.borrow_mut();
            step(&mut state.core)?;
            state.core.lifecycle(id)
        };
        if let (Some(lifecycle), Some(pane)) = (lifecycle, self.pane(id)) {
            pane.lifecycle_changed(lifecycle);
        }
        Ok(())
    }

    /// Carry out one focus hand-over the core decided: the previous pane
    /// blurs, the active publication moves, the new pane focuses.
    fn hand_over(&self, shared: &Shared, change: FocusChange) {
        if change.is_noop() {
            return;
        }
        if let Some(previous) = change.blur.and_then(|id| self.pane(id)) {
            previous.blur();
        }
        self.publish(shared);
        if let Some(next) = change.focus.and_then(|id| self.pane(id)) {
            next.focus();
        }
    }

    /// Create a pane for `request`: the core mints its id and records it,
    /// the factory builds its runtime, and the pane enters `Mounting` (the
    /// host's view mounts it and marks it ready). `env` receives the new id
    /// so the host can derive the pane's view of the focus authority.
    ///
    /// Call inside the SESSION's root owner (the session start runs in it;
    /// an in-session command re-enters it): the pane's own owner becomes
    /// that owner's child and the env's derived signals land in its arena,
    /// so both die with the session whatever else happens. The manager
    /// keeps no copy of the owner to build in — a strong one stored in the
    /// host's own arena would be a cycle that only an explicit disposal
    /// breaks, and a session whose unmount alone could not release it.
    pub fn create(
        &self,
        request: PaneRequest,
        launch: Option<LaunchDocument>,
        env: impl FnOnce(PaneId) -> PaneEnv,
    ) -> Result<PaneId, PaneError> {
        if Owner::current().is_none() {
            // Built here, the pane's owner and its env would belong to no
            // scope at all: nothing would ever clean them up.
            return Err(PaneError::Unowned);
        }
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        let (descriptor, change) = shared.borrow_mut().core.create(request)?;
        let id = descriptor.pane_id;
        let factory = shared.borrow().factory.clone();
        let pane = untrack(|| factory(env(id), descriptor, launch));
        shared.borrow_mut().panes.insert(id, pane);
        crate::diagnostics::note_pane_create();
        self.transition(&shared, id, |core| core.begin_mount(id))?;
        self.publish(&shared);
        self.hand_over(&shared, change);
        Ok(id)
    }

    /// The pane's view is built and its effects installed: `Mounting →
    /// Ready`.
    pub fn mark_ready(&self, id: PaneId) -> Result<(), PaneError> {
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        self.transition(&shared, id, |core| core.mark_ready(id))
    }

    /// `Ready → Suspended`: the pane keeps its document session but takes
    /// no new work (the host suspends its panes while the frame is off
    /// screen).
    pub fn suspend(&self, id: PaneId) -> Result<(), PaneError> {
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        self.transition(&shared, id, |core| core.suspend(id))
    }

    /// `Suspended → Ready`.
    pub fn resume(&self, id: PaneId) -> Result<(), PaneError> {
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        self.transition(&shared, id, |core| core.resume(id))
    }

    /// The focus authority: make `id` the active pane.
    pub fn set_active(&self, id: PaneId) -> Result<(), PaneError> {
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        let change = shared.borrow_mut().core.focus(id)?;
        self.hand_over(&shared, change);
        Ok(())
    }

    /// The request a pane holds to become active: it goes to the ONE focus
    /// authority ([`Self::set_active`]), which may refuse it (a pane on its
    /// way out) — the pane never flips focus itself.
    pub fn focus_request(&self, id: PaneId) -> Callback<()> {
        let manager = *self;
        Callback::new(move |_| {
            let _ = manager.set_active(id);
        })
    }

    /// Hand one pane its bounds; the pane hears about it only when they
    /// changed.
    pub fn resize(&self, id: PaneId, bounds: PaneBounds) -> Result<(), PaneError> {
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        let changed = shared.borrow_mut().core.resize(id, bounds)?;
        if !changed {
            return Ok(());
        }
        self.bounds.try_update(|published| {
            published.insert(id, bounds);
        });
        if let Some(pane) = self.pane(id) {
            pane.resize(bounds);
        }
        Ok(())
    }

    /// Hand every live pane its bounds (`bounds_for` answers per pane).
    pub fn resize_all(&self, bounds_for: impl Fn(PaneId) -> PaneBounds) {
        let Some(shared) = self.shared() else {
            return;
        };
        let live = shared.borrow().core.live().to_vec();
        for id in live {
            let _ = self.resize(id, bounds_for(id));
        }
    }

    /// Close one pane inside a live session: `→ Disposing`, out of the
    /// placement, focus handed to its successor (`prefer` when live: the
    /// layout's nearest pane), then disposed NOW (the sync half runs here;
    /// the async tail is spawned and finishes the bookkeeping).
    pub fn close(&self, id: PaneId, prefer: Option<PaneId>) -> Result<(), PaneError> {
        spawn_local(self.close_now(id, prefer)?);
        Ok(())
    }

    /// [`Self::close`]'s synchronous half: everything up to and including
    /// the pane's sync teardown. Returns the tail the caller must drive.
    pub fn close_now(&self, id: PaneId, prefer: Option<PaneId>) -> Result<PaneTeardown, PaneError> {
        let shared = self.shared().ok_or(PaneError::HostDisposed)?;
        let change = shared.borrow_mut().core.begin_close(id, prefer)?;
        let tail = self.dispose_one(&shared, id);
        self.publish(&shared);
        self.hand_over(&shared, change);
        Ok(tail)
    }

    /// A pane's lifecycle as the core holds it (tombstones included).
    pub fn lifecycle(&self, id: PaneId) -> Option<PaneLifecycle> {
        self.shared()?.borrow().core.lifecycle(id)
    }

    /// Dispose the whole workspace: every live pane runs its dispose now
    /// (sync half), and their tails wait in [`Self::take_teardown`] for the
    /// session's runtime. Idempotent.
    pub fn dispose_all(&self) {
        let Some(shared) = self.shared() else {
            return;
        };
        let ids = shared.borrow_mut().core.dispose_all();
        for id in ids {
            let tail = self.dispose_one(&shared, id);
            shared.borrow_mut().teardowns.push(tail);
        }
        self.publish(&shared);
    }

    /// One pane's dispose, after the core moved it to `Disposing`: out of
    /// the live map, the lifecycle mirrored, the pane's sync teardown run —
    /// and the pane object released right there. Returns the tail that
    /// awaits the pane's async teardown and then records `Disposed` in the
    /// core — through the `Rc`, never the arena, because the session's
    /// reactive scope may be gone by then.
    fn dispose_one(&self, shared: &Shared, id: PaneId) -> PaneTeardown {
        let pane = shared.borrow_mut().panes.remove(&id);
        let shared = Rc::clone(shared);
        let Some(pane) = pane else {
            // Created and disposed without a runtime (a factory that never
            // returned): nothing to tear down but the record.
            let _ = shared.borrow_mut().core.finish_dispose(id);
            return Box::pin(async {});
        };
        pane.lifecycle_changed(PaneLifecycle::Disposing);
        let tail = pane.dispose();
        // `dispose` is the last call the manager makes on a pane: the map
        // entry is gone, and this was the manager's one remaining reference.
        // Dropping it NOW frees the pane object (its owner shell, its
        // context map) at the sync teardown, instead of pinning it for as
        // long as the engine takes to destroy. The tail owns what it still
        // has to release, and marks the pane's own gates `Disposed`.
        drop(pane);
        Box::pin(async move {
            tail.await;
            let _ = shared.borrow_mut().core.finish_dispose(id);
        })
    }

    /// The workspace disposal's tails, in placement order, as one future
    /// (the session's runtime awaits it before it reports `Disposed`).
    pub fn take_teardown(&self) -> PaneTeardown {
        let tails = self
            .shared()
            .map(|shared| std::mem::take(&mut shared.borrow_mut().teardowns))
            .unwrap_or_default();
        Box::pin(async move {
            for tail in tails {
                tail.await;
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::task::{Context, Poll, Waker};

    use leptos::prelude::*;
    use runtime_contract::boundary::LaunchDocument;

    use super::PaneManager;
    use crate::host::contract::{
        ChromeSlot, PaneAppearance, PaneCommand, PaneDocStatus, PaneEnv, PaneFactory,
        PaneResourceCounts, PaneRuntime, PaneSite, PaneSurface, PaneTeardown,
    };
    use crate::host::model::{
        DocumentId, DocumentRef, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId,
        PaneLifecycle, PaneRequest,
    };

    type Log = Rc<RefCell<Vec<String>>>;
    type Launch = Option<LaunchDocument>;
    type Built = Rc<RefCell<Vec<Rc<FakePane>>>>;

    /// A pane runtime that records every call the manager makes, and owns a
    /// reactive scope of its own (a child of the host's, as the production
    /// pane's is) whose cleanup it reports.
    struct FakePane {
        id: PaneId,
        document: Option<DocumentId>,
        log: Log,
        owner: Owner,
        disposes: Cell<u32>,
    }

    impl FakePane {
        fn note(&self, what: &str) {
            self.log.borrow_mut().push(format!("{what} {}", self.id));
        }
    }

    impl PaneRuntime for FakePane {
        fn id(&self) -> PaneId {
            self.id
        }
        fn format(&self) -> PaneFormat {
            PaneFormat::Pdf
        }
        fn document(&self) -> Option<DocumentId> {
            self.document.clone()
        }
        fn lifecycle_changed(&self, lifecycle: PaneLifecycle) {
            self.note(&format!("lifecycle:{lifecycle:?}"));
        }
        fn surface(&self) -> PaneSurface {
            PaneSurface {
                status: Signal::stored(PaneDocStatus::Ready),
                error: Signal::stored(None),
                page: Signal::stored(1),
                reflowable: Signal::stored(false),
                search_visible: Signal::stored(false),
            }
        }
        fn mount(&self, _bounds: PaneBounds, _site: PaneSite) -> AnyView {
            ().into_any()
        }
        fn chrome(&self, _slot: ChromeSlot, _site: PaneSite) -> Option<AnyView> {
            None
        }
        fn resize(&self, bounds: PaneBounds) {
            self.note(&format!("resize:{}x{}", bounds.width, bounds.height));
        }
        fn focus(&self) {
            self.note("focus");
        }
        fn blur(&self) {
            self.note("blur");
        }
        fn appearance(&self, _appearance: PaneAppearance) {}
        fn command(&self, _command: PaneCommand) -> Result<(), PaneError> {
            Ok(())
        }
        fn resources(&self) -> PaneResourceCounts {
            PaneResourceCounts::default()
        }
        fn dispose(&self) -> PaneTeardown {
            self.disposes.set(self.disposes.get() + 1);
            self.note("dispose");
            self.owner.cleanup();
            let log = Rc::clone(&self.log);
            let id = self.id;
            Box::pin(async move { log.borrow_mut().push(format!("tail {id}")) })
        }
    }

    /// Drive a teardown to completion (every fake tail is ready at once).
    fn drive(mut tail: PaneTeardown) {
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(tail.as_mut().poll(&mut cx), Poll::Ready(()));
    }

    fn fixture() -> (Owner, PaneManager, Log, Built) {
        let owner = Owner::new();
        let log: Log = Rc::default();
        let built: Rc<RefCell<Vec<Rc<FakePane>>>> = Rc::default();
        let build = {
            let log = Rc::clone(&log);
            let built = Rc::clone(&built);
            move |descriptor: PaneDescriptor| -> Rc<dyn PaneRuntime> {
                let pane_owner = Owner::new();
                let cleaned = Rc::clone(&log);
                let id = descriptor.pane_id;
                pane_owner.with(|| {
                    // A reactive resource the pane owns: its cleanup must
                    // run when the pane is disposed, or with the host.
                    let marker = StoredValue::new_local(Some(cleaned));
                    on_cleanup(move || {
                        if let Some(Some(log)) = marker.try_get_value() {
                            log.borrow_mut().push(format!("owner-cleanup {id}"));
                        }
                    });
                });
                let pane = Rc::new(FakePane {
                    id,
                    document: descriptor.document.as_ref().map(|d| d.document_id.clone()),
                    log: Rc::clone(&log),
                    owner: pane_owner,
                    disposes: Cell::new(0),
                });
                built.borrow_mut().push(Rc::clone(&pane));
                pane
            }
        };
        let factory: PaneFactory =
            Rc::new(move |_: PaneEnv, d: PaneDescriptor, _: Launch| build(d));
        let manager = owner.with(|| PaneManager::new(factory));
        (owner, manager, log, built)
    }

    fn env(id: PaneId) -> PaneEnv {
        let settings = RwSignal::new(reader_core::settings::Settings::default());
        let ui = app_state::UiState {
            sidebar: RwSignal::new(app_state::SidebarMode::None),
            toast: RwSignal::new(None),
            window_maximized: RwSignal::new(false),
        };
        PaneEnv {
            runtime: crate::runtime::ReaderRuntime::new(),
            settings,
            ui,
            api: crate::context::ApiHandle::Standalone,
            session_id: 1,
            chrome: app_state::ChromeState {
                settings,
                ui,
                reader: app_state::ReaderSurface {
                    reflowable: Signal::stored(false),
                    search_visible: Signal::stored(false),
                    sidebar_slide: RwSignal::new(app_state::Motion::default()),
                },
            },
            active: Signal::stored(id.get() == 1),
            settings_open: RwSignal::new(false),
            request_focus: Callback::new(|_| {}),
            open: Callback::new(|_| {}),
            can_split: Signal::stored(false),
        }
    }

    fn request(path: &str, focus: bool) -> PaneRequest {
        PaneRequest {
            document: DocumentId::from_launch(None, path).map(|document_id| DocumentRef {
                document_id,
                path: path.to_string(),
            }),
            request_focus: focus,
            ..PaneRequest::default()
        }
    }

    fn count(log: &Log, entry: &str) -> usize {
        log.borrow()
            .iter()
            .filter(|line| line.as_str() == entry)
            .count()
    }

    #[test]
    fn create_mounts_and_publishes_one_active_pane() {
        let (owner, manager, log, _) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", false), None, env).unwrap();
            let b = manager.create(request("/a.pdf", false), None, env).unwrap();
            // Two panes on the SAME document are two panes: the id is the
            // pane's, never the document's.
            assert_ne!(a, b);
            assert_eq!(manager.active_untracked(), Some(a));
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Mounting));
            manager.mark_ready(a).unwrap();
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Ready));
            assert_eq!(count(&log, &format!("focus {a}")), 1);
            assert_eq!(count(&log, &format!("focus {b}")), 0);
            let documents: Vec<_> = manager
                .live_panes()
                .iter()
                .map(|pane| pane.document().unwrap())
                .collect();
            assert_eq!(documents[0], documents[1]);
        });
    }

    #[test]
    fn focus_blurs_the_previous_pane_then_focuses_the_next() {
        let (owner, manager, log, _) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            let b = manager.create(request("/b.pdf", false), None, env).unwrap();
            log.borrow_mut().clear();
            manager.set_active(b).unwrap();
            assert_eq!(
                *log.borrow(),
                vec![format!("blur {a}"), format!("focus {b}")]
            );
            assert_eq!(manager.active_untracked(), Some(b));
            // Asking for the active pane again changes nothing.
            log.borrow_mut().clear();
            manager.set_active(b).unwrap();
            assert!(log.borrow().is_empty());
        });
    }

    #[test]
    fn close_disposes_once_and_hands_focus_on() {
        let (owner, manager, log, built) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            let b = manager.create(request("/b.pdf", false), None, env).unwrap();
            let tail = manager.close_now(a, None).unwrap();
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Disposing));
            assert_eq!(count(&log, &format!("owner-cleanup {a}")), 1);
            assert_eq!(manager.active_untracked(), Some(b));
            assert_eq!(manager.placed.get_untracked(), vec![b]);
            drive(tail);
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Disposed));
            assert_eq!(count(&log, &format!("tail {a}")), 1);
            assert_eq!(built.borrow()[0].disposes.get(), 1);
            // A disposed pane takes no further operations and is never
            // revived.
            assert_eq!(manager.set_active(a), Err(PaneError::Gone(a)));
            assert!(matches!(
                manager.close_now(a, None),
                Err(PaneError::Gone(_))
            ));
            assert_eq!(manager.mark_ready(a), Err(PaneError::Gone(a)));
            assert!(manager.pane(a).is_none());
            assert_eq!(built.borrow()[0].disposes.get(), 1);
        });
    }

    #[test]
    fn dispose_all_cascades_to_every_pane_exactly_once() {
        let (owner, manager, log, built) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            let b = manager.create(request("/b.pdf", false), None, env).unwrap();
            manager.dispose_all();
            assert_eq!(manager.active_untracked(), None);
            assert!(manager.placed.get_untracked().is_empty());
            for id in [a, b] {
                assert_eq!(manager.lifecycle(id), Some(PaneLifecycle::Disposing));
                assert_eq!(count(&log, &format!("owner-cleanup {id}")), 1);
            }
            drive(manager.take_teardown());
            for id in [a, b] {
                assert_eq!(manager.lifecycle(id), Some(PaneLifecycle::Disposed));
            }
            // Idempotent, and the host creates nothing any more.
            manager.dispose_all();
            drive(manager.take_teardown());
            assert!(built.borrow().iter().all(|pane| pane.disposes.get() == 1));
            assert!(matches!(
                manager.create(request("/c.pdf", true), None, env),
                Err(PaneError::HostDisposed)
            ));
        });
    }

    #[test]
    fn the_host_owner_reaches_every_pane_scope() {
        // Host disposal cascades even without the explicit path: a pane's
        // reactive scope is the host's child.
        let (owner, manager, log, _) = fixture();
        let a = owner.with(|| manager.create(request("/a.pdf", true), None, env).unwrap());
        owner.cleanup();
        assert_eq!(count(&log, &format!("owner-cleanup {a}")), 1);
    }

    #[test]
    fn dropping_the_session_owner_alone_releases_the_whole_workspace() {
        // The unmount handle is the session owner's only strong holder: no
        // explicit disposal ran, yet dropping it cleans every pane's scope
        // and frees the host's arena (the manager's state with it). A
        // manager that kept its own copy of the owner would pin all of it.
        let (owner, manager, log, _) = fixture();
        let a = owner.with(|| manager.create(request("/a.pdf", true), None, env).unwrap());
        assert!(manager.shared().is_some());
        drop(owner);
        assert_eq!(count(&log, &format!("owner-cleanup {a}")), 1);
        assert!(
            manager.shared().is_none(),
            "the host's arena outlived its owner"
        );
    }

    #[test]
    fn the_manager_holds_no_pane_past_its_dispose() {
        // Only the test's own record still holds the pane once its dispose
        // returned — the tail owns what it has to release, not the pane.
        let (owner, manager, _, built) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            assert_eq!(Rc::strong_count(&built.borrow()[0]), 2);
            let tail = manager.close_now(a, None).unwrap();
            assert_eq!(Rc::strong_count(&built.borrow()[0]), 1);
            drive(tail);
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Disposed));
        });
    }

    #[test]
    fn a_pane_is_never_built_outside_an_owner() {
        // Outside every reactive owner the pane's scope and its env would be
        // orphans nothing cleans up: refused before an id is minted.
        let (_owner, manager, _, built) = fixture();
        assert_eq!(
            manager.create(request("/a.pdf", true), None, env),
            Err(PaneError::Unowned)
        );
        assert!(built.borrow().is_empty());
        let minted = manager
            .shared()
            .map(|shared| shared.borrow().core.created());
        assert_eq!(minted, Some(0));
    }

    #[test]
    fn resize_reaches_a_pane_only_when_its_bounds_change() {
        let (owner, manager, log, _) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            manager.resize_all(|_| PaneBounds::filling(800.0, 600.0));
            manager.resize_all(|_| PaneBounds::filling(800.0, 600.0));
            assert_eq!(count(&log, &format!("resize:800x600 {a}")), 1);
        });
    }

    #[test]
    fn a_focus_request_goes_through_the_one_authority() {
        let (owner, manager, log, _) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            let b = manager.create(request("/b.pdf", false), None, env).unwrap();
            log.borrow_mut().clear();
            // A pointer landing in pane b: the request blurs a, then b
            // focuses — the same hand-over the host's own calls make.
            manager.focus_request(b).run(());
            assert_eq!(
                *log.borrow(),
                vec![format!("blur {a}"), format!("focus {b}")]
            );
            assert_eq!(manager.active_untracked(), Some(b));
            // A pane on its way out asks in vain: the authority refuses and
            // nothing moves.
            let request_a = manager.focus_request(a);
            drive(manager.close_now(a, None).unwrap());
            log.borrow_mut().clear();
            request_a.run(());
            assert_eq!(manager.active_untracked(), Some(b));
            assert!(log.borrow().is_empty());
        });
    }

    #[test]
    fn suspend_and_resume_are_mirrored_into_the_pane() {
        let (owner, manager, log, _) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            // A pane still mounting has nothing to suspend yet.
            assert!(matches!(manager.suspend(a), Err(PaneError::Illegal { .. })));
            manager.mark_ready(a).unwrap();
            manager.suspend(a).unwrap();
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Suspended));
            assert_eq!(count(&log, &format!("lifecycle:Suspended {a}")), 1);
            manager.resume(a).unwrap();
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Ready));
            assert_eq!(count(&log, &format!("lifecycle:Ready {a}")), 2);
            // A suspended pane is still live: the workspace disposal takes
            // it like any other.
            manager.suspend(a).unwrap();
            manager.dispose_all();
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Disposing));
            drive(manager.take_teardown());
            assert_eq!(manager.lifecycle(a), Some(PaneLifecycle::Disposed));
        });
    }

    #[test]
    fn bounds_are_published_per_live_pane_and_pruned_with_it() {
        let (owner, manager, _, _) = fixture();
        owner.with(|| {
            let a = manager.create(request("/a.pdf", true), None, env).unwrap();
            let b = manager.create(request("/b.pdf", false), None, env).unwrap();
            assert_eq!(manager.bounds_of(a), None);
            let left = PaneBounds {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 600.0,
            };
            let right = PaneBounds { x: 400.0, ..left };
            manager.resize(a, left).unwrap();
            manager.resize(b, right).unwrap();
            assert_eq!(manager.bounds_of(a), Some(left));
            assert_eq!(manager.bounds_of(b), Some(right));
            drive(manager.close_now(a, None).unwrap());
            assert_eq!(manager.bounds_of(a), None);
            assert_eq!(manager.bounds_of(b), Some(right));
        });
    }
}
