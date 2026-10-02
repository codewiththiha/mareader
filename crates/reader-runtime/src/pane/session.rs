//! The pane's document session: the one object that owns the document the
//! pane shows, whatever its format.
//!
//! ```text
//! PaneCell.session: FormatSession
//!   ├── Pdf(PdfSession)        crates/pdf-engine — engine document, worker,
//!   │                          page registry, lanes, caches, paper, search
//!   ├── Markdown(MdSession)    the pane's reflow document lifetime
//!   └── Text(TxtSession)       the pane's reflow document lifetime
//! ```
//!
//! No format shares mutable state with another, and no session is shared
//! between panes: each pane installs a FRESH session per opened document
//! ([`crate::pane::handle::PaneHandle::install_session`]), and the one it
//! replaces is disposed on the spot. A disposed session refuses every call,
//! so work captured against it can never reach the document that replaced
//! it.
//!
//! The reflowable sessions are light on purpose: their content (blocks,
//! headings, heights, cuts, geometry, the stream handle) is stored in the
//! pane's own `ReaderState` — already per pane — and the session owns its
//! LIFETIME: the identity async work checks, the liveness that turns late
//! results away, and the dispose that releases the content. Markdown and
//! TXT share that code (`ReflowLifetime`), never an instance.

use std::cell::Cell;
use std::rc::Rc;

use pdf_engine::PdfSession;

use crate::state::document::reflow::ReflowContent;

/// The realm's reflow session ids: minted once, never reused.
static NEXT_REFLOW_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The lifetime half both reflowable formats share (code, not state: each
/// session has its own).
struct ReflowLifetime {
    id: u64,
    path: String,
    live: Cell<bool>,
    /// The pane's reflow content this document fills — released on dispose.
    reflow: ReflowContent,
}

impl ReflowLifetime {
    fn new(path: &str, reflow: ReflowContent) -> Rc<Self> {
        Rc::new(Self {
            id: NEXT_REFLOW_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1,
            path: path.to_string(),
            live: Cell::new(true),
            reflow,
        })
    }

    /// Stop accepting (late stream/measure results see `is_live() ==
    /// false`), then release the document's content. Idempotent.
    fn dispose(&self) {
        if self.live.replace(false) {
            self.reflow.reset();
        }
    }
}

macro_rules! reflow_session {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone)]
        pub struct $name(Rc<ReflowLifetime>);

        impl $name {
            pub(crate) fn new(path: &str, reflow: ReflowContent) -> Self {
                Self(ReflowLifetime::new(path, reflow))
            }

            /// The session's identity: unique for the realm's life.
            pub fn id(&self) -> u64 {
                self.0.id
            }

            pub fn path(&self) -> &str {
                &self.0.path
            }

            pub fn is_live(&self) -> bool {
                self.0.live.get()
            }

            /// Whether `other` is this very session.
            pub fn same(&self, other: &Self) -> bool {
                Rc::ptr_eq(&self.0, &other.0)
            }

            pub(crate) fn dispose(&self) {
                self.0.dispose();
            }
        }
    };
}

reflow_session! {
    /// A Markdown document open in a pane.
    MdSession
}

reflow_session! {
    /// A plain-text document open in a pane.
    TxtSession
}

/// What a pane's document is owned by. `None` until the first open.
#[derive(Clone, Default)]
pub(crate) enum FormatSession {
    #[default]
    None,
    Pdf(PdfSession),
    Markdown(MdSession),
    Text(TxtSession),
}

impl FormatSession {
    pub(crate) fn is_live(&self) -> bool {
        match self {
            Self::None => false,
            Self::Pdf(s) => s.is_live(),
            Self::Markdown(s) => s.is_live(),
            Self::Text(s) => s.is_live(),
        }
    }

    pub(crate) fn pdf(&self) -> Option<&PdfSession> {
        match self {
            Self::Pdf(s) => Some(s),
            _ => None,
        }
    }

    /// The live reflowable session's id, if this is one.
    pub(crate) fn reflow_id(&self) -> Option<u64> {
        match self {
            Self::Markdown(s) if s.is_live() => Some(s.id()),
            Self::Text(s) if s.is_live() => Some(s.id()),
            _ => None,
        }
    }

    /// Whether async work started for the reflow session `id` may still
    /// commit into this pane.
    pub(crate) fn admits_reflow(&self, id: u64) -> bool {
        match self {
            Self::Markdown(s) => s.is_live() && s.id() == id,
            Self::Text(s) => s.is_live() && s.id() == id,
            _ => false,
        }
    }

