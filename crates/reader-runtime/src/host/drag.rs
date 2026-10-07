//! The document drag session: typed data only, no DOM, no pane runtime.

use super::drop_target::DropTarget;
use super::geometry::DropGeometry;
use super::model::{DocumentId, PaneBounds, PaneFormat};

/// The repository's one drag threshold; a shorter move stays a click.
pub use app_ui::components::primitives::interactions::draggable_item::DRAG_THRESHOLD_PX;

/// The dragged document: an address the persisted library names.
#[derive(Clone, Debug, PartialEq)]
pub struct DocumentDragSource {
    pub document: Option<DocumentId>,
    /// The library row the drag started on.
    pub book_id: Option<String>,
    pub path: String,
    pub format: PaneFormat,
    /// What the user sees the document called (the row's title).
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
    /// A drag: the geometry measured at the start, and the chosen target.
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

    pub fn target(&self) -> Option<DropTarget> {
        match self {
            DragSession::Dragging { target, .. } => *target,
            _ => None,
        }
    }

    /// A press on a source. Only from `Idle`: no second drag while live.
    pub fn arm(&mut self, source: DocumentDragSource, at: (f64, f64)) -> bool {
        if self.is_live() {
            return false;
        }
        *self = DragSession::Arming { source, origin: at };
        true
    }

    /// A drag that starts already dragging, over a measured workspace. Only
    /// from `Idle`.
    #[cfg(test)]
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

    /// The pointer moved to `at`; crossing the threshold measures the
    /// geometry. Returns whether anything changed.
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

    /// Release: the drop to carry out, `None` for a click or no target.
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

    /// What the preview draws: the target, the box the drop will take.
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
            name: self.source()?.label.clone(),
            label: target.describe(pane.format),
        })
    }
}

/// The preview of the pending drop.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub target: DropTarget,
    pub rect: PaneBounds,
    /// The dragged document's name.
    pub name: String,
    /// The operation, in words.
    pub label: String,
}

#[cfg(test)]
mod tests {
    use super::super::drop_target::Edge;
    use super::super::geometry::PaneGeometry;
    use super::super::model::PaneId;
    use super::*;

    fn p(n: u64) -> PaneId {
        PaneId::for_tests(n)
    }

    fn source() -> DocumentDragSource {
        DocumentDragSource {
            document: DocumentId::from_launch(None, "/samples/Split Notes.md"),
            book_id: None,
            path: "/samples/Split Notes.md".to_string(),
            format: PaneFormat::Markdown,
            label: "Split Notes".to_string(),
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
        assert_eq!(preview.name, "Split Notes");
        assert_eq!(preview.label, "Drop to split right of Markdown");
        session.cancel();
        assert_eq!(session.preview(), None);
    }
}
