//! The pane's own handle onto what it owns, carried in the pane's
//! [`crate::context::ReaderContext`]: its identity, its lifecycle gate, its
//! document session and its resource registry.
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
use crate::pane::engine::PdfPane;
use crate::pane::session::FormatSession;
use crate::runtime::ReaderRuntime;

/// Everything a pane owns that must die with it and is not a reactive
/// node or its document session: the virtualizers (disposed BY the pane's
/// dispose — the component cleanups are the inner safety net).
#[derive(Default)]
pub(crate) struct PaneResources {
    virtualizers: Vec<virtual_list_leptos::Virtualizer>,
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
/// published for it, its document session, and its resources.
#[derive(Default)]
pub(crate) struct PaneCell {
    /// Written ONLY by the manager, right after the core's transition — a
    /// publication of the core's answer, not a second authority.
    lifecycle: PaneLifecycle,
    /// The ONE owner of the document this pane shows (`crate::pane::session`).
    /// Replaced — and the old one disposed — by every open; taken by the
    /// pane's dispose.
    session: FormatSession,
    /// The pane's document generation: claimed by every open and by the
    /// dispose, so an open's async tail commits only while it is still the
    /// pane's latest document attempt. Per pane — another pane's open never
    /// stales this one's.
    generation: u64,
    resources: PaneResources,
    /// The zoom the pane's descriptor asked for, until the first document's
    /// startup scale consumes it.
    pending_zoom: Option<f64>,
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

    /// The pane's PDF session, guarded. Capture it FRESH at each use — the
    /// guards snapshot the pane's (and the runtime's) lifecycle, and the
    /// session is whichever one the pane holds right now (none for a
    /// reflowable document, or before the first open).
    pub fn pdf(&self) -> PdfPane {
        let pane = self.lifecycle();
        let runtime = self.runtime.lifecycle();
        let session = self
            .cell
            .try_with_value(|cell| cell.session.pdf().cloned())
            .flatten();
        PdfPane::new(
            session,
            pane.admits_work() && runtime.admits_work(),
            pane.admits_teardown() && runtime.admits_teardown(),
        )
    }

    /// The guarded view onto a SPECIFIC session captured earlier — a page
    /// canvas binds to the session it was mounted for. Work is admitted
    /// only while the pane still holds exactly that session (a page left
    /// over from a replaced document can never register into, or render
    /// from, the next one); teardown always reaches it (the engine ignores
    /// a retired sid).
    pub fn pdf_for(&self, session: Option<&pdf_engine::PdfSession>) -> PdfPane {
        let pane = self.lifecycle();
        let runtime = self.runtime.lifecycle();
        let held = session.is_some_and(|s| self.holds_pdf(s));
        PdfPane::new(
            session.cloned(),
            held && pane.admits_work() && runtime.admits_work(),
            pane.admits_teardown() && runtime.admits_teardown(),
        )
    }

    /// Install `session` as the pane's document owner and return the one it
    /// replaces (the caller disposes it — a PDF open awaits the teardown
    /// before loading, a text open detaches it, the pane's dispose awaits
    /// it). Refused (returns `session` back as the
    /// "replaced" one, so it is disposed at once) when the slot is gone.
    pub(crate) fn install_session(&self, session: FormatSession) -> FormatSession {
        let mut incoming = Some(session);
        let replaced = self.cell.try_update_value(|cell| {
            std::mem::replace(&mut cell.session, incoming.take().unwrap_or_default())
        });
        match replaced {
            Some(previous) => previous,
            None => incoming.unwrap_or_default(),
        }
    }

    /// Take the pane's session out of the slot (the dispose). From here no
    /// call through this handle reaches it.
    pub(crate) fn take_session(&self) -> FormatSession {
        self.cell
            .try_update_value(|cell| std::mem::take(&mut cell.session))
            .unwrap_or_default()
    }

    /// The id of the pane's live reflowable session (Markdown or text), if
    /// that is what it holds — the identity a text document's async work
    /// captures and later hands to [`Self::admits_reflow`].
    pub(crate) fn reflow_session(&self) -> Option<u64> {
        self.cell
            .try_with_value(|cell| cell.session.reflow_id())
            .flatten()
    }

    /// The pane's document ends — its dispose. The generation is claimed
    /// first (an open still in flight can no longer land), then the format
    /// session is taken out of the slot and disposed: a Markdown/text
    /// session right here, synchronously; a PDF session's engine teardown
    /// is returned for the caller's tail to await. From this call on,
    /// nothing reaches the session through the pane. Returns the generation
    /// it claimed.
    pub(crate) fn end_document(
        &self,
    ) -> (u64, Option<impl std::future::Future<Output = ()> + use<>>) {
        let generation = self.claim_generation();
        (generation, self.take_session().dispose())
    }

    /// Whether the reflowable session `id` is still the pane's live
    /// document — the commit check for a text document's async tails.
    pub(crate) fn admits_reflow(&self, id: u64) -> bool {
        self.admits_work()
            && self
                .cell
                .try_with_value(|cell| cell.session.admits_reflow(id))
                .unwrap_or(false)
    }

