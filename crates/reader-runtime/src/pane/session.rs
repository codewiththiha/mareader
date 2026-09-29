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

    /// Whether async work started for the reflow session `id` may still
    /// commit into this pane.
    /// The live reflowable session's id, if this is one.
    pub(crate) fn reflow_id(&self) -> Option<u64> {
        match self {
            Self::Markdown(s) if s.is_live() => Some(s.id()),
            Self::Text(s) if s.is_live() => Some(s.id()),
            _ => None,
        }
    }

    pub(crate) fn admits_reflow(&self, id: u64) -> bool {
        match self {
            Self::Markdown(s) => s.is_live() && s.id() == id,
            Self::Text(s) => s.is_live() && s.id() == id,
            _ => false,
        }
    }

    /// Dispose the session. The reflowable half is synchronous (it only
    /// stops accepting and releases pane content); the PDF half awaits the
    /// engine's teardown, so it is returned as a future for the caller to
    /// await (the pane's disposal tail, a PDF open replacing it) or
    /// detach (a session a text open or a failed open replaced).
    pub(crate) fn dispose(self) -> Option<impl std::future::Future<Output = ()>> {
        match self {
            Self::None => None,
            Self::Markdown(s) => {
                s.dispose();
                None
            }
            Self::Text(s) => {
                s.dispose();
                None
            }
            Self::Pdf(s) => Some(async move { s.dispose().await }),
        }
    }

    /// Dispose without waiting: a replaced session's engine teardown runs
    /// detached (it no longer owns anything the pane shows, and every call
    /// it could receive is already refused).
    pub(crate) fn dispose_detached(self) {
        if let Some(teardown) = self.dispose() {
            wasm_bindgen_futures::spawn_local(teardown);
        }
    }

    /// [`Self::dispose_detached`] for the host tests, which have no
    /// `spawn_local`: the teardown is driven to completion in place.
    #[cfg(test)]
    pub(crate) fn dispose_detached_for_tests(self) {
        if let Some(teardown) = self.dispose() {
            tests::block_on(teardown);
        }
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
            assert!(pane_a.clone().dispose().is_none());
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
            if let Some(teardown) = pdf_pane.dispose() {
                block_on(teardown);
            }
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
