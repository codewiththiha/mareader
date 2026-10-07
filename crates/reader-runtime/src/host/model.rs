//! The pane domain, pure: identities, the descriptor, the lifecycle
//! machine and the manager's core.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Identities
// ---------------------------------------------------------------------------

/// A pane's identity inside one host: minted from a monotonic
/// counter, never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PaneId(u64);

impl PaneId {
    /// The raw number, for diagnostics and DOM attributes only.
    pub fn get(self) -> u64 {
        self.0
    }

    /// The id a pane frame was booted with.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn from_raw(n: u64) -> Self {
        Self(n)
    }

    /// A pane id out of thin air, for the pure layout tests only.
    #[cfg(test)]
    pub(crate) fn for_tests(n: u64) -> Self {
        Self(n)
    }
}

impl std::fmt::Display for PaneId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pane-{}", self.0)
    }
}

/// Which document a pane shows: the library row, else the address.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocumentId(String);

impl DocumentId {
    /// The document identity for a launch: `book:<row id>`, else
    /// `path:<address>`.
    pub fn from_launch(book_id: Option<&str>, path: &str) -> Option<Self> {
        match book_id {
            Some(id) if !id.is_empty() => Some(Self(format!("book:{id}"))),
            _ if !path.is_empty() => Some(Self(format!("path:{path}"))),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The document a pane is asked to show, as data.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentRef {
    pub document_id: DocumentId,
    pub path: String,
}

/// The format tag a pane reports; a label the host never branches on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneFormat {
    /// No document yet, or one whose format is not decided.
    #[default]
    Pending,
    Pdf,
    Markdown,
    Text,
}

/// A pane's box in the host's slot, in CSS px.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize)]
pub struct PaneBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl PaneBounds {
    /// The whole slot, for a workspace of one pane.
    pub fn filling(width: f64, height: f64) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }
}

// ---------------------------------------------------------------------------
// The descriptor
// ---------------------------------------------------------------------------

/// What a caller asks for when it creates a pane. Data only.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PaneRequest {
    pub document: Option<DocumentRef>,
    pub format: PaneFormat,
    /// The 1-based page the pane's viewport starts on.
    pub initial_page: u32,
    /// The initial zoom, `None` for the fit the settings resolve.
    pub initial_zoom: Option<f64>,
    /// Whether the new pane asks to become the active one.
    pub request_focus: bool,
}

/// The pane as the manager records it: request plus minted id.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneDescriptor {
    pub pane_id: PaneId,
    pub document: Option<DocumentRef>,
    pub format: PaneFormat,
    pub initial_page: u32,
    pub initial_zoom: Option<f64>,
    pub request_focus: bool,
}

impl PaneDescriptor {
    /// The descriptor a pane frame rebuilds from its boot message.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn remote(pane_id: PaneId, request: PaneRequest) -> Self {
        Self::minted(pane_id, request)
    }

    fn minted(pane_id: PaneId, request: PaneRequest) -> Self {
        Self {
            pane_id,
            document: request.document,
            format: request.format,
            initial_page: request.initial_page.max(1),
            initial_zoom: request.initial_zoom,
            request_focus: request.request_focus,
        }
    }
}

// ---------------------------------------------------------------------------
// The lifecycle
// ---------------------------------------------------------------------------

/// `New → Mounting → Ready → (Suspended) → Disposing → Disposed`;
/// terminal and never reused.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaneLifecycle {
    #[default]
    New,
    Mounting,
    Ready,
    Suspended,
    Disposing,
    Disposed,
}

impl PaneLifecycle {
    /// Whether new pane WORK may start: refused while suspended, from
    /// `Disposing` on.
    pub fn admits_work(self) -> bool {
        matches!(self, Self::New | Self::Mounting | Self::Ready)
    }

    /// Whether TEARDOWN may run: one state longer than work.
    pub fn admits_teardown(self) -> bool {
        self != Self::Disposed
    }

    /// Alive: anything before `Disposing`.
    pub fn is_live(self) -> bool {
        !matches!(self, Self::Disposing | Self::Disposed)
    }
}

// ---------------------------------------------------------------------------
// Errors and transition results
// ---------------------------------------------------------------------------

