//! The workspace layout, pure: a binary tree of splits whose leaves are
//! [`PaneId`]s. No Leptos, no DOM, no engine — and nothing heavier than an
//! id and a ratio: the tree never holds a session, a canvas, a virtualizer
//! or a document buffer, so reshaping the workspace can never retain one.
//! The runtimes live in the pane manager, keyed by the same ids.
//!
//! What the tree decides, and the host only carries out:
//!
//! * where a new pane goes ([`PaneTree::split`]: beside a pane, before or
//!   after it, along an axis);
//! * what closing a pane leaves ([`PaneTree::remove`]: its split collapses
//!   into the sibling — no unary split can exist, the type has none — and
//!   the pane that inherits focus is named, deterministically);
//! * how a divider drag becomes a ratio ([`PaneTree::drag_ratio`]: clamped
//!   so neither side becomes unusably small);
//! * every pane's box and every divider's hit strip for a workspace rect
//!   ([`PaneTree::layout`]), in whole pixels that tile the rect exactly.
//!
//! Ratios, not pixels, are the stored geometry: a window resize re-lays the
//! same tree. Pixels enter only when a drag is converted (the minimum pane
//! size depends on the room there is) and when the tree is laid out.

use serde::{Deserialize, Serialize};

use super::model::{PaneBounds, PaneId};

/// The smallest share either side of a split may have.
const MIN_RATIO: f64 = 0.15;
/// The largest share either side of a split may have.
const MAX_RATIO: f64 = 1.0 - MIN_RATIO;
/// The narrowest a pane may be dragged, when the split has room for two.
const MIN_PANE_PX: f64 = 200.0;
/// The width of a divider's pointer strip, centred on the seam. The strip
/// overlays the two panes' edges; the panes themselves tile the rect.
const DIVIDER_HIT_PX: f64 = 8.0;
/// The ratio a fresh split starts at.
pub const EVEN: f64 = 0.5;

/// How a split divides its box.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitAxis {
    /// Side by side: `first` is left, `second` is right (a vertical seam).
    Horizontal,
    /// Stacked: `first` is on top, `second` below (a horizontal seam).
    Vertical,
}

/// Where a new pane goes relative to the pane it splits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    /// Left of it, or above it.
    Before,
    /// Right of it, or below it.
    After,
}

/// Which way a pane is moved through the layout (the view menu's Move
/// items): toward that side of the workspace.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MoveDirection {
    Left,
    Right,
    Up,
    Down,
}

impl MoveDirection {
    /// Every direction, in the order the menu lists them.
    pub const ALL: [MoveDirection; 4] = [
        MoveDirection::Left,
        MoveDirection::Up,
        MoveDirection::Down,
        MoveDirection::Right,
    ];

    /// The split axis the move crosses, and the side of such a split the
    /// pane must START on for the move to go anywhere (`false`: `first`).
    fn crossing(self) -> (SplitAxis, bool) {
        match self {
            MoveDirection::Left => (SplitAxis::Horizontal, true),
            MoveDirection::Right => (SplitAxis::Horizontal, false),
            MoveDirection::Up => (SplitAxis::Vertical, true),
            MoveDirection::Down => (SplitAxis::Vertical, false),
        }
    }
}

/// Which moves the layout would carry out for one pane now.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub struct Moves {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
}

impl Moves {
    pub fn allows(self, direction: MoveDirection) -> bool {
        match direction {
            MoveDirection::Left => self.left,
            MoveDirection::Right => self.right,
            MoveDirection::Up => self.up,
            MoveDirection::Down => self.down,
        }
    }

    pub fn any(self) -> bool {
        self.left || self.right || self.up || self.down
    }
}

/// A split's identity, for the divider that resizes it. Minted by the tree
/// from a monotonic counter and never reused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize)]
#[serde(transparent)]
pub struct SplitId(u64);

impl SplitId {
    /// The raw number, for diagnostics and DOM attributes only.
    pub fn get(self) -> u64 {
        self.0
    }
}

/// One split: two children and the share of the box the first one gets.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct SplitNode {
    pub id: SplitId,
    pub axis: SplitAxis,
    /// `first`'s share, within `MIN_RATIO` and `MAX_RATIO`.
    pub ratio: f64,
    pub first: LayoutNode,
    pub second: LayoutNode,
}

/// A node of the layout: a pane, or a split of two nodes. There is no empty
/// node and no one-child split — the shapes the phase forbids cannot be
/// built.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutNode {
    Leaf(PaneId),
    Split(Box<SplitNode>),
}

impl LayoutNode {
    fn first_leaf(&self) -> PaneId {
        match self {
            LayoutNode::Leaf(id) => *id,
            LayoutNode::Split(split) => split.first.first_leaf(),
        }
    }

    fn last_leaf(&self) -> PaneId {
        match self {
            LayoutNode::Leaf(id) => *id,
            LayoutNode::Split(split) => split.second.last_leaf(),
        }
    }

    fn contains(&self, pane: PaneId) -> bool {
        match self {
            LayoutNode::Leaf(id) => *id == pane,
            LayoutNode::Split(split) => split.first.contains(pane) || split.second.contains(pane),
        }
    }

