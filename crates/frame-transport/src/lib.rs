//! The frame transport: the wire half of the frame protocol (§7, §8).
//!
//! `runtime-contract::protocol` owns the VOCABULARY every side serializes;
//! this crate owns how it moves: a cloneable message sink both runtime
//! runtimes call their [`ShellApi`] through ([`PortShellApi`]), the request
//! ids that turn the one synchronous bridge query (`resolve_launch`) into a
//! port round trip, and — behind `wasm32` — the `MessagePort` wire itself
//! ([`wasm::PortWire`]). Hosted route artifacts adopt a nonce/generation
//! authenticated channel offer from their actual same-origin parent.
//!
//! Host tests exercise everything except the DOM: the wire is a trait with a
//! recording double, so the envelope stamping (generation on every message,
//! request ids pairing resolve answers to their questions) is asserted off
//! the browser entirely.

#![forbid(unsafe_code)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use runtime_contract::ShellApi;
use runtime_contract::boundary::{DocStatusReport, LaunchDocument, ReadPoint};
use runtime_contract::protocol::{RuntimeEnvelope, RuntimeFrame};

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
#[derive(Clone, Default)]
pub struct TestWire {
    pub posted: Rc<RefCell<Vec<String>>>,
}

impl Wire for TestWire {
    fn post_json(&self, json: String) {
        self.posted.borrow_mut().push(json);
    }
}

/// The shared settle cell a request parks its answer at (`None` = still
/// pending), handed to the opener as the [`ResolveTicket`] it polls.
type TicketSlot = Rc<RefCell<Option<Option<LaunchDocument>>>>;

/// One outstanding `resolve_launch` round trip: the request id the Shell's
/// [`runtime_contract::protocol::ShellFrame::ResolveLaunchAnswer`] arrives
/// tagged with, and the descriptor it settled to (`None` while pending).
#[derive(Default)]
pub struct PendingResolves {
    next: Cell<u64>,
    pending: Rc<RefCell<HashMap<u64, TicketSlot>>>,
}

impl PendingResolves {
    /// The id the next request must carry. Ids are per-frame; the generation
    /// on the envelope already scopes them globally.
    pub fn issue(&self) -> (u64, ResolveTicket) {
        let id = self.next.get().saturating_add(1);
        self.next.set(id);
        let ticket = Rc::new(RefCell::new(None));
        self.pending.borrow_mut().insert(id, ticket.clone());
        (id, ResolveTicket { inner: ticket })
    }

    /// A resolve answer landing, matched to its request by id. A stale
    /// answer (unknown id — a double answer or one from another generation)
    /// is dropped, never parked: the identity guard belongs at the envelope.
    pub fn settle(&self, request: u64, document: Option<LaunchDocument>) {
        if let Some(ticket) = self.pending.borrow_mut().remove(&request) {
            *ticket.borrow_mut() = Some(document);
        }
    }

    /// Disposal settles unanswered tickets without retaining descriptors;
    /// an answer arriving after this cannot resurrect a removed request.
    pub fn cancel_all(&self) {
        for (_, ticket) in self.pending.borrow_mut().drain() {
            *ticket.borrow_mut() = Some(None);
        }
    }

    /// Outstanding request ids — the diagnostics window into open queries.
    pub fn count(&self) -> usize {
        self.pending.borrow().len()
    }
}

/// The handle an async opener polls for its answer. `None` until the Shell's
/// answer lands; the id the frame was issued is gone by then, so the ticket
/// is the whole query.
#[derive(Clone)]
pub struct ResolveTicket {
    inner: TicketSlot,
}

impl ResolveTicket {
    /// `None` while the Shell's answer is still in flight — a pending query,
    /// not a miss. `Some(document)` is the settled answer, where `None`
    /// inside means the store had no row for the path.
    pub fn take(&self) -> Option<Option<LaunchDocument>> {
        self.inner.borrow_mut().take()
    }
}