    /// Stop the session's in-flight work without ending it — the pane is
    /// about to leave, and its dispose follows over the boundary. A PDF's
    /// page renders are cancelled and its speculative thumbnail prefetches
    /// stand down; a reflowable session has no background
    /// work of its own to stop (its measurement flushes and open tails are
    /// refused once it ends).
    pub(crate) fn quiesce(&self) {
        if let Self::Pdf(s) = self {
            s.cancel_page_renders();
            s.suspend_prefetches();
        }
    }

    /// Dispose the session, whatever its format: from THIS call it refuses
    /// every operation (a reflowable session has already released the
    /// pane's content; a PDF session has stopped accepting and advanced its
    /// invalidation). What is still in flight — a PDF's engine teardown:
    /// document, worker, rasters, caches — is the returned [`Retiring`].
    pub(crate) fn dispose(self) -> Retiring {
        match self {
            Self::None => Retiring::settled_now(),
            Self::Markdown(s) => {
                s.dispose();
                Retiring::settled_now()
            }
            Self::Text(s) => {
                s.dispose();
                Retiring::settled_now()
            }
            Self::Pdf(s) => Retiring::pending(Box::pin(s.dispose())),
        }
    }
}

/// A document session's release, still in flight after its dispose.
///
/// One type for every format and every way a document ends — replaced by
/// the next open in the same pane, abandoned by a failed open, or ended by
/// the pane's dispose — so the policy of WHEN the memory must be back is
/// the caller's, never a per-format special case:
///
/// - an open in the SAME pane awaits it before loading
///   ([`crate::pane::handle::PaneHandle::replace_document`]): a pane never
///   holds two documents at once;
/// - the pane's dispose awaits it in its tail;
/// - anything that no longer waits on it detaches it.
///
/// It is scoped to the one session it came from: awaiting it never waits on
/// another pane's document, so panes open, replace and close concurrently
/// (split mode opens several documents at once, and none of them is killed
/// or delayed by another's open).
#[must_use = "await the release, or detach it; dropping it defers the engine teardown to the drop net"]
pub(crate) struct Retiring(Option<std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>>);

impl Retiring {
    fn settled_now() -> Self {
        Self(None)
    }

    fn pending(release: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>) -> Self {
        Self(Some(release))
    }

    /// Wait until the session's resources are released.
    pub(crate) async fn settled(self) {
        if let Some(release) = self.0 {
            release.await;
        }
    }

    /// Let the release finish on its own: nothing waits for it.
    pub(crate) fn detach(self) {
        if let Some(release) = self.0 {
            wasm_bindgen_futures::spawn_local(release);
        }
    }

    /// Whether anything is still in flight (tests only: the callers never
    /// branch on it — they await or detach).
    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.0.is_some()
    }

    /// [`Self::settled`] for the host tests, which have no executor.
    #[cfg(test)]
    pub(crate) fn settle_for_tests(self) {
        tests::block_on(self.settled());
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn reflow() -> ReflowContent {
        ReflowContent::default()
    }

    #[test]
    fn reflow_sessions_are_distinct_and_die_once() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let md = MdSession::new("/a.md", reflow());
            let txt = TxtSession::new("/b.txt", reflow());
            assert_ne!(md.id(), txt.id());
            assert!(md.is_live() && txt.is_live());
            let pane_a = FormatSession::Markdown(md.clone());
            let pane_b = FormatSession::Text(txt.clone());
            assert!(pane_a.admits_reflow(md.id()));
            assert!(!pane_a.admits_reflow(txt.id()));
            assert!(!pane_a.clone().dispose().is_pending());
            assert!(!md.is_live());
            assert!(!pane_a.admits_reflow(md.id()));
            // Disposing one pane's session leaves the other's alive.
            assert!(txt.is_live());
            assert!(pane_b.admits_reflow(txt.id()));
            md.dispose(); // idempotent
        });
    }

    #[test]
    fn a_pdf_pane_disposes_while_a_text_pane_lives() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let pdf = PdfSession::create();
            let txt = TxtSession::new("/b.txt", reflow());
            let pdf_pane = FormatSession::Pdf(pdf.clone());
            let text_pane = FormatSession::Text(txt.clone());
            let release = pdf_pane.dispose();
            // Refused from the dispose call, before the release is awaited.
            assert!(!pdf.is_live());
            assert!(release.is_pending());
            release.settle_for_tests();
            assert!(!pdf.is_live());
            assert!(text_pane.is_live());
        });
    }

    /// The host tests' executor: nothing here pends (there is no engine on
    /// the host), so one poll with a no-op waker finishes every future.
    pub(crate) fn block_on<F: std::future::Future>(f: F) -> F::Output {
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(out) => out,
            std::task::Poll::Pending => panic!("host futures must not pend"),
        }
    }
}