    fn collect_leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            LayoutNode::Leaf(id) => out.push(*id),
            LayoutNode::Split(split) => {
                split.first.collect_leaves(out);
                split.second.collect_leaves(out);
            }
        }
    }

    fn find_split(&self, id: SplitId) -> Option<&SplitNode> {
        match self {
            LayoutNode::Leaf(_) => None,
            LayoutNode::Split(split) if split.id == id => Some(split),
            LayoutNode::Split(split) => split
                .first
                .find_split(id)
                .or_else(|| split.second.find_split(id)),
        }
    }

    fn find_split_mut(&mut self, id: SplitId) -> Option<&mut SplitNode> {
        // One binding of the split: two match arms each binding it mutably
        // (one returning it) is a double borrow to the checker.
        let LayoutNode::Split(split) = self else {
            return None;
        };
        if split.id == id {
            return Some(&mut **split);
        }
        if split.first.find_split(id).is_some() {
            split.first.find_split_mut(id)
        } else {
            split.second.find_split_mut(id)
        }
    }

    /// Replace the leaf `target` with `with(target)`. `true` if it was found.
    fn replace_leaf(&mut self, target: PaneId, with: &mut Option<LayoutNode>) -> bool {
        match self {
            LayoutNode::Leaf(id) if *id == target => {
                if let Some(node) = with.take() {
                    *self = node;
                }
                true
            }
            LayoutNode::Leaf(_) => false,
            LayoutNode::Split(split) => {
                split.first.replace_leaf(target, with) || split.second.replace_leaf(target, with)
            }
        }
    }

    /// Remove the leaf `pane` from the subtree under this SPLIT node,
    /// collapsing its parent split into the sibling. Returns the pane that
    /// inherits focus, or `None` when `pane` is not under here.
    fn remove_under(&mut self, pane: PaneId) -> Option<PaneId> {
        let LayoutNode::Split(split) = self else {
            return None;
        };
        let (closed_first, sibling) = match (&split.first, &split.second) {
            (LayoutNode::Leaf(id), _) if *id == pane => (true, split.second.clone()),
            (_, LayoutNode::Leaf(id)) if *id == pane => (false, split.first.clone()),
            _ => {
                return split
                    .first
                    .remove_under(pane)
                    .or_else(|| split.second.remove_under(pane));
            }
        };
        // The pane nearest to where the closed one stood: the sibling's
        // edge that touched it. A closed left/top pane hands over to the
        // sibling's first leaf; a closed right/bottom pane to its last.
        let successor = if closed_first {
            sibling.first_leaf()
        } else {
            sibling.last_leaf()
        };
        *self = sibling;
        Some(successor)
    }

    /// The route from this node down to `pane`: each split passed, as its
    /// axis and whether the route takes its `second` child.
    fn route_to(&self, pane: PaneId) -> Option<Vec<(SplitAxis, bool)>> {
        match self {
            LayoutNode::Leaf(id) => (*id == pane).then(Vec::new),
            LayoutNode::Split(split) => {
                let (side, rest) = if let Some(rest) = split.first.route_to(pane) {
                    (false, rest)
                } else {
                    (true, split.second.route_to(pane)?)
                };
                let mut route = Vec::with_capacity(rest.len() + 1);
                route.push((split.axis, side));
                route.extend(rest);
                Some(route)
            }
        }
    }

    fn at(&self, sides: &[bool]) -> Option<&LayoutNode> {
        match (sides.split_first(), self) {
            (None, node) => Some(node),
            (Some((side, rest)), LayoutNode::Split(split)) => {
                let child = if *side { &split.second } else { &split.first };
                child.at(rest)
            }
            (Some(_), LayoutNode::Leaf(_)) => None,
        }
    }

    fn at_mut(&mut self, sides: &[bool]) -> Option<&mut LayoutNode> {
        match sides.split_first() {
            None => Some(self),
            Some((side, rest)) => match self {
                LayoutNode::Split(split) => {
                    if *side {
                        split.second.at_mut(rest)
                    } else {
                        split.first.at_mut(rest)
                    }
                }
                LayoutNode::Leaf(_) => None,
            },
        }
    }

    fn lay_out(&self, rect: PaneBounds, out: &mut TreeLayout) {
        match self {
            LayoutNode::Leaf(id) => out.panes.push((*id, rect)),
            LayoutNode::Split(split) => {
                let (first, second) = divide(rect, split.axis, split.ratio);
                let seam = match split.axis {
                    SplitAxis::Horizontal => PaneBounds {
                        x: first.x + first.width - DIVIDER_HIT_PX / 2.0,
                        y: rect.y,
                        width: DIVIDER_HIT_PX,
                        height: rect.height,
                    },
                    SplitAxis::Vertical => PaneBounds {
                        x: rect.x,
                        y: first.y + first.height - DIVIDER_HIT_PX / 2.0,
                        width: rect.width,
                        height: DIVIDER_HIT_PX,
                    },
                };
                out.dividers.push(DividerLayout {
                    split: split.id,
                    axis: split.axis,
                    hit: seam,
                    span: rect,
                });
                split.first.lay_out(first, out);
                split.second.lay_out(second, out);
            }
        }
    }
}

/// Split `rect` along `axis` at `ratio`, in whole pixels: the first side is
/// the rounded share, the second everything left, so the two tile the rect
/// with no gap and no overlap at any size. Public for the drop preview: the
/// box a preview draws is the box this lays out, never a second rounding.
pub fn split_rects(rect: PaneBounds, axis: SplitAxis, ratio: f64) -> (PaneBounds, PaneBounds) {
    divide(rect, axis, ratio)
}

/// The layout policy's answer to "may this pane be split along `axis`": a
/// fresh split starts [`EVEN`], and both halves must keep `MIN_PANE_PX`
/// along the axis. Measured in the boxes the layout itself would produce.
pub fn split_fits(rect: PaneBounds, axis: SplitAxis) -> bool {
    let (first, second) = divide(rect, axis, EVEN);
    let extent = |b: PaneBounds| match axis {
        SplitAxis::Horizontal => b.width,
        SplitAxis::Vertical => b.height,
    };
    extent(first) >= MIN_PANE_PX && extent(second) >= MIN_PANE_PX
}

