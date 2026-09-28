//! The pane's own handle onto what it owns, carried in the pane's
//! [`crate::context::ReaderContext`]: its identity, its lifecycle gate and
//! its resource registry.
//!
//! The registry lives in the HOST's arena, not the pane's: a pane's reactive
//! owner is cleaned up in the middle of its own disposal, and the registry
//! must still answer the teardown after that (the virtualizers it hands to
//! the tail were registered from inside the owner being cleaned). The
//! pane's disposal tail releases the slot the moment its teardown finished
//! ([`PaneHandle::release`]) — a pane closed inside a live session costs the
//! host's arena nothing afterwards — and the host's owner sweeps whatever a
//! tail never reached.

use leptos::prelude::*;

use crate::host::model::{PaneId, PaneLifecycle};
use crate::runtime::ReaderRuntime;

/// Everything a pane owns that must die with it and is not a reactive
/// node: the virtualizers (disposed BY the pane's dispose — the component
/// cleanups are the inner safety net), and the engine document session the
/// pane opened.
#[derive(Default)]
pub(crate) struct PaneResources {
    virtualizers: Vec<virtual_list_leptos::Virtualizer>,
    /// The pane holds the engine's document session (a PDF it opened).
    document_session: bool,
}

impl PaneResources {
    fn track(&mut self, v: &virtual_list_leptos::Virtualizer) {
        if !self.virtualizers.iter().any(|known| known == v) {
            self.virtualizers.push(v.clone());
        }
    }

    fn untrack(&mut self, v: &virtual_list_leptos::Virtualizer) {
        if let Some(at) = self.virtualizers.iter().position(|known| known == v) {
            self.virtualizers.remove(at);
        }
    }
}

/// The pane's slot in the host arena: the lifecycle the manager last
/// published for it, and its resources.
#[derive(Default)]
pub(crate) struct PaneCell {
    /// Written ONLY by the manager, right after the core's transition — a
    /// publication of the core's answer, not a second authority.
    lifecycle: PaneLifecycle,
    resources: PaneResources,
}

/// The pane's Copy handle. Every access is `try_`: a handle captured by a
/// straggling callback can outlive the host's arena, and a dead slot must
/// answer "nothing here", never abort the artifact.
#[derive(Clone, Copy)]
pub struct PaneHandle {
    id: PaneId,
    runtime: ReaderRuntime,
    cell: StoredValue<PaneCell, LocalStorage>,
}

impl PaneHandle {
    /// A new slot, allocated in the CURRENT owner — the host calls this
    /// inside its own owner, so the slot outlives the pane's.
    pub(crate) fn new(id: PaneId, runtime: ReaderRuntime) -> Self {
        Self {
            id,
            runtime,
            cell: StoredValue::new_local(PaneCell::default()),
        }
    }

    pub fn id(&self) -> PaneId {
        self.id
    }

    /// The manager's publication of a transition.
    pub(crate) fn publish_lifecycle(&self, lifecycle: PaneLifecycle) {
        let _ = self
            .cell
            .try_update_value(|cell| cell.lifecycle = lifecycle);
    }

    pub fn lifecycle(&self) -> PaneLifecycle {
        self.cell
            .try_with_value(|cell| cell.lifecycle)
            .unwrap_or(PaneLifecycle::Disposed)
    }

    /// Whether pane work may start: the pane admits it AND the session
    /// runtime does.
    pub fn admits_work(&self) -> bool {
        self.lifecycle().admits_work() && self.runtime.lifecycle().admits_work()
    }

    /// The guarded engine-session handle. Capture it FRESH at each use — the
    /// guards snapshot the pane's (and the session's) state at creation.
    pub fn pdf(&self) -> crate::pane::engine::PdfSessionHandle {
        let pane = self.lifecycle();
        let session = self.runtime.lifecycle();
        crate::pane::engine::PdfSessionHandle::new(
            pane.admits_work() && session.admits_work(),
            pane.admits_teardown() && session.admits_teardown(),
        )
    }

    /// Register a virtualizer this pane created.
    pub fn track_virtualizer(&self, v: &virtual_list_leptos::Virtualizer) {
        let _ = self.cell.try_update_value(|cell| cell.resources.track(v));
    }

    /// The registering owner's cleanup dropped its virtualizer.
    pub fn untrack_virtualizer(&self, v: &virtual_list_leptos::Virtualizer) {
        let _ = self.cell.try_update_value(|cell| cell.resources.untrack(v));
    }

    /// The open flow's engine open landed (or a reflow open released it).
    pub(crate) fn note_document_session(&self, held: bool) {
        let _ = self
            .cell
            .try_update_value(|cell| cell.resources.document_session = held);
    }

    pub fn virtualizer_count(&self) -> usize {
        self.cell
            .try_with_value(|cell| cell.resources.virtualizers.len())
            .unwrap_or(0)
    }

    pub fn holds_document_session(&self) -> bool {
        self.cell
            .try_with_value(|cell| cell.resources.document_session)
            .unwrap_or(false)
    }

    /// The pane's last word: its teardown finished, so its slot leaves the
    /// host's arena. From here every gate reads the released slot as
    /// `Disposed` (the `try_` reads' fallback), which is exactly the
    /// lifecycle a pane with nothing left owns — no separate write needed,
    /// and a stale task still holding the handle is refused, not revived.
    /// Releasing a slot the host's owner already swept is a no-op.
    pub(crate) fn release(&self) {
        self.cell.dispose();
    }

    /// Hand every registered virtualizer to the disposal tail: the registry
    /// is out of the picture from this moment.
    pub(crate) fn take_virtualizers(&self) -> Vec<virtual_list_leptos::Virtualizer> {
        self.cell
            .try_update_value(|cell| std::mem::take(&mut cell.resources.virtualizers))
            .unwrap_or_default()
    }
}