/// `ShellApi` over the frame port: every boundary command serialized onto
/// the wire behind its frame's generation (§8 — a message without ITS
/// generation cannot leave this api, so a stale generation is unmakeable
/// here and the Shell's drop log reads it there).
pub struct PortShellApi<W: Wire> {
    wire: W,
    generation: u64,
    resolves: Rc<PendingResolves>,
}

impl<W: Wire> PortShellApi<W> {
    pub fn new(wire: W, generation: u64, resolves: Rc<PendingResolves>) -> Self {
        Self {
            wire,
            generation,
            resolves,
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

    /// The request id a resolve answer will arrive tagged with, plus the
    /// ticket the async opener polls. The opener's flow (see the reader's
    /// document-open service) cannot be synchronous over a port: ask, then
    /// await the ticket — or park a continuation behind the id, the two
    /// shells of the same round trip.
    pub fn ask_resolve_launch(&self, path: &str) -> (u64, ResolveTicket) {
        let (id, ticket) = self.resolves.issue();
        self.emit(RuntimeFrame::ResolveLaunch {
            request: id,
            path: path.to_string(),
        });
        (id, ticket)
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
    fn resolve_launch(&self, path: &str) -> Option<LaunchDocument> {
        // A port cannot answer synchronously: the synchronous trait entry is
        // the same-page bridge's shape. The frame's open pipeline uses
        // [`PortShellApi::ask_resolve_launch`] and awaits; a caller that
        // still reaches here (the single generic call site guarded off frame
        // boots) gets the honest answer for "no channel": nothing resolved.
        let _ = self.ask_resolve_launch(path);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use runtime_contract::protocol::BootStage;

    fn api() -> (PortShellApi<TestWire>, TestWire, Rc<PendingResolves>) {
        let wire = TestWire::default();
        let resolves = Rc::new(PendingResolves::default());
        let api = PortShellApi::new(wire.clone(), 17, resolves.clone());
        (api, wire, resolves)
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
        let (api, wire, _) = api();
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
        let (api, wire, _) = api();
        api.save_gloss("b-3", "[]".to_string());
        let posted = wire.posted.borrow();
        assert_eq!(
            *posted,
            vec![r#"{"generation":17,"kind":"saveGloss","key":"b-3","marks":"[]"}"#.to_string()]
        );
    }

    #[test]
    fn a_status_update_carries_the_stage_it_names() {
        let (api, wire, _) = api();
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

    #[test]
    fn disposal_cancels_every_pending_resolve_and_ignores_late_answers() {
        let (api, _, resolves) = api();
        let (first_id, first) = api.ask_resolve_launch("/books/first.pdf");
        let (_, second) = api.ask_resolve_launch("/books/second.pdf");
        resolves.cancel_all();
        assert_eq!(resolves.count(), 0);
        assert_eq!(first.take(), Some(None));
        assert_eq!(second.take(), Some(None));
        resolves.settle(first_id, None);
        assert!(first.take().is_none());
        assert_eq!(resolves.count(), 0);
    }

    #[test]
    fn a_resolve_round_trip_pairs_the_answer_to_its_request() {
        let (api, wire, resolves) = api();
        let (_first_id, first) = api.ask_resolve_launch("/books/first.pdf");
        let (_second_id, second) = api.ask_resolve_launch("/books/second.pdf");
        let posted = wire.posted.borrow();
        assert_eq!(posted.len(), 2);
        assert!(posted[0].contains(r#""request":1"#), "{}", posted[0]);
        assert!(posted[1].contains(r#""request":2"#), "{}", posted[1]);
        assert_eq!(resolves.count(), 2);
        // The answer to the SECOND request settles its ticket only: a
        // settled "no row" reads Some(None), distinguishable from pending.
        resolves.settle(2, None);
        assert_eq!(second.take(), Some(None));
        // A repeated take of the settled ticket yields nothing; the known
        // pending first ticket stays pending.
        assert!(second.take().is_none());
        assert!(first.take().is_none());
        assert_eq!(resolves.count(), 1);
        resolves.settle(99, None);
        assert_eq!(resolves.count(), 1, "a stale answer is dropped, not parked");
    }
}
