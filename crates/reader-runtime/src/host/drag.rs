//! The document drag session: a small state machine the host owns, one per
//! workspace, holding typed data only — the source descriptor, the geometry
//! measured when the drag started, the target shown. No DOM element, no
//! pane runtime, no document state: a drag is an intent until it is dropped,
//! and a drop is handed to the command layer ([`super::commands`]), which
//! alone mutates the workspace.
//!
//! ```text
//! Idle ─arm─▶ Arming ─(threshold)─▶ Dragging{target} ─release─▶ Idle + DropIntent
//!   └────────────start (keyboard, Shell relay)──▶ Dragging     └cancel─▶ Idle
//! ```
//!
//! `Arming` does no work at all: the geometry is measured when the pointer
//! crosses the threshold, never before, so a press that stays a click costs
//! nothing and changes nothing.

use runtime_contract::boundary::DocumentDragDescriptor;

use super::drop_target::{DropTarget, Edge, format_label};
use super::geometry::DropGeometry;
use super::model::{DocumentId, PaneBounds, PaneFormat, PaneId};

/// The repository's one drag threshold (`app_ui`'s draggable item): a press
/// that moves this far or less is still a click.
pub use app_ui::components::primitives::interactions::draggable_item::DRAG_THRESHOLD_PX;

/// Where a dragged document came from. The payload identifies the resource;
/// the command layer resolves it through the established open path.
#[derive(Clone, Debug, PartialEq)]
pub enum DragOrigin {
    /// A pane in this workspace: the drop opens ANOTHER view of its document
    /// in a new pane. The source pane is never touched (no session moves).
    Pane(PaneId),
    /// A library row the Shell carried across the runtime switch.
    Library(DocumentDragDescriptor),
}

/// The dragged document, as the host reasons about it.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentDragSource {
    pub document: Option<DocumentId>,
    pub origin: DragOrigin,
    pub format: PaneFormat,
    pub label: String,
}

/// A drop the command layer is asked to carry out.
#[derive(Clone, Debug, PartialEq)]
pub struct DropIntent {
    pub source: DocumentDragSource,
    pub target: DropTarget,
}

/// The session.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum DragSession {
    #[default]
    Idle,
    /// Pressed on a source; the pointer has not left the threshold yet.
    Arming {
        source: DocumentDragSource,
        origin: (f64, f64),
    },
    /// A drag: the geometry measured when it started, and the target the
    /// pointer (or the keyboard) chose, if any.
    Dragging {
        source: DocumentDragSource,
        geometry: DropGeometry,
        target: Option<DropTarget>,
        over_workspace: bool,
    },
}

