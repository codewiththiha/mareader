//! Workspace commands: the one door a drop goes through.
//!
//! A pointer handler never mutates the tree. It produces a
//! [`super::drag::DropIntent`]; the host turns it into a
//! [`WorkspaceCommand`] and runs it (`ReaderHost::run`), and the command is
//! validated HERE, against the workspace as it is at the drop, before
//! anything changes: the geometry a drag measured may be stale by then (a
//! pane closed, the window resized), and a refused command leaves the tree
//! exactly as it was. What survives validation is a [`DropPlan`] the host
//! executes through its one placement path (`open_document`).

use super::drag::DocumentDragSource;
use super::drop_target::DropTarget;
use super::model::{MAX_PANES, PaneError, PaneId};
use super::tree::{PaneTree, Side, SplitAxis, TreeError, TreeLayout, split_fits};

/// A typed workspace command.
#[derive(Clone, Debug, PartialEq)]
pub enum WorkspaceCommand {
    /// Open the dragged document where the drop landed: a new pane beside a
    /// pane, or the empty pane itself.
    OpenInDropTarget {
        source: DocumentDragSource,
        target: DropTarget,
    },
}

/// A validated drop, in the host's placement vocabulary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropPlan {
    /// A new pane on `side` of `of` along `axis`.
    Split {
        of: PaneId,
        axis: SplitAxis,
        side: Side,
    },
    /// Into the empty pane `pane`, in place.
    Here { pane: PaneId },
}

/// The layout policy's room check for splitting `pane` along `axis`, against
/// the layout as laid out now. A pane the layout has not measured yet (no
/// box, or a zero box before the slot's first measurement) is not refused:
/// there is nothing to judge it by, and the tree's ratio clamp still holds.
pub fn check_room(layout: &TreeLayout, pane: PaneId, axis: SplitAxis) -> Result<(), PaneError> {
    match layout.bounds_of(pane) {
        Some(bounds) if bounds.width > 0.0 && bounds.height > 0.0 => {
            if split_fits(bounds, axis) {
                Ok(())
            } else {
                Err(PaneError::Layout(TreeError::NoRoom(pane)))
            }
        }
        _ => Ok(()),
    }
}

