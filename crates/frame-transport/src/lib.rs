//! The frame transport: the wire half of the frame protocol (§7, §8).
//!
//! `runtime-contract::protocol` owns the VOCABULARY every side serializes;
//! this crate owns how it moves: a cloneable message sink both runtime
//! runtimes call their [`ShellApi`] through ([`PortShellApi`]), the request
//! owned launch futures that pair queries with async port answers, and —
//! behind `wasm32` — the `MessagePort` wire itself
//! ([`wasm::PortWire`]). Hosted route artifacts adopt a nonce/generation
//! authenticated channel offer from their actual same-origin parent; the
//! boot those artifacts share on top of that adoption — the marker, the
//! boundary, the runtime root, the paint report — is [`artifact`].
//!
//! Host tests exercise everything except the DOM: the wire is a trait with a
//! recording double, so the envelope stamping (generation on every message,
//! request ids pairing resolve answers to their questions) is asserted off
//! the browser entirely.

#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

use runtime_contract::ShellApi;
use runtime_contract::boundary::{DocStatusReport, LaunchDocument, ReadPoint};
use runtime_contract::protocol::{RuntimeEnvelope, RuntimeFrame};

pub mod artifact;
pub mod wasm;

/// A claimed hosted boot never falls back to a standalone application.
#[derive(Debug, PartialEq, Eq)]
pub enum BootMarker {
    Standalone,
    Hosted { generation: u64, nonce: String },
    InvalidHosted,
}

pub fn parse_boot_marker(search: &str) -> BootMarker {
    let query = search.strip_prefix('?').unwrap_or(search);
    let mut hosted = false;
    let mut generation = None;
    let mut nonce = None;
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("hosted", "1")) => hosted = true,
            Some(("g", value)) => generation = value.parse::<u64>().ok().filter(|g| *g > 0),
            Some(("n", value)) if !value.is_empty() => nonce = Some(value.to_string()),
            _ => {}
        }
    }
    if !hosted {
        return BootMarker::Standalone;
    }
    match generation.zip(nonce) {
        Some((generation, nonce)) => BootMarker::Hosted { generation, nonce },
        None => BootMarker::InvalidHosted,
    }
}

pub const CHANNEL_KIND: &str = "mareader.channel";

/// A destination a serialized envelope can be posted to. The production
/// implementation is the frame's `MessagePort`; the tests' is a `Vec`.
pub trait Wire: Clone + 'static {
    fn post_json(&self, json: String);
}

/// A recording sink for the host lanes: every posted string, shared with the
/// clone the api holds (a frame's wire is `Clone` so the session's context
/// stays free of lifetimes).
#[cfg(test)]
#[derive(Clone, Default)]
pub struct TestWire {
    pub posted: Rc<RefCell<Vec<String>>>,
}

#[cfg(test)]
impl Wire for TestWire {
    fn post_json(&self, json: String) {
        self.posted.borrow_mut().push(json);
    }
}

#[derive(Default)]
struct ResolveState {
    answer: Option<Option<LaunchDocument>>,
    wake: Option<Waker>,
}

type ResolveMap = RefCell<HashMap<u64, Rc<RefCell<ResolveState>>>>;

/// One registry owns every outstanding launch query. Responses, disposal
/// and dropped futures all remove their entry before waking a continuation.
#[derive(Default)]
struct PendingResolves {
    next: Cell<u64>,
    pending: Rc<ResolveMap>,
}

impl PendingResolves {
    fn issue(&self) -> (u64, ResolveTicket) {
        let id = self
            .next
            .get()
            .checked_add(1)
            .expect("resolve ids exhausted");
        self.next.set(id);
        let state = Rc::new(RefCell::new(ResolveState::default()));
        self.pending.borrow_mut().insert(id, state.clone());
        (
            id,
            ResolveTicket {
                id,
                state,
                registry: Rc::downgrade(&self.pending),
            },
        )
    }

    fn settle(&self, request: u64, document: Option<LaunchDocument>) {
        let state = self.pending.borrow_mut().remove(&request);
        if let Some(state) = state {
            Self::answer(state, document);
        }
    }

    fn cancel_all(&self) {
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        for (_, state) in pending {
            Self::answer(state, None);
        }
    }

    fn answer(state: Rc<RefCell<ResolveState>>, document: Option<LaunchDocument>) {
        let wake = {
            let mut state = state.borrow_mut();
            state.answer = Some(document);
            state.wake.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
    }
}

/// An awaited launch answer, not a synchronous miss. Its drop cancels the
/// outstanding request; a late response cannot retain data for an abandoned
/// continuation. The owning API cancels/wakes all futures during disposal.
pub struct ResolveTicket {
    id: u64,
    state: Rc<RefCell<ResolveState>>,
    registry: Weak<ResolveMap>,
}

impl Future for ResolveTicket {
    type Output = Option<LaunchDocument>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.borrow_mut();
        if let Some(answer) = state.answer.take() {
            return Poll::Ready(answer);
        }
        if state
            .wake
            .as_ref()
            .is_none_or(|wake| !wake.will_wake(cx.waker()))
        {
            state.wake = Some(cx.waker().clone());
        }
        Poll::Pending
    }
}

