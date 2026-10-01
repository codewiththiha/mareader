//! Lifting a pane: a press-and-hold on a pane's empty space picks the pane
//! up, and dropping it on another pane docks it beside that pane's nearest
//! edge. While it is held the workspace is laid out as if it were closed
//! (`ReaderHost::layout`), so its neighbours fill its space and every target
//! previews exactly the box the drop will give it.
//!
//! Pure data, like the document drag: the lifted pane, where the pointer is
//! in slot coordinates, and the target that pointer resolves to. No DOM, no
//! runtime. A drop is carried out by the host's layout-only commands
//! (`ReaderHost::swap_panes` / `ReaderHost::dock_pane`), so a pane never
//! loses its session by being moved.

use super::drop_target::Edge;
use super::model::{PaneBounds, PaneId};
use super::tree::{EVEN, Side, TreeLayout, split_fits, split_rects};

/// How long a still press on a pane's empty space must hold before the pane
/// lifts. Long enough that a reader resting the pointer while panning never
/// lifts a pane by accident, short enough to feel deliberate rather than
/// stuck; the hold ring shows it filling.
pub const HOLD_TO_LIFT_MS: u64 = 1200;

/// Where a lifted pane would go if released now.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiftTarget {
    /// Trade places with `pane`.
    Swap(PaneId),
    /// Dock on `edge` of `pane`, halving its box.
    Dock(PaneId, Edge),
}

impl LiftTarget {
    pub fn pane(self) -> PaneId {
        match self {
            LiftTarget::Swap(pane) | LiftTarget::Dock(pane, _) => pane,
        }
    }

    /// The box the lifted pane would occupy, given the target's box.
    pub fn predicted_rect(self, rect: PaneBounds) -> PaneBounds {
        match self {
            LiftTarget::Swap(_) => rect,
            LiftTarget::Dock(_, edge) => {
                let (axis, side) = edge.placement();
                let (first, second) = split_rects(rect, axis, EVEN);
                match side {
                    Side::Before => first,
                    Side::After => second,
                }
            }
        }
    }

    /// The pending relocation in words, for the preview and the live region.
    pub fn describe(self) -> &'static str {
        match self {
            LiftTarget::Swap(_) => "Release to swap panes",
            LiftTarget::Dock(_, Edge::Left) => "Release to move left of this pane",
            LiftTarget::Dock(_, Edge::Right) => "Release to move right of this pane",
            LiftTarget::Dock(_, Edge::Top) => "Release to move above this pane",
            LiftTarget::Dock(_, Edge::Bottom) => "Release to move below this pane",
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            LiftTarget::Swap(_) => "swap",
            LiftTarget::Dock(_, edge) => edge.word(),
        }
    }
}

/// A lifted pane.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lift {
    pub pane: PaneId,
    /// The pointer where the pane lifted, in slot coordinates: the lifted
    /// card is drawn relative to it.
    pub origin: (f64, f64),
    /// The pointer now, in slot coordinates.
    pub at: (f64, f64),
    pub target: Option<LiftTarget>,
}

impl Lift {
    pub fn new(pane: PaneId, at: (f64, f64)) -> Self {
        Self {
            pane,
            origin: at,
            at,
            target: None,
        }
    }

    /// Follow the pointer to `at` and re-resolve the target against
    /// `layout`.
    pub fn moved(&mut self, at: (f64, f64), layout: &TreeLayout) {
        self.at = at;
        self.target = resolve(self.pane, at, layout);
    }
}

/// The target under slot point `at` for lifted pane `lifted`, resolved
/// against the layout WITHOUT it: nothing outside every pane; otherwise a
/// dock beside the target's nearest edge that it has room to be halved at.
/// A pane too small to halve either way is traded places with instead.
pub fn resolve(lifted: PaneId, at: (f64, f64), layout: &TreeLayout) -> Option<LiftTarget> {
    let (pane, rect) = layout
        .panes
        .iter()
        .find(|(_, rect)| contains(*rect, at))
        .copied()?;
    if pane == lifted {
        return None;
    }
    let fx = (at.0 - rect.x) / rect.width.max(1.0);
    let fy = (at.1 - rect.y) / rect.height.max(1.0);
    let edges = [
        (Edge::Left, fx),
        (Edge::Right, 1.0 - fx),
        (Edge::Top, fy),
        (Edge::Bottom, 1.0 - fy),
    ];
    let nearest = edges
        .into_iter()
        .filter(|(edge, _)| split_fits(rect, edge.placement().0))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    Some(match nearest {
        Some((edge, _)) => LiftTarget::Dock(pane, edge),
        None => LiftTarget::Swap(pane),
    })
}

fn contains(rect: PaneBounds, at: (f64, f64)) -> bool {
    at.0 >= rect.x && at.0 < rect.x + rect.width && at.1 >= rect.y && at.1 < rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::super::tree::{PaneTree, SplitAxis};
    use super::*;

    fn p(n: u64) -> PaneId {
        PaneId::for_tests(n)
    }

    fn pair() -> TreeLayout {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        tree.layout(PaneBounds::filling(1200.0, 800.0))
    }

    #[test]
    fn over_the_lifted_pane_there_is_no_target() {
        assert_eq!(resolve(p(1), (300.0, 400.0), &pair()), None);
        assert_eq!(resolve(p(1), (5000.0, 400.0), &pair()), None);
    }

    #[test]
    fn a_pane_docks_beside_its_nearest_edge() {
        let layout = pair();
        assert_eq!(
            resolve(p(1), (900.0, 20.0), &layout),
            Some(LiftTarget::Dock(p(2), Edge::Top))
        );
        assert_eq!(
            resolve(p(1), (1000.0, 400.0), &layout),
            Some(LiftTarget::Dock(p(2), Edge::Right))
        );
        assert_eq!(
            resolve(p(1), (650.0, 500.0), &layout),
            Some(LiftTarget::Dock(p(2), Edge::Left))
        );
    }

    #[test]
    fn a_pane_too_narrow_to_halve_docks_on_the_other_axis() {
        // 350 px wide cannot be halved side by side; 800 px tall can.
        let mut narrow = pair();
        narrow.panes[1].1.width = 350.0;
        narrow.panes[1].1.x = 850.0;
        assert_eq!(
            resolve(p(1), (1190.0, 700.0), &narrow),
            Some(LiftTarget::Dock(p(2), Edge::Bottom))
        );
    }

    #[test]
    fn a_pane_too_small_to_halve_swaps() {
        let mut tiny = pair();
        tiny.panes[1].1 = PaneBounds {
            x: 850.0,
            y: 0.0,
            width: 350.0,
            height: 300.0,
        };
        assert_eq!(
            resolve(p(1), (1000.0, 100.0), &tiny),
            Some(LiftTarget::Swap(p(2)))
        );
    }

    #[test]
    fn a_dock_predicts_half_the_target() {
        let rect = PaneBounds::filling(600.0, 800.0);
        let half = LiftTarget::Dock(p(2), Edge::Bottom).predicted_rect(rect);
        assert_eq!((half.y, half.height), (400.0, 400.0));
        assert_eq!(LiftTarget::Swap(p(2)).predicted_rect(rect), rect);
    }
}