fn divide(rect: PaneBounds, axis: SplitAxis, ratio: f64) -> (PaneBounds, PaneBounds) {
    match axis {
        SplitAxis::Horizontal => {
            let first_w = (rect.width * ratio).round().min(rect.width).max(0.0);
            (
                PaneBounds {
                    width: first_w,
                    ..rect
                },
                PaneBounds {
                    x: rect.x + first_w,
                    width: rect.width - first_w,
                    ..rect
                },
            )
        }
        SplitAxis::Vertical => {
            let first_h = (rect.height * ratio).round().min(rect.height).max(0.0);
            (
                PaneBounds {
                    height: first_h,
                    ..rect
                },
                PaneBounds {
                    y: rect.y + first_h,
                    height: rect.height - first_h,
                    ..rect
                },
            )
        }
    }
}

/// One divider as laid out: its pointer strip, and the box of the split it
/// resizes (a drag's pointer position is converted against `span`).
#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub struct DividerLayout {
    pub split: SplitId,
    pub axis: SplitAxis,
    pub hit: PaneBounds,
    pub span: PaneBounds,
}

/// The whole workspace laid out: every pane's box (in leaf order) and every
/// divider's strip (outermost first).
#[derive(Clone, PartialEq, Debug, Default, Serialize)]
pub struct TreeLayout {
    pub panes: Vec<(PaneId, PaneBounds)>,
    pub dividers: Vec<DividerLayout>,
}

impl TreeLayout {
    pub fn bounds_of(&self, pane: PaneId) -> Option<PaneBounds> {
        self.panes
            .iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, bounds)| *bounds)
    }
}

/// Why a tree operation was refused. A refused operation changes nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TreeError {
    /// `set_root` on a tree that already has panes.
    NotEmpty,
    /// The pane is not in the tree.
    UnknownPane(PaneId),
    /// The pane is already in the tree (a pane is in one place, once).
    DuplicatePane(PaneId),
    /// No split with this id.
    UnknownSplit(SplitId),
    /// A ratio that is not a finite number.
    InvalidRatio,
    /// The layout policy refused a split: one half of the pane would be
    /// narrower (or shorter) than `MIN_PANE_PX` ([`split_fits`]).
    NoRoom(PaneId),
    /// A drop asked to open in a pane that already shows a document (only
    /// an empty pane takes a document in place from a drop).
    Occupied(PaneId),
    /// Nothing lies that way from the pane (or a pane was asked to trade
    /// places with itself).
    NoMove(PaneId),
    /// A move between subtrees that are not disjoint.
    InvalidMove,
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::NotEmpty => write!(f, "the workspace already has a pane"),
            TreeError::UnknownPane(id) => write!(f, "{id} is not in the workspace"),
            TreeError::DuplicatePane(id) => write!(f, "{id} is already in the workspace"),
            TreeError::UnknownSplit(id) => write!(f, "split-{} does not exist", id.get()),
            TreeError::InvalidRatio => write!(f, "a split ratio must be a finite number"),
            TreeError::NoRoom(id) => write!(f, "{id} has no room for another pane"),
            TreeError::Occupied(id) => write!(f, "{id} already shows a document"),
            TreeError::NoMove(id) => write!(f, "{id} cannot move that way"),
            TreeError::InvalidMove => write!(f, "those panes cannot trade places"),
        }
    }
}

/// What one move exchanges (see `PaneTree::move_plan`).
struct MovePlan {
    /// The route to the crossed split.
    crossed: Vec<bool>,
    /// The route to the subtree that moves with the pane.
    mine: Vec<bool>,
    /// The route to the subtree it trades places with.
    other: Vec<bool>,
    /// Whether the two are the crossed split's own children.
    at_crossing: bool,
}

/// The workspace layout. Empty, one pane, or a tree of splits.
#[derive(Clone, PartialEq, Debug, Default, Serialize)]
pub struct PaneTree {
    root: Option<LayoutNode>,
    #[serde(skip)]
    next_split: u64,
}

impl PaneTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root(&self) -> Option<&LayoutNode> {
        self.root.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn contains(&self, pane: PaneId) -> bool {
        self.root.as_ref().is_some_and(|root| root.contains(pane))
    }

