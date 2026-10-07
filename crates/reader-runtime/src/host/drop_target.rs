//! Drop targets: what a document dropped on the workspace may do.

use serde::Serialize;

use super::model::{PaneBounds, PaneFormat, PaneId};
use super::tree::{EVEN, Side, SplitAxis, split_rects};

/// One edge of a pane: where the new pane goes relative to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// Every edge, in the tie-break priority a score draw resolves by.
    pub const PRIORITY: [Edge; 4] = [Edge::Right, Edge::Bottom, Edge::Left, Edge::Top];

    /// The split this edge asks the tree for.
    pub fn placement(self) -> (SplitAxis, Side) {
        match self {
            Edge::Left => (SplitAxis::Horizontal, Side::Before),
            Edge::Right => (SplitAxis::Horizontal, Side::After),
            Edge::Top => (SplitAxis::Vertical, Side::Before),
            Edge::Bottom => (SplitAxis::Vertical, Side::After),
        }
    }

    /// The edge's name for DOM attributes and diagnostics.
    pub fn word(self) -> &'static str {
        match self {
            Edge::Left => "left",
            Edge::Right => "right",
            Edge::Top => "top",
            Edge::Bottom => "bottom",
        }
    }

    fn phrase(self) -> &'static str {
        match self {
            Edge::Left => "left of",
            Edge::Right => "right of",
            Edge::Top => "above",
            Edge::Bottom => "below",
        }
    }
}

/// What a drop does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget {
    /// A new pane on `edge` of `pane`, halving `pane`'s box.
    Split { pane: PaneId, edge: Edge },
    /// Into `pane` itself — offered only while `pane` holds no document.
    Here { pane: PaneId },
}

impl DropTarget {
    /// The pane the target is measured against.
    pub fn pane(self) -> PaneId {
        match self {
            DropTarget::Split { pane, .. } | DropTarget::Here { pane } => pane,
        }
    }

    /// The target's name for DOM attributes and diagnostics.
    pub fn word(self) -> &'static str {
        match self {
            DropTarget::Split { edge, .. } => edge.word(),
            DropTarget::Here { .. } => "here",
        }
    }

    /// The box the drop produces, by the tree's own rounding.
    pub fn predicted_rect(self, rect: PaneBounds) -> PaneBounds {
        match self {
            DropTarget::Here { .. } => rect,
            DropTarget::Split { edge, .. } => {
                let (axis, side) = edge.placement();
                let (first, second) = split_rects(rect, axis, EVEN);
                match side {
                    Side::Before => first,
                    Side::After => second,
                }
            }
        }
    }

    /// The pending operation in words.
    pub fn describe(self, format: PaneFormat) -> String {
        match self {
            DropTarget::Split { edge, .. } => {
                format!("Drop to split {} {}", edge.phrase(), format_label(format))
            }
            DropTarget::Here { .. } => "Drop to open here".to_string(),
        }
    }
}

/// A format's name as the workspace says it to the user.
fn format_label(format: PaneFormat) -> &'static str {
    match format {
        PaneFormat::Pdf => "PDF",
        PaneFormat::Markdown => "Markdown",
        PaneFormat::Text => "Text",
        PaneFormat::Pending => "the empty pane",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(n: u64) -> PaneId {
        PaneId::for_tests(n)
    }

    fn rect(x: f64, y: f64, width: f64, height: f64) -> PaneBounds {
        PaneBounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn every_edge_names_the_split_the_tree_performs() {
        assert_eq!(
            Edge::Left.placement(),
            (SplitAxis::Horizontal, Side::Before)
        );
        assert_eq!(
            Edge::Right.placement(),
            (SplitAxis::Horizontal, Side::After)
        );
        assert_eq!(Edge::Top.placement(), (SplitAxis::Vertical, Side::Before));
        assert_eq!(Edge::Bottom.placement(), (SplitAxis::Vertical, Side::After));
    }

    #[test]
    fn the_predicted_box_is_the_half_the_new_pane_gets() {
        let pane = rect(100.0, 50.0, 801.0, 600.0);
        let split = |edge| DropTarget::Split { pane: p(1), edge };
        // The layout rounds the FIRST half: 400.5 → 401, so the second
        // takes 400.
        assert_eq!(
            split(Edge::Left).predicted_rect(pane),
            rect(100.0, 50.0, 401.0, 600.0)
        );
        assert_eq!(
            split(Edge::Right).predicted_rect(pane),
            rect(501.0, 50.0, 400.0, 600.0)
        );
        assert_eq!(
            split(Edge::Top).predicted_rect(pane),
            rect(100.0, 50.0, 801.0, 300.0)
        );
        assert_eq!(
            split(Edge::Bottom).predicted_rect(pane),
            rect(100.0, 350.0, 801.0, 300.0)
        );
        assert_eq!(DropTarget::Here { pane: p(1) }.predicted_rect(pane), pane);
    }

    #[test]
    fn the_label_names_the_operation_and_the_pane_not_a_colour() {
        let right = DropTarget::Split {
            pane: p(1),
            edge: Edge::Right,
        };
        assert_eq!(
            right.describe(PaneFormat::Pdf),
            "Drop to split right of PDF"
        );
        let below = DropTarget::Split {
            pane: p(2),
            edge: Edge::Bottom,
        };
        assert_eq!(
            below.describe(PaneFormat::Markdown),
            "Drop to split below Markdown"
        );
        assert_eq!(
            DropTarget::Here { pane: p(3) }.describe(PaneFormat::Pending),
            "Drop to open here"
        );
    }
}