impl Drop for ResolveTicket {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.borrow_mut().remove(&self.id);
        }
        self.state.borrow_mut().wake = None;
    }
}

/// `ShellApi` over the frame port: every boundary command serialized onto
/// the wire behind its frame's generation (§8 — a message without ITS
/// generation cannot leave this api, so a stale generation is unmakeable
/// here and the Shell's drop log reads it there).
pub struct PortShellApi<W: Wire> {
    wire: W,
    generation: u64,
    resolves: PendingResolves,
}

impl<W: Wire> PortShellApi<W> {
    pub fn new(wire: W, generation: u64) -> Self {
        Self {
            wire,
            generation,
            resolves: PendingResolves::default(),
        }
    }

    /// Post one bound message. The runtime's boot handshake messages (ready /
    /// painted / status / dispose-complete) ride this too — same envelope,
    /// same guard, one serialization path for everything the frame emits.
    pub fn emit(&self, body: RuntimeFrame) {
        let envelope = RuntimeEnvelope {
            generation: self.generation,
            body,
        };
        if let Ok(json) = serde_json::to_string(&envelope) {
            self.wire.post_json(json);
        }
    }

    /// Resolve a path over the port. The future distinguishes pending from
    /// an answered miss; callers must await it before opening the document.
    pub fn resolve_launch(&self, path: &str) -> ResolveTicket {
        let (request, ticket) = self.resolves.issue();
        self.emit(RuntimeFrame::ResolveLaunch {
            request,
            path: path.to_string(),
        });
        ticket
    }

    pub fn settle_launch(&self, request: u64, document: Option<LaunchDocument>) {
        self.resolves.settle(request, document);
    }

    pub fn cancel_resolves(&self) {
        self.resolves.cancel_all();
    }
}

impl<W: Wire> Drop for PortShellApi<W> {
    fn drop(&mut self) {
        self.cancel_resolves();
    }
}