    /// Every pane, in reading order (left before right, top before bottom).
    pub fn leaves(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.collect_leaves(&mut out);
        }
        out
    }

    pub fn len(&self) -> usize {
        self.leaves().len()
    }

    /// Place the first pane: the whole workspace.
    pub fn set_root(&mut self, pane: PaneId) -> Result<(), TreeError> {
        if self.root.is_some() {
            return Err(TreeError::NotEmpty);
        }
        self.root = Some(LayoutNode::Leaf(pane));
        Ok(())
    }

    /// Put `new` beside `target`, on `side` of it along `axis`, halving
    /// `target`'s box. Returns the new split's id.
    pub fn split(
        &mut self,
        target: PaneId,
        axis: SplitAxis,
        side: Side,
        new: PaneId,
    ) -> Result<SplitId, TreeError> {
        if self.contains(new) {
            return Err(TreeError::DuplicatePane(new));
        }
        if !self.contains(target) {
            return Err(TreeError::UnknownPane(target));
        }
        self.next_split += 1;
        let id = SplitId(self.next_split);
        let (first, second) = match side {
            Side::Before => (new, target),
            Side::After => (target, new),
        };
        let mut with = Some(LayoutNode::Split(Box::new(SplitNode {
            id,
            axis,
            ratio: EVEN,
            first: LayoutNode::Leaf(first),
            second: LayoutNode::Leaf(second),
        })));
        let root = self.root.as_mut().expect("contains() found the target");
        let replaced = root.replace_leaf(target, &mut with);
        debug_assert!(replaced, "contains() found the target");
        Ok(id)
    }

    /// Take `pane` out. Its split collapses into the sibling subtree (the
    /// normalization: no split is ever left with one child). Returns the
    /// pane that should inherit focus if `pane` had it — the nearest one,
    /// the sibling's edge that touched the closed pane — or `None` when the
    /// workspace is now empty.
    pub fn remove(&mut self, pane: PaneId) -> Result<Option<PaneId>, TreeError> {
        if matches!(self.root, Some(LayoutNode::Leaf(id)) if id == pane) {
            self.root = None;
            return Ok(None);
        }
        self.root
            .as_mut()
            .and_then(|root| root.remove_under(pane))
            .map(Some)
            .ok_or(TreeError::UnknownPane(pane))
    }

    /// The two subtrees a move of `pane` toward `direction` exchanges, as
    /// routes of sides from the root, and whether the exchange happens at
    /// the crossed split itself (`true`) or deeper, between matching cells.
    ///
    /// The crossed split is the NEAREST ancestor along the move's axis that
    /// has `pane` on the side the move leaves. Its other side is matched
    /// against the route from that split down to `pane`, level by level,
    /// while the other side is split the same way: a 2×2 grid swaps one
    /// cell for the cell beside it, while a stacked pair moving past one
    /// tall pane moves as a column and the tall pane takes its place.
    fn move_plan(&self, pane: PaneId, direction: MoveDirection) -> Option<MovePlan> {
        let root = self.root.as_ref()?;
        let route = root.route_to(pane)?;
        let (axis, leaving) = direction.crossing();
        let crossed = route
            .iter()
            .rposition(|(step_axis, side)| *step_axis == axis && *side == leaving)?;
        let mut mine: Vec<bool> = route[..=crossed].iter().map(|(_, side)| *side).collect();
        let mut other = mine.clone();
        *other.last_mut().expect("the crossed split is on the route") = !leaving;
        let mut depth = 0;
        for (step_axis, side) in &route[crossed + 1..] {
            match root.at(&other) {
                Some(LayoutNode::Split(split)) if split.axis == *step_axis => {
                    mine.push(*side);
                    other.push(*side);
                    depth += 1;
                }
                _ => break,
            }
        }
        Some(MovePlan {
            crossed: route[..crossed].iter().map(|(_, side)| *side).collect(),
            mine,
            other,
            at_crossing: depth == 0,
        })
    }

    /// Which moves [`PaneTree::move_pane`] would carry out for `pane`.
    pub fn moves_for(&self, pane: PaneId) -> Moves {
        let can = |direction| self.move_plan(pane, direction).is_some();
        Moves {
            left: can(MoveDirection::Left),
            right: can(MoveDirection::Right),
            up: can(MoveDirection::Up),
            down: can(MoveDirection::Down),
        }
    }

    /// Move `pane` one step toward `direction` (see `move_plan` for which
    /// subtrees trade places). When whole sides of a split trade places the
    /// split's ratio flips too, so each side keeps the size it had; a swap
    /// between matching cells keeps every ratio. Refused (nothing changes)
    /// when there is nothing that way.
    pub fn move_pane(&mut self, pane: PaneId, direction: MoveDirection) -> Result<(), TreeError> {
        let plan = self
            .move_plan(pane, direction)
            .ok_or(TreeError::NoMove(pane))?;
        self.swap_nodes(&plan.mine, &plan.other)?;
        if plan.at_crossing
            && let Some(LayoutNode::Split(split)) = self
                .root
                .as_mut()
                .and_then(|root| root.at_mut(&plan.crossed))
        {
            split.ratio = (1.0 - split.ratio).clamp(MIN_RATIO, MAX_RATIO);
        }
        Ok(())
    }

    /// Exchange two panes' places; every split keeps its shape and ratio.
    pub fn swap(&mut self, a: PaneId, b: PaneId) -> Result<(), TreeError> {
        if a == b {
            return Err(TreeError::NoMove(a));
        }
        let root = self.root.as_ref().ok_or(TreeError::UnknownPane(a))?;
        let to_sides = |pane| {
            root.route_to(pane)
                .map(|route| route.into_iter().map(|(_, side)| side).collect::<Vec<_>>())
                .ok_or(TreeError::UnknownPane(pane))
        };
        let (route_a, route_b) = (to_sides(a)?, to_sides(b)?);
        self.swap_nodes(&route_a, &route_b)
    }

    /// Take `pane` out of its place and put it on `side` of `target` along
    /// `axis`, halving `target`'s box. The pane keeps its id (and so its
    /// session); only the layout changes.
    pub fn dock(
        &mut self,
        pane: PaneId,
        target: PaneId,
        axis: SplitAxis,
        side: Side,
    ) -> Result<(), TreeError> {
        if pane == target {
            return Err(TreeError::NoMove(pane));
        }
        if !self.contains(target) {
            return Err(TreeError::UnknownPane(target));
        }
        let before = self.clone();
        self.remove(pane)?;
        if let Err(error) = self.split(target, axis, side, pane) {
            *self = before;
            return Err(error);
        }
        Ok(())
    }

    /// Exchange the subtrees at two disjoint routes.
    fn swap_nodes(&mut self, a: &[bool], b: &[bool]) -> Result<(), TreeError> {
        let root = self.root.as_mut().ok_or(TreeError::InvalidMove)?;
        let first = root.at(a).cloned().ok_or(TreeError::InvalidMove)?;
        let second = root.at(b).cloned().ok_or(TreeError::InvalidMove)?;
        if a.starts_with(b) || b.starts_with(a) {
            return Err(TreeError::InvalidMove);
        }
        *root.at_mut(a).ok_or(TreeError::InvalidMove)? = second;
        *root.at_mut(b).ok_or(TreeError::InvalidMove)? = first;
        Ok(())
    }

    /// The ratio of split `id`.
    pub fn ratio(&self, id: SplitId) -> Option<f64> {
        self.root
            .as_ref()
            .and_then(|root| root.find_split(id))
            .map(|split| split.ratio)
    }

    /// Set split `id`'s ratio, clamped to `MIN_RATIO` and `MAX_RATIO`. Returns
    /// the ratio stored.
    pub fn set_ratio(&mut self, id: SplitId, ratio: f64) -> Result<f64, TreeError> {
        if !ratio.is_finite() {
            return Err(TreeError::InvalidRatio);
        }
        let split = self
            .root
            .as_mut()
            .and_then(|root| root.find_split_mut(id))
            .ok_or(TreeError::UnknownSplit(id))?;
        split.ratio = ratio.clamp(MIN_RATIO, MAX_RATIO);
        Ok(split.ratio)
    }

    /// The ratio a divider drag asks for: the pointer's position along the
    /// divider's `span` (see [`DividerLayout`]), clamped so neither side
    /// drops under `MIN_PANE_PX` when the span has room for two such
    /// panes, and never outside `MIN_RATIO` and `MAX_RATIO`. Pure: the caller
    /// stores it with [`PaneTree::set_ratio`].
    pub fn drag_ratio(axis: SplitAxis, span: PaneBounds, pointer: (f64, f64)) -> Option<f64> {
        let (origin, extent, at) = match axis {
            SplitAxis::Horizontal => (span.x, span.width, pointer.0),
            SplitAxis::Vertical => (span.y, span.height, pointer.1),
        };
        if extent <= 0.0 || !extent.is_finite() || !at.is_finite() {
            return None;
        }
        Some(clamp_ratio((at - origin) / extent, extent))
    }

    /// Lay the tree out over `rect`: every pane's box, every divider strip.
    pub fn layout(&self, rect: PaneBounds) -> TreeLayout {
        let mut out = TreeLayout::default();
        if let Some(root) = &self.root {
            root.lay_out(rect, &mut out);
        }
        out
    }

    /// The tree's invariants against the manager's live panes, checked by
    /// the tests after every operation (and by the host in debug builds):
    /// every leaf is a live pane and every live pane is a leaf, no pane
    /// appears twice, split ids are unique, every ratio is in range, and the
    /// active pane is in the tree exactly when the tree is not empty.
    pub fn check_invariants(&self, live: &[PaneId], active: Option<PaneId>) -> Result<(), String> {
        let leaves = self.leaves();
        let mut seen = std::collections::BTreeSet::new();
        for leaf in &leaves {
            if !seen.insert(*leaf) {
                return Err(format!("{leaf} appears twice in the tree"));
            }
            if !live.contains(leaf) {
                return Err(format!("{leaf} is in the tree but not live"));
            }
        }
        for pane in live {
            if !seen.contains(pane) {
                return Err(format!("{pane} is live but not in the tree"));
            }
        }
        let mut splits = std::collections::BTreeSet::new();
        let mut stack: Vec<&LayoutNode> = self.root.iter().collect();
        while let Some(node) = stack.pop() {
            if let LayoutNode::Split(split) = node {
                if !splits.insert(split.id) {
                    return Err(format!("split-{} appears twice", split.id.get()));
                }
                if !(MIN_RATIO..=MAX_RATIO).contains(&split.ratio) {
                    return Err(format!(
                        "split-{} ratio {} out of range",
                        split.id.get(),
                        split.ratio
                    ));
                }
                stack.push(&split.first);
                stack.push(&split.second);
            }
        }
        match (active, leaves.is_empty()) {
            (Some(active), _) if !seen.contains(&active) => {
                Err(format!("active {active} is not in the tree"))
            }
            (None, false) => Err("panes are placed but none is active".to_string()),
            _ => Ok(()),
        }
    }
}