impl DragSession {
    /// The session's state in one word, for diagnostics.
    pub fn phase(&self) -> &'static str {
        match self {
            DragSession::Idle => "idle",
            DragSession::Arming { .. } => "arming",
            DragSession::Dragging {
                target: Some(_), ..
            } => "overTarget",
            DragSession::Dragging {
                over_workspace: true,
                ..
            } => "overWorkspace",
            DragSession::Dragging { .. } => "dragging",
        }
    }

    pub fn is_live(&self) -> bool {
        !matches!(self, DragSession::Idle)
    }

    pub fn is_dragging(&self) -> bool {
        matches!(self, DragSession::Dragging { .. })
    }

    pub fn source(&self) -> Option<&DocumentDragSource> {
        match self {
            DragSession::Idle => None,
            DragSession::Arming { source, .. } | DragSession::Dragging { source, .. } => {
                Some(source)
            }
        }
    }

    /// The pane the drag was lifted from, if it was lifted from one.
    pub fn source_pane(&self) -> Option<PaneId> {
        match self.source()?.origin {
            DragOrigin::Pane(pane) => Some(pane),
            DragOrigin::Library(_) => None,
        }
    }

    pub fn target(&self) -> Option<DropTarget> {
        match self {
            DragSession::Dragging { target, .. } => *target,
            _ => None,
        }
    }

    /// A press on a source. Only from `Idle`: a second press while a drag is
    /// live is not a second drag.
    pub fn arm(&mut self, source: DocumentDragSource, at: (f64, f64)) -> bool {
        if self.is_live() {
            return false;
        }
        *self = DragSession::Arming { source, origin: at };
        true
    }

    /// A drag that starts already dragging: a keyboard placement, or the
    /// Shell relaying a drag it took over. Only from `Idle`.
    pub fn start(&mut self, source: DocumentDragSource, geometry: DropGeometry) -> bool {
        if self.is_live() {
            return false;
        }
        *self = DragSession::Dragging {
            source,
            geometry,
            target: None,
            over_workspace: false,
        };
        true
    }

    /// The pointer moved to `at` (client coordinates). While arming, nothing
    /// happens until the pointer is farther than [`DRAG_THRESHOLD_PX`] from
    /// the press; crossing it measures the geometry (`measure`, called at
    /// most once per drag — a `None` means there is no workspace to drop
    /// on, and the press is let go). Returns whether the shown target (or
    /// the phase) changed, so the caller writes only on a change.
    pub fn moved(
        &mut self,
        at: (f64, f64),
        measure: impl FnOnce() -> Option<DropGeometry>,
    ) -> bool {
        if let DragSession::Arming { source, origin } = &*self {
            let (dx, dy) = (at.0 - origin.0, at.1 - origin.1);
            // The boundary is inside, like the draggable item's radius.
            if dx * dx + dy * dy <= DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX {
                return false;
            }
            let source = source.clone();
            let Some(geometry) = measure() else {
                *self = DragSession::Idle;
                return true;
            };
            *self = DragSession::Dragging {
                source,
                geometry,
                target: None,
                over_workspace: false,
            };
            self.point(at);
            return true;
        }
        self.point(at)
    }

    /// Re-choose the target for a pointer at `at`. Returns whether anything
    /// shown changed.
    fn point(&mut self, at: (f64, f64)) -> bool {
        let DragSession::Dragging {
            geometry,
            target,
            over_workspace,
            ..
        } = self
        else {
            return false;
        };
        let next = geometry.choose(at, *target);
        let inside = geometry.to_slot(at).is_some();
        let changed = next != *target || inside != *over_workspace;
        *target = next;
        *over_workspace = inside;
        changed
    }

    /// Select `target` directly (the keyboard's placement). Refused unless
    /// the drag's geometry offers it.
    pub fn select(&mut self, next: DropTarget) -> bool {
        let DragSession::Dragging {
            geometry, target, ..
        } = self
        else {
            return false;
        };
        if !geometry.offers(next) {
            return false;
        }
        let changed = *target != Some(next);
        *target = Some(next);
        changed
    }

    /// The keyboard's step: the split on `edge` of the pane the current
    /// target is measured against (the source pane before any target).
    pub fn select_edge(&mut self, edge: Edge) -> bool {
        let pane = self.target().map(DropTarget::pane).or(self.source_pane());
        match pane {
            Some(pane) => self.select(DropTarget::Split { pane, edge }),
            None => false,
        }
    }

    /// The pointer was released (at `at`, when the release has a position).
    /// The session is over either way; the intent is the drop to carry out,
    /// `None` for a click (still arming) or a release over no target.
    pub fn release(&mut self, at: Option<(f64, f64)>) -> Option<DropIntent> {
        if let Some(at) = at {
            self.point(at);
        }
        match std::mem::take(self) {
            DragSession::Dragging {
                source,
                target: Some(target),
                ..
            } => Some(DropIntent { source, target }),
            _ => None,
        }
    }

    /// End the session with no drop. Returns whether one was live.
    pub fn cancel(&mut self) -> bool {
        let was = self.is_live();
        *self = DragSession::Idle;
        was
    }

    /// What the preview draws: the target, the box the dropped document
    /// will occupy (slot coordinates), and the operation in words.
    pub fn preview(&self) -> Option<Preview> {
        let DragSession::Dragging {
            geometry,
            target: Some(target),
            ..
        } = self
        else {
            return None;
        };
        let pane = geometry.pane(target.pane())?;
        Some(Preview {
            target: *target,
            rect: target.predicted_rect(pane.rect),
            label: target.describe(pane.format),
        })
    }
}

/// The preview of the pending drop.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub target: DropTarget,
    pub rect: PaneBounds,
    pub label: String,
}

/// The source label a pane's document is dragged under.
pub fn pane_source_label(format: PaneFormat) -> String {
    format_label(format).to_string()
}

#[cfg(test)]
mod tests {
    use super::super::geometry::PaneGeometry;
    use super::*;

    fn p(n: u64) -> PaneId {
        PaneId::for_tests(n)
    }

    fn source() -> DocumentDragSource {
        DocumentDragSource {
            document: DocumentId::from_launch(None, "/samples/Split Notes.md"),
            origin: DragOrigin::Pane(p(1)),
            format: PaneFormat::Markdown,
            label: "Markdown".to_string(),
        }
    }

    fn geometry() -> DropGeometry {
        DropGeometry {
            workspace: PaneBounds {
                x: 0.0,
                y: 40.0,
                width: 1000.0,
                height: 800.0,
            },
            panes: vec![PaneGeometry {
                pane: p(1),
                rect: PaneBounds {
                    x: 0.0,
                    y: 0.0,
                    width: 1000.0,
                    height: 800.0,
                },
                format: PaneFormat::Markdown,
                empty: false,
            }],
            can_add: true,
        }
    }