impl<W: Wire> ShellApi for PortShellApi<W> {
    fn open_document(&self, launch: &LaunchDocument) {
        self.emit(RuntimeFrame::OpenDocument {
            launch: Box::new(launch.clone()),
        });
    }
    fn navigate_library(&self) {
        self.emit(RuntimeFrame::NavigateLibrary);
    }
    fn read_point(&self, point: &ReadPoint) {
        self.emit(RuntimeFrame::ReadPoint {
            point: Box::new(point.clone()),
        });
    }
    fn save_settings(&self, settings: &reader_core::settings::Settings) {
        self.emit(RuntimeFrame::SaveSettings {
            settings: Box::new(settings.clone()),
        });
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        self.emit(RuntimeFrame::SaveCover {
            path: path.to_string(),
            image: image.clone(),
        });
    }
    fn save_gloss(&self, key: &str, marks: String) {
        self.emit(RuntimeFrame::SaveGloss {
            key: key.to_string(),
            marks,
        });
    }
    fn bake_cover(&self, path: &str) {
        self.emit(RuntimeFrame::BakeCover {
            path: path.to_string(),
        });
    }
    fn doc_status(&self, report: &DocStatusReport) {
        self.emit(RuntimeFrame::DocStatus {
            report: report.clone(),
        });
    }
    fn publish_digest(&self, json: String) {
        self.emit(RuntimeFrame::PublishDigest { json });
    }
    fn reload(&self) {
        self.emit(RuntimeFrame::Reload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_contract::protocol::BootStage;

    fn api() -> (PortShellApi<TestWire>, TestWire) {
        let wire = TestWire::default();
        (PortShellApi::new(wire.clone(), 17), wire)
    }

    #[test]
    fn a_claimed_hosted_boot_never_becomes_standalone() {
        assert_eq!(parse_boot_marker(""), BootMarker::Standalone);
        assert_eq!(
            parse_boot_marker("?open=/sample.pdf"),
            BootMarker::Standalone
        );
        assert_eq!(
            parse_boot_marker("?hosted=1&g=7&n=nonce"),
            BootMarker::Hosted {
                generation: 7,
                nonce: "nonce".to_string(),
            }
        );
        for marker in [
            "?hosted=1",
            "?hosted=1&g=7",
            "?hosted=1&g=x&n=y",
            "?hosted=1&g=0&n=y",
        ] {
            assert_eq!(parse_boot_marker(marker), BootMarker::InvalidHosted);
        }
    }

    #[test]
    fn every_emitted_envelope_carries_the_frame_generation() {
        let (api, wire) = api();
        api.navigate_library();
        api.bake_cover("/books/a.pdf");
        api.publish_digest("{}".to_string());
        api.emit(RuntimeFrame::Ready);
        let posted = wire.posted.borrow();
        assert_eq!(posted.len(), 4);
        let generation = posted[0].find(r#""generation":17"#);
        assert!(generation.is_some(), "{}", posted[0]);
        let kind = posted[0].find(r#""kind":"navigateLibrary""#);
        assert!(kind.is_some(), "{}", posted[0]);
        let bake_kind = posted[1].find(r#""kind":"bakeCover""#);
        assert!(bake_kind.is_some(), "{}", posted[1]);
        let bake_path = posted[1].find(r#""path":"/books/a.pdf""#);
        assert!(bake_path.is_some(), "{}", posted[1]);
        assert!(posted[3].contains(r#""kind":"ready""#), "{}", posted[3]);
    }

    #[test]
    fn a_gloss_save_leaves_over_the_port_for_the_shell_to_write() {
        let (api, wire) = api();
        api.save_gloss("b-3", "[]".to_string());
        let posted = wire.posted.borrow();
        assert_eq!(
            *posted,
            vec![r#"{"generation":17,"kind":"saveGloss","key":"b-3","marks":"[]"}"#.to_string()]
        );
    }

    #[test]
    fn a_status_update_carries_the_stage_it_names() {
        let (api, wire) = api();
        api.emit(RuntimeFrame::Status {
            stage: BootStage::Mounted,
        });
        let posted = wire.posted.borrow();
        assert_eq!(posted.len(), 1);
        assert_eq!(
            posted[0],
            r#"{"generation":17,"kind":"status","stage":"mounted"}"#
        );
    }

    struct WakeCount(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for WakeCount {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn poll(
        ticket: &mut ResolveTicket,
        wake: &std::sync::Arc<WakeCount>,
    ) -> Poll<Option<LaunchDocument>> {
        let waker = Waker::from(wake.clone());
        Pin::new(ticket).poll(&mut Context::from_waker(&waker))
    }

    #[test]
    fn disposal_cancels_every_pending_resolve_and_ignores_late_answers() {
        let (api, _) = api();
        let mut first = api.resolve_launch("/books/first.pdf");
        let mut second = api.resolve_launch("/books/second.pdf");
        let wake = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        assert!(poll(&mut first, &wake).is_pending());
        assert!(poll(&mut second, &wake).is_pending());
        api.cancel_resolves();
        assert_eq!(api.resolves.pending.borrow().len(), 0);
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(poll(&mut first, &wake), Poll::Ready(None));
        assert_eq!(poll(&mut second, &wake), Poll::Ready(None));
        api.settle_launch(1, None);
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::Relaxed), 2);
        assert_eq!(api.resolves.pending.borrow().len(), 0);
    }

    #[test]
    fn a_resolve_round_trip_pairs_and_wakes_only_its_request() {
        let (api, wire) = api();
        let mut first = api.resolve_launch("/books/first.pdf");
        let mut second = api.resolve_launch("/books/second.pdf");
        let wake = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        assert!(poll(&mut first, &wake).is_pending());
        assert!(poll(&mut second, &wake).is_pending());
        let posted = wire.posted.borrow();
        assert_eq!(posted.len(), 2);
        assert!(posted[0].contains(r#""request":1"#), "{}", posted[0]);
        assert!(posted[1].contains(r#""request":2"#), "{}", posted[1]);
        assert_eq!(api.resolves.pending.borrow().len(), 2);
        api.settle_launch(2, None);
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(poll(&mut second, &wake), Poll::Ready(None));
        assert!(poll(&mut first, &wake).is_pending());
        assert_eq!(api.resolves.pending.borrow().len(), 1);
        api.settle_launch(99, None);
        api.settle_launch(2, None);
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(api.resolves.pending.borrow().len(), 1);
    }

    #[test]
    fn a_stored_launch_is_not_replaced_with_a_synchronous_miss() {
        let (api, _) = api();
        let mut ticket = api.resolve_launch("/books/read.pdf");
        let wake = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        assert!(poll(&mut ticket, &wake).is_pending());
        let launch = LaunchDocument {
            book_id: Some("stored-book".into()),
            path: "/books/read.pdf".into(),
            resume_page: 8,
            saved_fraction: Some(0.5),
            blend_override: false,
            cover_data_url: None,
            display_name: Some("Read".into()),
        };
        api.settle_launch(1, Some(launch.clone()));
        assert_eq!(poll(&mut ticket, &wake), Poll::Ready(Some(launch)));
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(api.resolves.pending.borrow().is_empty());
    }

    #[test]
    fn dropping_a_query_or_its_api_releases_and_wakes_the_owned_continuation() {
        let (api, _) = api();
        let first = api.resolve_launch("/books/first.pdf");
        drop(first);
        assert!(api.resolves.pending.borrow().is_empty());
        api.settle_launch(1, None);
        let mut second = api.resolve_launch("/books/second.pdf");
        let wake = std::sync::Arc::new(WakeCount(std::sync::atomic::AtomicUsize::new(0)));
        assert!(poll(&mut second, &wake).is_pending());
        drop(api);
        assert_eq!(wake.0.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert_eq!(poll(&mut second, &wake), Poll::Ready(None));
    }
}