/// Clamp a ratio for a split `extent` px long: the stored range, narrowed so
/// both sides keep [`MIN_PANE_PX`] whenever the extent can afford two.
fn clamp_ratio(ratio: f64, extent: f64) -> f64 {
    let (mut lo, mut hi) = (MIN_RATIO, MAX_RATIO);
    if extent >= 2.0 * MIN_PANE_PX {
        lo = lo.max(MIN_PANE_PX / extent);
        hi = hi.min(1.0 - MIN_PANE_PX / extent);
    }
    ratio.clamp(lo, hi)
}

#[cfg(test)]
mod tests {
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

    fn check(tree: &PaneTree, active: Option<PaneId>) {
        let live = tree.leaves();
        tree.check_invariants(&live, active)
            .unwrap_or_else(|e| panic!("invariant: {e}\n{tree:#?}"));
    }

    /// The layout tiles the rect: pane areas add up to it, nobody overlaps.
    fn assert_tiles(layout: &TreeLayout, whole: PaneBounds) {
        let area: f64 = layout.panes.iter().map(|(_, b)| b.width * b.height).sum();
        assert_eq!(
            area,
            whole.width * whole.height,
            "pane areas must add up to the rect"
        );
        for (i, (a, ra)) in layout.panes.iter().enumerate() {
            for (b, rb) in layout.panes.iter().skip(i + 1) {
                let overlap_x = (ra.x + ra.width).min(rb.x + rb.width) - ra.x.max(rb.x);
                let overlap_y = (ra.y + ra.height).min(rb.y + rb.height) - ra.y.max(rb.y);
                assert!(
                    overlap_x <= 0.0 || overlap_y <= 0.0,
                    "{a} and {b} overlap: {ra:?} {rb:?}"
                );
            }
        }
    }

    #[test]
    fn a_single_pane_fills_the_workspace() {
        let mut tree = PaneTree::new();
        assert!(tree.is_empty());
        tree.set_root(p(1)).unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(tree.set_root(p(2)), Err(TreeError::NotEmpty));
        let layout = tree.layout(rect(1000.0, 800.0));
        assert_eq!(layout.panes, vec![(p(1), rect(1000.0, 800.0))]);
        assert!(layout.dividers.is_empty());
    }