    /// Whether `session` is still the pane's PDF session (and live).
    pub fn holds_pdf(&self, session: &pdf_engine::PdfSession) -> bool {
        session.is_live()
            && self
                .cell
                .try_with_value(|cell| cell.session.pdf().is_some_and(|s| s.same(session)))
                .unwrap_or(false)
    }

    /// Claim the pane's document generation for a new attempt (an open or
    /// the dispose) and return it. Every earlier generation of THIS pane is
    /// stale from here on; other panes are untouched. The value comes from
    /// the realm mint, so it also serves as the diagnostics epoch.
    pub(crate) fn claim_generation(&self) -> u64 {
        let generation = crate::services::document::session::next_generation();
        let _ = self
            .cell
            .try_update_value(|cell| cell.generation = generation);
        generation
    }

    /// Whether `generation` is still this pane's latest document attempt —
    /// false once a later open or the dispose claimed it, or the slot is
    /// gone.
    pub(crate) fn owns_generation(&self, generation: u64) -> bool {
        self.cell
            .try_with_value(|cell| cell.generation == generation)
            .unwrap_or(false)
    }

    /// The pane's current document generation (0 before the first open).
    pub(crate) fn generation(&self) -> u64 {
        self.cell
            .try_with_value(|cell| cell.generation)
            .unwrap_or(0)
    }

    /// Record the zoom the pane was created with (its descriptor's
    /// `initial_zoom`; `None` leaves the fit to the settings).
    pub(crate) fn seed_initial_zoom(&self, zoom: Option<f64>) {
        let _ = self.cell.try_update_value(|cell| cell.pending_zoom = zoom);
    }

    /// The requested zoom, handed out ONCE: the first document that seeds
    /// takes it, every later open fits as the settings say.
    pub(crate) fn take_initial_zoom(&self) -> Option<f64> {
        self.cell
            .try_update_value(|cell| cell.pending_zoom.take())
            .flatten()
    }

    /// Register a virtualizer this pane created.
    pub fn track_virtualizer(&self, v: &virtual_list_leptos::Virtualizer) {
        let _ = self.cell.try_update_value(|cell| cell.resources.track(v));
    }

    /// The registering owner's cleanup dropped its virtualizer.
    pub fn untrack_virtualizer(&self, v: &virtual_list_leptos::Virtualizer) {
        let _ = self.cell.try_update_value(|cell| cell.resources.untrack(v));
    }

    pub fn virtualizer_count(&self) -> usize {
        self.cell
            .try_with_value(|cell| cell.resources.virtualizers.len())
            .unwrap_or(0)
    }