/// Why the manager refused an operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneError {
    /// No pane with this id was ever created by this manager.
    Unknown(PaneId),
    /// The pane is disposing or disposed: it takes no further operations.
    Gone(PaneId),
    /// The transition is not legal from the pane's current state.
    Illegal { pane: PaneId, from: PaneLifecycle },
    /// The host itself is disposed: it creates nothing any more.
    HostDisposed,
    /// A pane was asked for outside every reactive owner.
    Unowned,
    /// The workspace already holds [`MAX_PANES`] live panes.
    WorkspaceFull,
    /// The layout refused the placement.
    Layout(super::tree::TreeError),
}

/// The most live panes one workspace holds: a resource bound first.
pub const MAX_PANES: usize = 4;

/// One focus hand-over: blur the first, then focus the second.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct FocusChange {
    pub blur: Option<PaneId>,
    pub focus: Option<PaneId>,
}

impl FocusChange {
    pub fn is_noop(&self) -> bool {
        self.blur.is_none() && self.focus.is_none()
    }
}

// ---------------------------------------------------------------------------
// The manager core
// ---------------------------------------------------------------------------

/// One pane as the core tracks it.
#[derive(Clone, PartialEq, Debug)]
pub struct PaneRecord {
    pub descriptor: PaneDescriptor,
    pub lifecycle: PaneLifecycle,
    pub bounds: PaneBounds,
}

/// The manager's bookkeeping: tombstones included, placement order,
/// the one active pane.
#[derive(Clone, Debug, Default)]
pub struct PaneManagerCore {
    next_id: u64,
    records: BTreeMap<PaneId, PaneRecord>,
    /// Live panes in placement order; disposing panes leave at once.
    order: Vec<PaneId>,
    active: Option<PaneId>,
    host_disposed: bool,
}