    #[test]
    fn a_press_that_stays_inside_the_threshold_is_a_click() {
        let mut session = DragSession::default();
        assert!(session.arm(source(), (500.0, 400.0)));
        let mut measured = 0;
        // Exactly on the radius is still inside.
        assert!(!session.moved((500.0 + DRAG_THRESHOLD_PX, 400.0), || {
            measured += 1;
            Some(geometry())
        }));
        assert_eq!(measured, 0, "arming measures nothing");
        assert_eq!(session.phase(), "arming");
        assert_eq!(session.release(Some((502.0, 401.0))), None);
        assert_eq!(session, DragSession::Idle);
    }

    #[test]
    fn crossing_the_threshold_measures_once_and_picks_a_target() {
        let mut session = DragSession::default();
        session.arm(source(), (500.0, 440.0));
        let mut measured = 0;
        assert!(session.moved((900.0, 440.0), || {
            measured += 1;
            Some(geometry())
        }));
        assert_eq!(measured, 1);
        assert_eq!(session.phase(), "overTarget");
        assert_eq!(
            session.target(),
            Some(DropTarget::Split {
                pane: p(1),
                edge: Edge::Right
            })
        );
        // Later moves never measure again.
        session.moved((500.0, 820.0), || panic!("measured twice"));
        assert_eq!(
            session.target(),
            Some(DropTarget::Split {
                pane: p(1),
                edge: Edge::Bottom
            })
        );
    }

    #[test]
    fn a_move_that_changes_nothing_reports_nothing() {
        let mut session = DragSession::default();
        session.start(source(), geometry());
        assert!(session.moved((950.0, 440.0), || None));
        assert!(!session.moved((951.0, 441.0), || None));
    }

    #[test]
    fn leaving_the_workspace_clears_the_target_and_a_release_there_drops_nothing() {
        let mut session = DragSession::default();
        session.start(source(), geometry());
        session.moved((950.0, 440.0), || None);
        assert!(session.target().is_some());
        assert!(session.moved((950.0, 10.0), || None));
        assert_eq!(session.phase(), "dragging");
        assert_eq!(session.target(), None);
        assert_eq!(session.release(Some((950.0, 10.0))), None);
        assert!(!session.is_live());
    }

    #[test]
    fn a_release_over_a_target_is_the_one_intent() {
        let mut session = DragSession::default();
        session.start(source(), geometry());
        session.moved((500.0, 60.0), || None);
        let intent = session.release(Some((500.0, 60.0))).expect("a drop");
        assert_eq!(intent.source, source());
        assert_eq!(
            intent.target,
            DropTarget::Split {
                pane: p(1),
                edge: Edge::Top
            }
        );
        assert_eq!(session, DragSession::Idle);
    }

    #[test]
    fn cancel_ends_every_phase_with_no_intent() {
        let mut arming = DragSession::default();
        arming.arm(source(), (0.0, 0.0));
        assert!(arming.cancel());
        assert_eq!(arming, DragSession::Idle);

        let mut dragging = DragSession::default();
        dragging.start(source(), geometry());
        dragging.moved((950.0, 440.0), || None);
        assert!(dragging.cancel());
        assert_eq!(dragging, DragSession::Idle);
        // Nothing to release after a cancel.
        assert_eq!(dragging.release(Some((950.0, 440.0))), None);
        assert!(!dragging.cancel());
    }

    #[test]
    fn a_second_press_during_a_drag_is_not_a_second_drag() {
        let mut session = DragSession::default();
        session.start(source(), geometry());
        assert!(!session.arm(source(), (1.0, 1.0)));
        assert!(!session.start(source(), geometry()));
        assert!(session.is_dragging());
    }

    #[test]
    fn no_workspace_to_measure_lets_the_press_go() {
        let mut session = DragSession::default();
        session.arm(source(), (0.0, 0.0));
        assert!(session.moved((50.0, 0.0), || None));
        assert_eq!(session, DragSession::Idle);
    }

    #[test]
    fn the_keyboard_selects_only_offered_edges() {
        let mut session = DragSession::default();
        session.start(source(), geometry());
        assert!(session.select_edge(Edge::Left));
        assert_eq!(
            session.target(),
            Some(DropTarget::Split {
                pane: p(1),
                edge: Edge::Left
            })
        );
        // A pane that is not in the geometry is not a target.
        assert!(!session.select(DropTarget::Split {
            pane: p(7),
            edge: Edge::Left
        }));
        let intent = session.release(None).expect("Enter drops the selection");
        assert_eq!(
            intent.target,
            DropTarget::Split {
                pane: p(1),
                edge: Edge::Left
            }
        );
    }

    #[test]
    fn the_preview_is_the_predicted_box_and_its_words() {
        let mut session = DragSession::default();
        session.start(source(), geometry());
        session.moved((950.0, 440.0), || None);
        let preview = session.preview().expect("a target is shown");
        assert_eq!(
            preview.rect,
            PaneBounds {
                x: 500.0,
                y: 0.0,
                width: 500.0,
                height: 800.0
            }
        );
        assert_eq!(preview.label, "Drop to split right of Markdown");
        session.cancel();
        assert_eq!(session.preview(), None);
    }
}