    /// The pane holds a live document session (of any format).
    pub fn holds_document_session(&self) -> bool {
        self.cell
            .try_with_value(|cell| cell.session.is_live())
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

#[cfg(test)]
mod tests {
    use leptos::prelude::*;

    use super::PaneHandle;
    use crate::host::model::{PaneManagerCore, PaneRequest};
    use crate::runtime::ReaderRuntime;

    #[test]
    fn the_requested_zoom_is_handed_out_once() {
        let owner = Owner::new();
        owner.with(|| {
            let (descriptor, _) = PaneManagerCore::new()
                .create(PaneRequest {
                    initial_zoom: Some(1.5),
                    ..PaneRequest::default()
                })
                .unwrap();
            let handle = PaneHandle::new(descriptor.pane_id, ReaderRuntime::new());
            assert_eq!(handle.take_initial_zoom(), None);
            handle.seed_initial_zoom(descriptor.initial_zoom);
            assert_eq!(handle.take_initial_zoom(), Some(1.5));
            // Consumed: the pane's next document fits as the settings say.
            assert_eq!(handle.take_initial_zoom(), None);
        });
    }

    fn two_handles() -> (PaneHandle, PaneHandle) {
        let mut core = PaneManagerCore::new();
        let (a, _) = core.create(PaneRequest::default()).unwrap();
        let (b, _) = core.create(PaneRequest::default()).unwrap();
        let runtime = ReaderRuntime::new();
        (
            PaneHandle::new(a.pane_id, runtime),
            PaneHandle::new(b.pane_id, runtime),
        )
    }

    #[test]
    fn document_generations_are_per_pane() {
        let owner = Owner::new();
        owner.with(|| {
            let (a, b) = two_handles();
            let first = a.claim_generation();
            // Pane B's open must not stale pane A's in-flight one.
            let other = b.claim_generation();
            assert!(a.owns_generation(first));
            assert!(b.owns_generation(other));
            // A's own next attempt does.
            let second = a.claim_generation();
            assert!(!a.owns_generation(first));
            assert!(a.owns_generation(second));
            assert!(second > first, "generations are never reused");
        });
    }

    #[test]
    fn each_pane_holds_its_own_pdf_session() {
        use crate::pane::session::FormatSession;
        use pdf_engine::PdfSession;
        let owner = Owner::new();
        owner.with(|| {
            let (a, b) = two_handles();
            let sa = PdfSession::create();
            let sb = PdfSession::create();
            assert!(matches!(
                a.install_session(FormatSession::Pdf(sa.clone())),
                FormatSession::None
            ));
            assert!(matches!(
                b.install_session(FormatSession::Pdf(sb.clone())),
                FormatSession::None
            ));
            assert!(a.holds_pdf(&sa) && !a.holds_pdf(&sb));
            assert!(b.holds_pdf(&sb) && !b.holds_pdf(&sa));
            assert_eq!(a.pdf().session().map(PdfSession::sid), Some(sa.sid()));
            // Reopen in A: the new session replaces the old, which comes
            // back to be disposed — B never notices.
            let next = PdfSession::create();
            let replaced = a.install_session(FormatSession::Pdf(next.clone()));
            assert!(replaced.pdf().is_some_and(|s| s.same(&sa)));
            replaced.dispose_detached_for_tests();
            assert!(!sa.is_live());
            assert!(
                !a.holds_pdf(&sa),
                "a replaced session is never the pane's again"
            );
            assert!(a.holds_pdf(&next));
            assert!(b.holds_pdf(&sb));
            // The dispose takes the session out of the slot.
            let taken = a.take_session();
            assert!(taken.pdf().is_some_and(|s| s.same(&next)));
            assert!(a.pdf().session().is_none());
            assert!(!a.holds_document_session());
            assert!(b.holds_document_session());
        });
    }

    #[test]
    fn a_released_slot_refuses_a_session() {
        use crate::pane::session::FormatSession;
        use pdf_engine::PdfSession;
        let owner = Owner::new();
        owner.with(|| {
            let (a, _) = two_handles();
            a.release();
            let late = PdfSession::create();
            // The session comes straight back, to be disposed by the caller.
            let back = a.install_session(FormatSession::Pdf(late.clone()));
            assert!(back.pdf().is_some_and(|s| s.same(&late)));
            assert!(a.pdf().session().is_none());
            assert!(!a.owns_generation(a.claim_generation()));
        });
    }

    /// Pane close: closing the PDF pane disposes ITS session only — the text
    /// pane's session stays live and keeps admitting its own work.
    #[test]
    fn closing_the_pdf_pane_leaves_the_text_pane_live() {
        use crate::pane::session::tests::block_on;
        use crate::pane::session::{FormatSession, TxtSession};
        use crate::state::document::reflow::ReflowContent;
        use pdf_engine::PdfSession;
        let owner = Owner::new();
        owner.with(|| {
            let (a, b) = two_handles();
            let pdf = PdfSession::create();
            let txt = TxtSession::new("/b.txt", ReflowContent::default());
            a.install_session(FormatSession::Pdf(pdf.clone()));
            b.install_session(FormatSession::Text(txt.clone()));
            let b_id = b.reflow_session().expect("B holds a text session");
            let (_, teardown) = a.end_document();
            block_on(teardown.expect("a PDF teardown is the tail's to await"));
            assert!(!pdf.is_live());
            assert!(!a.holds_document_session());
            assert!(txt.is_live());
            assert!(b.admits_reflow(b_id));
            assert!(b.holds_document_session());
        });
    }

    /// Reopen: open A, close A, open B in the same pane — every handle
    /// captured for A (a view, a page's bound view, the open's generation)
    /// is refused, and none of it lands on B.
    #[test]
    fn nothing_captured_for_a_reaches_the_next_document() {
        use crate::pane::session::FormatSession;
        use crate::pane::session::tests::block_on;
        use pdf_engine::PdfSession;
        let owner = Owner::new();
        owner.with(|| {
            let (pane, _) = two_handles();
            let open_a = pane.claim_generation();
            let a = PdfSession::create();
            pane.install_session(FormatSession::Pdf(a.clone()));
            let view_a = pane.pdf();
            let page_a = pane.pdf_for(Some(&a));
            // Close A.
            let (_, teardown) = pane.end_document();
            block_on(teardown.expect("PDF teardown"));
            // Open B.
            let open_b = pane.claim_generation();
            let b = PdfSession::create();
            pane.install_session(FormatSession::Pdf(b.clone()));

            assert!(!pane.owns_generation(open_a), "A's open tail stands down");
            assert!(pane.owns_generation(open_b));
            for stale in [&view_a, &page_a] {
                assert!(
                    stale.session().is_some_and(|s| s.same(&a)),
                    "still A, never B"
                );
                assert!(!stale.still_current(&pane));
                assert!(stale.search("anything").is_none());
                let render = block_on(stale.render_page("page-1", 1.0, false));
                assert_eq!(render.map(|_| ()).unwrap_err().name, "no_session");
            }
            // A page bound to A never gets work admitted against B.
            let late = pane.pdf_for(Some(&a));
            assert!(late.search("anything").is_none());
            assert!(b.is_live() && pane.holds_pdf(&b));
            assert!(pane.pdf().session().is_some_and(|s| s.same(&b)));
        });
    }
}