    #[test]
    fn a_horizontal_split_puts_panes_side_by_side() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let split = tree
            .split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(tree.leaves(), vec![p(1), p(2)]);
        assert_eq!(tree.ratio(split), Some(EVEN));
        let whole = rect(1001.0, 700.0);
        let layout = tree.layout(whole);
        let left = layout.bounds_of(p(1)).unwrap();
        let right = layout.bounds_of(p(2)).unwrap();
        assert_eq!((left.x, left.width, left.height), (0.0, 501.0, 700.0));
        assert_eq!((right.x, right.width, right.height), (501.0, 500.0, 700.0));
        assert_tiles(&layout, whole);
        let divider = layout.dividers[0];
        assert_eq!(divider.split, split);
        assert_eq!(divider.hit.x, 501.0 - DIVIDER_HIT_PX / 2.0);
        assert_eq!(divider.hit.height, 700.0);
        assert_eq!(divider.span, whole);
    }

    #[test]
    fn a_vertical_split_stacks_panes_and_before_goes_first() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Vertical, Side::Before, p(2))
            .unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(
            tree.leaves(),
            vec![p(2), p(1)],
            "Before puts the new pane on top"
        );
        let layout = tree.layout(rect(800.0, 600.0));
        assert_eq!(layout.bounds_of(p(2)).unwrap().y, 0.0);
        assert_eq!(layout.bounds_of(p(1)).unwrap().y, 300.0);
        assert_eq!(layout.dividers[0].hit.width, 800.0);
    }

    #[test]
    fn nested_splits_lay_out_inside_their_parent() {
        // [1 | [2 / 3]]
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        tree.split(p(2), SplitAxis::Vertical, Side::After, p(3))
            .unwrap();
        check(&tree, Some(p(2)));
        assert_eq!(tree.leaves(), vec![p(1), p(2), p(3)]);
        let whole = rect(1200.0, 800.0);
        let layout = tree.layout(whole);
        assert_tiles(&layout, whole);
        assert_eq!(layout.bounds_of(p(1)).unwrap(), rect(600.0, 800.0));
        assert_eq!(
            layout.bounds_of(p(2)).unwrap(),
            PaneBounds {
                x: 600.0,
                y: 0.0,
                width: 600.0,
                height: 400.0
            }
        );
        assert_eq!(
            layout.bounds_of(p(3)).unwrap(),
            PaneBounds {
                x: 600.0,
                y: 400.0,
                width: 600.0,
                height: 400.0
            }
        );
        assert_eq!(layout.dividers.len(), 2);
        // The inner divider spans only its own split's box.
        assert_eq!(layout.dividers[1].span.x, 600.0);
        assert_eq!(layout.dividers[1].hit.width, 600.0);
    }

    #[test]
    fn closing_the_left_pane_leaves_the_right_filling_the_workspace() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        assert_eq!(tree.remove(p(1)), Ok(Some(p(2))));
        check(&tree, Some(p(2)));
        assert_eq!(
            tree.root(),
            Some(&LayoutNode::Leaf(p(2))),
            "the unary split normalizes away"
        );
        assert_eq!(
            tree.layout(rect(900.0, 600.0)).panes,
            vec![(p(2), rect(900.0, 600.0))]
        );
    }

    #[test]
    fn closing_the_right_pane_leaves_the_left() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        assert_eq!(tree.remove(p(2)), Ok(Some(p(1))));
        check(&tree, Some(p(1)));
        assert_eq!(tree.root(), Some(&LayoutNode::Leaf(p(1))));
    }

    #[test]
    fn closing_a_nested_pane_collapses_only_its_own_split() {
        // [1 | [2 / 3]] − 2 → [1 | 3], the outer ratio kept.
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let outer = tree
            .split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        tree.split(p(2), SplitAxis::Vertical, Side::After, p(3))
            .unwrap();
        tree.set_ratio(outer, 0.3).unwrap();
        assert_eq!(tree.remove(p(2)), Ok(Some(p(3))));
        check(&tree, Some(p(3)));
        assert_eq!(tree.leaves(), vec![p(1), p(3)]);
        assert_eq!(
            tree.ratio(outer),
            Some(0.3),
            "the surviving split keeps its ratio"
        );
        let LayoutNode::Split(split) = tree.root().unwrap() else {
            panic!("the outer split survives");
        };
        assert_eq!(split.second, LayoutNode::Leaf(p(3)));
    }

    #[test]
    fn closing_a_pane_hands_focus_to_the_nearest_leaf_of_its_sibling() {
        // [1 | [2 / 3]]: closing 1 (a first child) → the sibling's FIRST
        // leaf, 2 — the one that touched it.
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        tree.split(p(2), SplitAxis::Vertical, Side::After, p(3))
            .unwrap();
        assert_eq!(tree.clone().remove(p(1)), Ok(Some(p(2))));
        // [[1 / 2] | 3]: closing 3 (a second child) → the sibling's LAST
        // leaf, 2.
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(3))
            .unwrap();
        tree.split(p(1), SplitAxis::Vertical, Side::After, p(2))
            .unwrap();
        assert_eq!(tree.leaves(), vec![p(1), p(2), p(3)]);
        assert_eq!(tree.remove(p(3)), Ok(Some(p(2))));
        check(&tree, Some(p(2)));
    }

    #[test]
    fn closing_the_last_pane_empties_the_tree() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        assert_eq!(tree.remove(p(1)), Ok(None));
        assert!(tree.is_empty());
        check(&tree, None);
        assert!(tree.layout(rect(10.0, 10.0)).panes.is_empty());
        // And a fresh root can be placed again.
        tree.set_root(p(2)).unwrap();
        check(&tree, Some(p(2)));
    }

    #[test]
    fn refused_operations_change_nothing() {
        let mut tree = PaneTree::new();
        assert_eq!(tree.remove(p(1)), Err(TreeError::UnknownPane(p(1))));
        tree.set_root(p(1)).unwrap();
        let before = tree.clone();
        assert_eq!(
            tree.split(p(9), SplitAxis::Horizontal, Side::After, p(2)),
            Err(TreeError::UnknownPane(p(9)))
        );
        assert_eq!(
            tree.split(p(1), SplitAxis::Horizontal, Side::After, p(1)),
            Err(TreeError::DuplicatePane(p(1)))
        );
        assert_eq!(tree.remove(p(9)), Err(TreeError::UnknownPane(p(9))));
        assert_eq!(
            tree.set_ratio(SplitId(42), 0.5),
            Err(TreeError::UnknownSplit(SplitId(42)))
        );
        assert_eq!(tree, before);
        let split = tree
            .split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        assert_eq!(
            tree.set_ratio(split, f64::NAN),
            Err(TreeError::InvalidRatio)
        );
        assert_eq!(tree.ratio(split), Some(EVEN));
    }

    #[test]
    fn a_ratio_is_clamped_to_the_usable_range() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let split = tree
            .split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        assert_eq!(tree.set_ratio(split, 0.7), Ok(0.7));
        assert_eq!(tree.set_ratio(split, 0.0), Ok(MIN_RATIO));
        assert_eq!(tree.set_ratio(split, 1.5), Ok(MAX_RATIO));
        assert_eq!(tree.set_ratio(split, -3.0), Ok(MIN_RATIO));
        check(&tree, Some(p(1)));
    }

    #[test]
    fn a_drag_becomes_a_ratio_that_keeps_both_panes_usable() {
        let span = PaneBounds {
            x: 100.0,
            y: 50.0,
            width: 1000.0,
            height: 600.0,
        };
        // The pointer's place along the span.
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Horizontal, span, (600.0, 0.0)),
            Some(0.5)
        );
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Vertical, span, (0.0, 350.0)),
            Some(0.5)
        );
        // A quarter of a 600 px span would leave the upper pane 150 px:
        // the drag stops where it keeps MIN_PANE_PX.
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Vertical, span, (0.0, 200.0)),
            Some(MIN_PANE_PX / 600.0)
        );
        // Neither side under MIN_PANE_PX when the span affords two: 1000 px
        // wide → [0.2, 0.8], tighter than the stored range.
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Horizontal, span, (150.0, 0.0)),
            Some(0.2)
        );
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Horizontal, span, (1090.0, 0.0)),
            Some(0.8)
        );
        // A span too small for two minimum panes falls back to the stored
        // range rather than an empty one.
        let narrow = PaneBounds {
            width: 300.0,
            ..span
        };
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Horizontal, narrow, (100.0, 0.0)),
            Some(MIN_RATIO)
        );
        // No extent, no ratio.
        let flat = PaneBounds { width: 0.0, ..span };
        assert_eq!(
            PaneTree::drag_ratio(SplitAxis::Horizontal, flat, (100.0, 0.0)),
            None
        );
    }

    #[test]
    fn layout_rounds_to_whole_pixels_and_always_tiles() {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        let a = tree
            .split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        let b = tree
            .split(p(2), SplitAxis::Vertical, Side::After, p(3))
            .unwrap();
        tree.set_ratio(a, 0.333).unwrap();
        tree.set_ratio(b, 0.61).unwrap();
        for (w, h) in [(997.0, 613.0), (1280.0, 720.0), (401.0, 399.0)] {
            let whole = rect(w, h);
            let layout = tree.layout(whole);
            for (_, bounds) in &layout.panes {
                for v in [bounds.x, bounds.y, bounds.width, bounds.height] {
                    assert_eq!(v, v.round(), "whole pixels only: {bounds:?}");
                }
            }
            assert_tiles(&layout, whole);
        }
    }

    #[test]
    fn a_pane_id_is_not_a_document_the_same_document_twice_is_two_panes() {
        // The tree knows panes, never documents: two panes that happen to
        // show one book are two leaves, closed independently.
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(2))
            .unwrap();
        assert_eq!(tree.remove(p(2)), Ok(Some(p(1))));
        assert!(tree.contains(p(1)) && !tree.contains(p(2)));
    }

    /// Random split / close / resize sequences against a plain model: the
    /// invariants hold after every step, the leaf set is exactly the model's,
    /// and every layout tiles the workspace.
    #[test]
    fn random_operations_keep_every_invariant() {
        let mut seed: u64 = 0x5eed_1234_abcd_0001;
        let mut next = move |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        for _round in 0..40 {
            let mut tree = PaneTree::new();
            let mut model: Vec<PaneId> = Vec::new();
            let mut active: Option<PaneId> = None;
            let mut minted = 0u64;
            for _step in 0..60 {
                match next(4) {
                    // Split (or place the root).
                    0 | 1 => {
                        minted += 1;
                        let new = p(minted);
                        if model.is_empty() {
                            tree.set_root(new).unwrap();
                        } else {
                            let target = model[next(model.len() as u64) as usize];
                            let axis = if next(2) == 0 {
                                SplitAxis::Horizontal
                            } else {
                                SplitAxis::Vertical
                            };
                            let side = if next(2) == 0 {
                                Side::Before
                            } else {
                                Side::After
                            };
                            tree.split(target, axis, side, new).unwrap();
                        }
                        model.push(new);
                        active = Some(new);
                    }
                    // Close one.
                    2 if !model.is_empty() => {
                        let victim = model.remove(next(model.len() as u64) as usize);
                        let successor = tree.remove(victim).unwrap();
                        assert_eq!(successor.is_none(), model.is_empty());
                        if let Some(successor) = successor {
                            assert!(model.contains(&successor), "the successor is a survivor");
                        }
                        if active == Some(victim) {
                            active = successor;
                        }
                    }
                    // Resize a divider.
                    _ => {
                        let layout = tree.layout(rect(1600.0, 900.0));
                        if let Some(d) = layout.dividers.get(next(4) as usize) {
                            let at = next(2000) as f64 - 200.0;
                            if let Some(ratio) = PaneTree::drag_ratio(d.axis, d.span, (at, at)) {
                                tree.set_ratio(d.split, ratio).unwrap();
                            }
                        }
                    }
                }
                check(&tree, active);
                let mut leaves = tree.leaves();
                leaves.sort();
                let mut expected = model.clone();
                expected.sort();
                assert_eq!(leaves, expected);
                let layout = tree.layout(rect(1600.0, 900.0));
                if model.is_empty() {
                    // Every pane closed: nothing is laid out.
                    assert!(layout.panes.is_empty() && layout.dividers.is_empty());
                } else {
                    assert_tiles(&layout, rect(1600.0, 900.0));
                }
            }
        }
    }

    /// Left column A over B, beside a tall C on the right.
    fn column_and_tall() -> PaneTree {
        let mut tree = PaneTree::new();
        tree.set_root(p(1)).unwrap();
        tree.split(p(1), SplitAxis::Horizontal, Side::After, p(3))
            .unwrap();
        tree.split(p(1), SplitAxis::Vertical, Side::After, p(2))
            .unwrap();
        tree
    }

    fn boxes(tree: &PaneTree) -> Vec<(PaneId, (f64, f64, f64, f64))> {
        tree.layout(rect(1000.0, 800.0))
            .panes
            .into_iter()
            .map(|(id, b)| (id, (b.x, b.y, b.width, b.height)))
            .collect()
    }

    #[test]
    fn a_stacked_pair_moves_past_a_tall_pane_as_a_column() {
        let mut tree = column_and_tall();
        assert!(tree.moves_for(p(1)).right);
        assert!(!tree.moves_for(p(1)).left);
        tree.move_pane(p(1), MoveDirection::Right).unwrap();
        check(&tree, Some(p(1)));
        let laid = boxes(&tree);
        // The tall pane takes the left column; the pair keeps its order.
        assert_eq!(laid[0], (p(3), (0.0, 0.0, 500.0, 800.0)));
        assert_eq!(laid[1], (p(1), (500.0, 0.0, 500.0, 400.0)));
        assert_eq!(laid[2], (p(2), (500.0, 400.0, 500.0, 400.0)));
    }

    #[test]
    fn a_tall_pane_moves_past_a_stacked_pair_as_a_whole() {
        let mut tree = column_and_tall();
        assert!(tree.moves_for(p(3)).left);
        assert!(!tree.moves_for(p(3)).up && !tree.moves_for(p(3)).down);
        tree.move_pane(p(3), MoveDirection::Left).unwrap();
        check(&tree, Some(p(3)));
        assert_eq!(tree.leaves(), vec![p(3), p(1), p(2)]);
    }

    #[test]
    fn moving_down_swaps_the_two_panes_of_a_column() {
        let mut tree = column_and_tall();
        assert!(tree.moves_for(p(1)).down);
        assert!(!tree.moves_for(p(2)).down);
        tree.move_pane(p(1), MoveDirection::Down).unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(tree.leaves(), vec![p(2), p(1), p(3)]);
    }

    #[test]
    fn in_a_grid_only_the_matching_cell_trades_places() {
        // A over B on the left, C over D on the right.
        let mut tree = column_and_tall();
        tree.split(p(3), SplitAxis::Vertical, Side::After, p(4))
            .unwrap();
        tree.move_pane(p(1), MoveDirection::Right).unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(tree.leaves(), vec![p(3), p(2), p(1), p(4)]);
        let laid = boxes(&tree);
        assert_eq!(laid[2], (p(1), (500.0, 0.0, 500.0, 400.0)));
    }

    #[test]
    fn a_whole_side_move_keeps_each_side_its_size() {
        let mut tree = column_and_tall();
        let Some(LayoutNode::Split(root)) = tree.root() else {
            panic!("a split root");
        };
        let id = root.id;
        tree.set_ratio(id, 0.3).unwrap();
        tree.move_pane(p(1), MoveDirection::Right).unwrap();
        let laid = boxes(&tree);
        assert_eq!(laid[0], (p(3), (0.0, 0.0, 700.0, 800.0)));
        assert_eq!(laid[1].1.2, 300.0);
    }

    #[test]
    fn a_move_with_nowhere_to_go_changes_nothing() {
        let mut tree = column_and_tall();
        let before = tree.clone();
        assert_eq!(
            tree.move_pane(p(3), MoveDirection::Right),
            Err(TreeError::NoMove(p(3)))
        );
        assert_eq!(tree, before);
        let mut single = PaneTree::new();
        single.set_root(p(1)).unwrap();
        assert!(!single.moves_for(p(1)).any());
    }

    #[test]
    fn swapping_and_docking_keep_every_pane_once() {
        let mut tree = column_and_tall();
        tree.swap(p(2), p(3)).unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(tree.leaves(), vec![p(1), p(3), p(2)]);
        assert_eq!(tree.swap(p(2), p(2)), Err(TreeError::NoMove(p(2))));
        tree.dock(p(1), p(2), SplitAxis::Vertical, Side::After)
            .unwrap();
        check(&tree, Some(p(1)));
        assert_eq!(tree.leaves(), vec![p(3), p(2), p(1)]);
        let before = tree.clone();
        assert!(
            tree.dock(p(1), p(1), SplitAxis::Vertical, Side::After)
                .is_err()
        );
        assert_eq!(tree, before);
    }
}