/// Validate `target` against the workspace at the drop. `live` is the count
/// of placed panes; `is_empty` answers whether a pane holds no document.
pub fn plan(
    target: DropTarget,
    tree: &PaneTree,
    layout: &TreeLayout,
    live: usize,
    is_empty: impl Fn(PaneId) -> bool,
) -> Result<DropPlan, PaneError> {
    let pane = target.pane();
    if !tree.contains(pane) {
        return Err(PaneError::Layout(TreeError::UnknownPane(pane)));
    }
    match target {
        DropTarget::Here { pane } => {
            // In place is only ever an EMPTY pane: a drop never replaces a
            // document the user is reading.
            if !is_empty(pane) {
                return Err(PaneError::Layout(TreeError::Occupied(pane)));
            }
            Ok(DropPlan::Here { pane })
        }
        DropTarget::Split { pane, edge } => {
            if live >= MAX_PANES {
                return Err(PaneError::WorkspaceFull);
            }
            let (axis, side) = edge.placement();
            check_room(layout, pane, axis)?;
            Ok(DropPlan::Split {
                of: pane,
                axis,
                side,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::drop_target::Edge;
    use super::super::model::PaneBounds;
    use super::super::tree::LayoutNode;
    use super::*;

    fn p(n: u64) -> PaneId {
        PaneId::for_tests(n)
    }

    fn rect(width: f64, height: f64) -> PaneBounds {
        PaneBounds {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    }

    fn split(n: u64, edge: Edge) -> DropTarget {
        DropTarget::Split { pane: p(n), edge }
    }

    /// Carry out a plan on a tree the way the host does: the new pane is
    /// placed by the tree's one split operation.
    fn apply(tree: &mut PaneTree, plan: DropPlan, new: PaneId) {
        match plan {
            DropPlan::Split { of, axis, side } => {
                tree.split(of, axis, side, new)
                    .expect("a validated plan splits");
            }
            DropPlan::Here { .. } => {}
        }
    }

    /// The tree's shape with split ids and ratios dropped.
    fn shape(node: &LayoutNode) -> String {
        match node {
            LayoutNode::Leaf(id) => id.get().to_string(),
            LayoutNode::Split(split) => {
                let axis = match split.axis {
                    SplitAxis::Horizontal => "H",
                    SplitAxis::Vertical => "V",
                };
                format!("{axis}({},{})", shape(&split.first), shape(&split.second))
            }
        }
    }

    fn drop_on(tree: &mut PaneTree, target: DropTarget, new: u64) {
        let layout = tree.layout(rect(1600.0, 1200.0));
        let plan = plan(target, tree, &layout, tree.len(), |_| false).expect("valid");
        apply(tree, plan, p(new));
        tree.check_invariants(&tree.leaves(), Some(p(new)))
            .expect("the tree stays sound");
    }

    #[test]
    fn every_orientation_on_a_single_pane() {
        for (edge, expected) in [
            (Edge::Right, "H(1,2)"),
            (Edge::Left, "H(2,1)"),
            (Edge::Bottom, "V(1,2)"),
            (Edge::Top, "V(2,1)"),
        ] {
            let mut tree = PaneTree::new();
            tree.set_root(p(1)).unwrap();
            drop_on(&mut tree, split(1, edge), 2);
            assert_eq!(shape(tree.root().unwrap()), expected, "{edge:?}");
            // The pane that was there stays where it was: only the new one
            // is placed beside it.
            assert!(tree.contains(p(1)));
        }
    }

    #[test]
    fn every_orientation_inside_a_nested_tree() {
        // V(1,2) — pane 2 on the bottom — then each edge of pane 2.
        for (edge, expected) in [
            (Edge::Top, "V(1,V(3,2))"),
            (Edge::Bottom, "V(1,V(2,3))"),
            (Edge::Left, "V(1,H(3,2))"),
            (Edge::Right, "V(1,H(2,3))"),
        ] {
            let mut tree = PaneTree::new();
            tree.set_root(p(1)).unwrap();
            tree.split(p(1), SplitAxis::Vertical, Side::After, p(2))
                .unwrap();
            drop_on(&mut tree, split(2, edge), 3);
            assert_eq!(shape(tree.root().unwrap()), expected, "{edge:?}");
        }
    }

    #[test]
    fn a_second_level_of_nesting_is_split_in_place_never_flattened() {
        // H(1,V(2,3)); drop on 3's left → H(1,V(2,H(4,3))).
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        tree.split(p(2), SplitAxis::Vertical, Side::After, p(3))
            .unwrap();
        drop_on(&mut tree, split(3, Edge::Left), 4);
        assert_eq!(shape(tree.root().unwrap()), "H(1,V(2,H(4,3)))");
        // A fresh split starts even, stored in the tree.
        let layout = tree.layout(rect(1600.0, 1200.0));
        assert_eq!(layout.bounds_of(p(4)).unwrap().width, 400.0);
        assert_eq!(layout.bounds_of(p(3)).unwrap().width, 400.0);
    }

    #[test]
    fn a_target_pane_not_in_the_tree_is_refused_and_nothing_changes() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let before = tree.clone();
        let layout = tree.layout(rect(1000.0, 800.0));
        assert_eq!(
            plan(split(9, Edge::Right), &tree, &layout, 1, |_| false),
            Err(PaneError::Layout(TreeError::UnknownPane(p(9))))
        );
        assert_eq!(tree, before);
    }

    #[test]
    fn a_split_with_no_room_at_the_drop_is_refused() {
        // The window shrank after the drag measured: 390 wide cannot halve.
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let before = tree.clone();
        let layout = tree.layout(rect(390.0, 800.0));
        assert_eq!(
            plan(split(1, Edge::Left), &tree, &layout, 1, |_| false),
            Err(PaneError::Layout(TreeError::NoRoom(p(1))))
        );
        // Stacking still fits.
        assert!(plan(split(1, Edge::Top), &tree, &layout, 1, |_| false).is_ok());
        assert_eq!(tree, before);
    }

    #[test]
    fn a_full_workspace_refuses_a_split() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let layout = tree.layout(rect(1600.0, 1200.0));
        assert_eq!(
            plan(split(1, Edge::Right), &tree, &layout, MAX_PANES, |_| false),
            Err(PaneError::WorkspaceFull)
        );
    }

    #[test]
    fn in_place_is_only_for_an_empty_pane() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let layout = tree.layout(rect(1000.0, 800.0));
        let here = DropTarget::Here { pane: p(1) };
        assert_eq!(
            plan(here, &tree, &layout, 1, |_| true),
            Ok(DropPlan::Here { pane: p(1) })
        );
        assert_eq!(
            plan(here, &tree, &layout, 1, |_| false),
            Err(PaneError::Layout(TreeError::Occupied(p(1))))
        );
    }

    #[test]
    fn an_unmeasured_pane_is_not_refused_for_room() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let layout = tree.layout(rect(0.0, 0.0));
        assert_eq!(check_room(&layout, p(1), SplitAxis::Horizontal), Ok(()));
    }
}