impl PaneManagerCore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a pane; a request for focus hands it over.
    pub fn create(
        &mut self,
        request: PaneRequest,
    ) -> Result<(PaneDescriptor, FocusChange), PaneError> {
        if self.host_disposed {
            return Err(PaneError::HostDisposed);
        }
        if self.order.len() >= MAX_PANES {
            return Err(PaneError::WorkspaceFull);
        }
        self.next_id += 1;
        let id = PaneId(self.next_id);
        let wants_focus = request.request_focus || self.active.is_none();
        let descriptor = PaneDescriptor::minted(id, request);
        self.records.insert(
            id,
            PaneRecord {
                descriptor: descriptor.clone(),
                lifecycle: PaneLifecycle::New,
                bounds: PaneBounds::default(),
            },
        );
        self.order.push(id);
        let focus = if wants_focus {
            self.hand_focus_to(Some(id))
        } else {
            FocusChange::default()
        };
        Ok((descriptor, focus))
    }

    fn live_record(&mut self, id: PaneId) -> Result<&mut PaneRecord, PaneError> {
        match self.records.get_mut(&id) {
            None => Err(PaneError::Unknown(id)),
            Some(record) if !record.lifecycle.is_live() => Err(PaneError::Gone(id)),
            Some(record) => Ok(record),
        }
    }

    /// `New → Mounting`.
    pub fn begin_mount(&mut self, id: PaneId) -> Result<(), PaneError> {
        let record = self.live_record(id)?;
        match record.lifecycle {
            PaneLifecycle::New => {
                record.lifecycle = PaneLifecycle::Mounting;
                Ok(())
            }
            from => Err(PaneError::Illegal { pane: id, from }),
        }
    }

    /// `Mounting → Ready`.
    pub fn mark_ready(&mut self, id: PaneId) -> Result<(), PaneError> {
        let record = self.live_record(id)?;
        match record.lifecycle {
            PaneLifecycle::Mounting => {
                record.lifecycle = PaneLifecycle::Ready;
                Ok(())
            }
            from => Err(PaneError::Illegal { pane: id, from }),
        }
    }

    /// `Ready → Suspended`: the pane keeps its session but takes no work.
    pub fn suspend(&mut self, id: PaneId) -> Result<(), PaneError> {
        let record = self.live_record(id)?;
        match record.lifecycle {
            PaneLifecycle::Ready => {
                record.lifecycle = PaneLifecycle::Suspended;
                Ok(())
            }
            from => Err(PaneError::Illegal { pane: id, from }),
        }
    }

    /// `Suspended → Ready`.
    pub fn resume(&mut self, id: PaneId) -> Result<(), PaneError> {
        let record = self.live_record(id)?;
        match record.lifecycle {
            PaneLifecycle::Suspended => {
                record.lifecycle = PaneLifecycle::Ready;
                Ok(())
            }
            from => Err(PaneError::Illegal { pane: id, from }),
        }
    }

    /// Make `id` the active pane: the one focus authority.
    pub fn focus(&mut self, id: PaneId) -> Result<FocusChange, PaneError> {
        self.live_record(id)?;
        Ok(self.hand_focus_to(Some(id)))
    }

    fn hand_focus_to(&mut self, next: Option<PaneId>) -> FocusChange {
        if self.active == next {
            return FocusChange::default();
        }
        let blur = self.active;
        self.active = next;
        FocusChange { blur, focus: next }
    }

    /// Record measured bounds; `Ok(false)` when they did not change.
    pub fn resize(&mut self, id: PaneId, bounds: PaneBounds) -> Result<bool, PaneError> {
        let record = self.live_record(id)?;
        if record.bounds == bounds {
            return Ok(false);
        }
        record.bounds = bounds;
        Ok(true)
    }

    /// Begin closing `id`: out of the order, focus handed on.
    pub fn begin_close(
        &mut self,
        id: PaneId,
        prefer: Option<PaneId>,
    ) -> Result<FocusChange, PaneError> {
        let record = self.live_record(id)?;
        record.lifecycle = PaneLifecycle::Disposing;
        let at = self.order.iter().position(|known| *known == id);
        if let Some(at) = at {
            self.order.remove(at);
        }
        if self.active != Some(id) {
            return Ok(FocusChange::default());
        }
        let preferred = prefer.filter(|pane| self.order.contains(pane));
        let successor = preferred.or_else(|| {
            at.and_then(|at| {
                self.order
                    .get(at)
                    .or_else(|| at.checked_sub(1).and_then(|before| self.order.get(before)))
                    .copied()
            })
        });
        // The closing pane is blurred by its own dispose, not the
        // hand-over.
        self.active = successor;
        Ok(FocusChange {
            blur: None,
            focus: successor,
        })
    }

    /// `Disposing → Disposed`; the record stays as a tombstone.
    pub fn finish_dispose(&mut self, id: PaneId) -> Result<(), PaneError> {
        match self.records.get_mut(&id) {
            None => Err(PaneError::Unknown(id)),
            Some(record) if record.lifecycle == PaneLifecycle::Disposing => {
                record.lifecycle = PaneLifecycle::Disposed;
                Ok(())
            }
            Some(record) => Err(PaneError::Illegal {
                pane: id,
                from: record.lifecycle,
            }),
        }
    }

    /// Dispose the workspace: every live pane disposes, idempotently.
    pub fn dispose_all(&mut self) -> Vec<PaneId> {
        self.host_disposed = true;
        self.active = None;
        let ids = std::mem::take(&mut self.order);
        for id in &ids {
            if let Some(record) = self.records.get_mut(id) {
                record.lifecycle = PaneLifecycle::Disposing;
            }
        }
        ids
    }

    pub fn active(&self) -> Option<PaneId> {
        self.active
    }

    /// Live panes in placement order.
    pub fn live(&self) -> &[PaneId] {
        &self.order
    }

    /// Panes whose dispose began and has not finished, in id order.
    pub fn disposing(&self) -> Vec<PaneId> {
        self.records
            .iter()
            .filter(|(_, record)| record.lifecycle == PaneLifecycle::Disposing)
            .map(|(id, _)| *id)
            .collect()
    }

    pub fn record(&self, id: PaneId) -> Option<&PaneRecord> {
        self.records.get(&id)
    }

    pub fn lifecycle(&self, id: PaneId) -> Option<PaneLifecycle> {
        self.records.get(&id).map(|record| record.lifecycle)
    }

    pub fn is_host_disposed(&self) -> bool {
        self.host_disposed
    }

    /// Every pane this core ever created, disposed ones included.
    pub fn created(&self) -> usize {
        self.records.len()
    }

    /// The invariants the tests check after every operation.
    pub fn check_invariants(&self) -> Result<(), String> {
        if let Some(active) = self.active {
            let lifecycle = self.lifecycle(active);
            if !lifecycle.is_some_and(PaneLifecycle::is_live) {
                return Err(format!("active {active} is not live ({lifecycle:?})"));
            }
            if !self.order.contains(&active) {
                return Err(format!("active {active} is not placed"));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.order {
            if !seen.insert(*id) {
                return Err(format!("{id} placed twice"));
            }
            if !self.lifecycle(*id).is_some_and(PaneLifecycle::is_live) {
                return Err(format!("{id} placed but not live"));
            }
        }
        let live = self
            .records
            .values()
            .filter(|record| record.lifecycle.is_live())
            .count();
        if live != self.order.len() {
            return Err(format!("{live} live panes but {} placed", self.order.len()));
        }
        if self.host_disposed && (self.active.is_some() || !self.order.is_empty()) {
            return Err("a disposed host still places panes".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str) -> Option<DocumentRef> {
        Some(DocumentRef {
            document_id: DocumentId::from_launch(None, path).expect("a path names a document"),
            path: path.to_string(),
        })
    }

    fn request(path: &str) -> PaneRequest {
        PaneRequest {
            document: doc(path),
            format: PaneFormat::Pdf,
            initial_page: 1,
            initial_zoom: None,
            request_focus: false,
        }
    }

    fn ok(core: &PaneManagerCore) {
        core.check_invariants().expect("manager invariants");
    }

    fn ready(core: &mut PaneManagerCore, path: &str) -> PaneId {
        let (descriptor, _) = core.create(request(path)).expect("create");
        core.begin_mount(descriptor.pane_id).expect("mount");
        core.mark_ready(descriptor.pane_id).expect("ready");
        ok(core);
        descriptor.pane_id
    }

    #[test]
    fn the_first_pane_is_created_new_and_takes_focus() {
        let mut core = PaneManagerCore::new();
        let (descriptor, focus) = core.create(request("/a.pdf")).expect("create");
        assert_eq!(core.lifecycle(descriptor.pane_id), Some(PaneLifecycle::New));
        assert_eq!(
            focus,
            FocusChange {
                blur: None,
                focus: Some(descriptor.pane_id)
            }
        );
        assert_eq!(core.active(), Some(descriptor.pane_id));
        ok(&core);
    }

    #[test]
    fn the_lifecycle_walks_new_mounting_ready_suspended_disposing_disposed() {
        let mut core = PaneManagerCore::new();
        let (d, _) = core.create(request("/a.pdf")).expect("create");
        let id = d.pane_id;
        assert!(core.mark_ready(id).is_err(), "Ready needs Mounting first");
        core.begin_mount(id).expect("mount");
        assert!(core.begin_mount(id).is_err(), "no double mount");
        core.mark_ready(id).expect("ready");
        core.suspend(id).expect("suspend");
        assert!(!core.lifecycle(id).expect("known").admits_work());
        core.resume(id).expect("resume");
        assert!(core.lifecycle(id).expect("known").admits_work());
        core.begin_close(id, None).expect("close");
        assert_eq!(core.lifecycle(id), Some(PaneLifecycle::Disposing));
        assert!(core.lifecycle(id).expect("known").admits_teardown());
        core.finish_dispose(id).expect("disposed");
        assert_eq!(core.lifecycle(id), Some(PaneLifecycle::Disposed));
        assert!(!core.lifecycle(id).expect("known").admits_teardown());
        ok(&core);
    }

    #[test]
    fn focus_has_one_authority_the_previous_pane_blurs() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        let b = ready(&mut core, "/b.pdf");
        assert_eq!(core.active(), Some(a), "a later pane does not steal focus");
        let change = core.focus(b).expect("focus b");
        assert_eq!(
            change,
            FocusChange {
                blur: Some(a),
                focus: Some(b)
            }
        );
        assert_eq!(core.active(), Some(b));
        assert!(
            core.focus(b).expect("again").is_noop(),
            "refocus is a no-op"
        );
        ok(&core);
    }

    #[test]
    fn a_pane_that_requests_focus_gets_it_at_creation() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        let (d, change) = core
            .create(PaneRequest {
                request_focus: true,
                ..request("/b.pdf")
            })
            .expect("create");
        assert_eq!(change.blur, Some(a));
        assert_eq!(change.focus, Some(d.pane_id));
        assert_eq!(core.active(), Some(d.pane_id));
        ok(&core);
    }

    #[test]
    fn closing_the_active_pane_hands_focus_on_and_the_last_close_empties() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        let b = ready(&mut core, "/b.pdf");
        let c = ready(&mut core, "/c.pdf");
        core.focus(b).expect("focus b");
        // b closes: the pane that took its place (c) becomes active.
        let change = core.begin_close(b, None).expect("close b");
        assert_eq!(change.focus, Some(c));
        assert_eq!(core.active(), Some(c));
        ok(&core);
        // c closes: nothing after it, so the one before (a) takes over.
        let change = core.begin_close(c, None).expect("close c");
        assert_eq!(change.focus, Some(a));
        ok(&core);
        // closing an inactive pane leaves focus alone.
        let d = ready(&mut core, "/d.pdf");
        assert!(core.begin_close(d, None).expect("close d").is_noop());
        assert_eq!(core.active(), Some(a));
        // the last pane closes: nobody is active.
        let change = core.begin_close(a, None).expect("close a");
        assert_eq!(change.focus, None);
        assert_eq!(core.active(), None);
        assert!(core.live().is_empty());
        ok(&core);
    }

    #[test]
    fn closing_the_active_pane_prefers_the_layouts_successor_when_it_is_live() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        let b = ready(&mut core, "/b.pdf");
        let c = ready(&mut core, "/c.pdf");
        core.focus(b).expect("focus b");
        // The layout's preference wins over the order.
        let change = core.begin_close(b, Some(a)).expect("close b");
        assert_eq!(
            change,
            FocusChange {
                blur: None,
                focus: Some(a)
            }
        );
        ok(&core);
        // A preference that is not live falls back to the order.
        let change = core.begin_close(a, Some(a)).expect("close a");
        assert_eq!(change.focus, Some(c));
        ok(&core);
        let d = ready(&mut core, "/d.pdf");
        core.focus(d).expect("focus d");
        let change = core.begin_close(d, Some(b)).expect("close d");
        assert_eq!(change.focus, Some(c), "b is gone, so the order decides");
        // Closing an inactive pane ignores the preference entirely.
        let e = ready(&mut core, "/e.pdf");
        assert!(core.begin_close(e, Some(e)).expect("close e").is_noop());
        assert_eq!(core.active(), Some(c));
        ok(&core);
    }

    #[test]
    fn a_full_workspace_refuses_another_pane_until_one_closes() {
        let mut core = PaneManagerCore::new();
        let panes: Vec<PaneId> = (0..MAX_PANES)
            .map(|n| ready(&mut core, &format!("/{n}.pdf")))
            .collect();
        let created = core.created();
        assert_eq!(
            core.create(request("/one-more.pdf")),
            Err(PaneError::WorkspaceFull)
        );
        assert_eq!(core.created(), created, "a refused create mints nothing");
        ok(&core);
        core.begin_close(panes[0], None).expect("close one");
        // A disposing pane is out of the workspace already.
        ready(&mut core, "/one-more.pdf");
        ok(&core);
    }

    #[test]
    fn a_disposed_pane_refuses_every_operation_and_is_never_reused() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        core.begin_close(a, None).expect("close");
        // Disposing already refuses work operations.
        assert_eq!(core.focus(a), Err(PaneError::Gone(a)));
        core.finish_dispose(a).expect("disposed");
        assert_eq!(core.focus(a), Err(PaneError::Gone(a)));
        assert_eq!(
            core.resize(a, PaneBounds::filling(10.0, 10.0)),
            Err(PaneError::Gone(a))
        );
        assert_eq!(core.begin_mount(a), Err(PaneError::Gone(a)));
        assert_eq!(core.mark_ready(a), Err(PaneError::Gone(a)));
        assert_eq!(core.suspend(a), Err(PaneError::Gone(a)));
        assert_eq!(core.begin_close(a, None), Err(PaneError::Gone(a)));
        assert!(core.finish_dispose(a).is_err(), "disposed twice");
        // A new pane for the SAME document gets a new id.
        let again = ready(&mut core, "/a.pdf");
        assert_ne!(again, a);
        assert!(again > a, "ids are monotonic");
        ok(&core);
    }

    #[test]
    fn an_unknown_pane_is_unknown_not_gone() {
        let mut core = PaneManagerCore::new();
        let ghost = PaneId(42);
        assert_eq!(core.focus(ghost), Err(PaneError::Unknown(ghost)));
        assert_eq!(core.finish_dispose(ghost), Err(PaneError::Unknown(ghost)));
    }

    #[test]
    fn dispose_all_cascades_to_every_pane_and_closes_the_host() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        let b = ready(&mut core, "/b.pdf");
        let (c, _) = core.create(request("/c.pdf")).expect("a pane still New");
        let torn = core.dispose_all();
        assert_eq!(torn, vec![a, b, c.pane_id], "placement order");
        assert_eq!(core.active(), None);
        for id in &torn {
            assert_eq!(core.lifecycle(*id), Some(PaneLifecycle::Disposing));
            core.finish_dispose(*id).expect("teardown finished");
            assert_eq!(core.lifecycle(*id), Some(PaneLifecycle::Disposed));
        }
        ok(&core);
        assert!(core.dispose_all().is_empty(), "idempotent");
        assert_eq!(
            core.create(request("/d.pdf")).map(|_| ()),
            Err(PaneError::HostDisposed),
            "a disposed host creates nothing"
        );
        assert_eq!(core.created(), 3);
    }

    #[test]
    fn resize_records_bounds_and_reports_real_changes_only() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/a.pdf");
        let bounds = PaneBounds::filling(800.0, 600.0);
        assert_eq!(core.resize(a, bounds), Ok(true));
        assert_eq!(core.resize(a, bounds), Ok(false));
        assert_eq!(core.record(a).map(|r| r.bounds), Some(bounds));
        assert_eq!(PaneBounds::filling(-5.0, 3.0).width, 0.0);
    }

    #[test]
    fn a_pane_id_is_not_a_document_id() {
        let mut core = PaneManagerCore::new();
        let a = ready(&mut core, "/same.pdf");
        let b = ready(&mut core, "/same.pdf");
        assert_ne!(a, b, "two panes on one document are two panes");
        let doc_a = core.record(a).and_then(|r| r.descriptor.document.clone());
        let doc_b = core.record(b).and_then(|r| r.descriptor.document.clone());
        assert_eq!(doc_a, doc_b, "…showing one document");
        let doc_id = doc_a.expect("a document").document_id;
        assert_ne!(doc_id.as_str(), a.to_string());
        assert_eq!(doc_id.as_str(), "path:/same.pdf");
        // The library row wins over the address when it exists.
        assert_eq!(
            DocumentId::from_launch(Some("row-7"), "/same.pdf").map(|d| d.as_str().to_string()),
            Some("book:row-7".to_string())
        );
        assert_eq!(DocumentId::from_launch(None, ""), None);
        assert_eq!(DocumentId::from_launch(Some(""), ""), None);
    }

    #[test]
    fn a_pane_without_a_document_is_legal() {
        let mut core = PaneManagerCore::new();
        let (d, _) = core
            .create(PaneRequest {
                document: None,
                ..PaneRequest::default()
            })
            .expect("a warm pane");
        assert_eq!(d.format, PaneFormat::Pending);
        assert_eq!(d.initial_page, 1, "page is clamped to 1-based");
        ok(&core);
    }

    /// No operation sequence can break single-active ownership.
    #[test]
    fn no_sequence_of_operations_breaks_single_active_ownership() {
        let mut core = PaneManagerCore::new();
        // A small deterministic LCG: reproducible, dependency-free.
        let mut seed: u64 = 0x5eed;
        let mut next = move |bound: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % bound
        };
        let mut ids: Vec<PaneId> = Vec::new();
        for _ in 0..2_000 {
            let pick = |n: u64, ids: &Vec<PaneId>| ids.get(n as usize % ids.len().max(1)).copied();
            match next(7) {
                0 => {
                    if let Ok((d, _)) = core.create(PaneRequest {
                        request_focus: next(2) == 0,
                        ..request("/x.pdf")
                    }) {
                        ids.push(d.pane_id);
                    }
                }
                1 => {
                    if let Some(id) = pick(next(64), &ids) {
                        let _ = core.begin_mount(id);
                        let _ = core.mark_ready(id);
                    }
                }
                2 => {
                    if let Some(id) = pick(next(64), &ids) {
                        let _ = core.focus(id);
                    }
                }
                3 => {
                    if let Some(id) = pick(next(64), &ids) {
                        let _ = core.begin_close(id, None);
                    }
                }
                4 => {
                    if let Some(id) = pick(next(64), &ids) {
                        let _ = core.finish_dispose(id);
                    }
                }
                5 => {
                    if let Some(id) = pick(next(64), &ids) {
                        let _ = core.resize(id, PaneBounds::filling(next(900) as f64, 700.0));
                        let _ = core.suspend(id);
                        let _ = core.resume(id);
                    }
                }
                _ => {
                    if next(50) == 0 {
                        core.dispose_all();
                    }
                }
            }
            core.check_invariants()
                .expect("invariants hold after every step");
        }
    }
}
