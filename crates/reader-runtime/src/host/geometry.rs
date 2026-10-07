//! Drop geometry: the workspace and panes as a drag sees them.

use super::drop_target::{DropTarget, Edge};
use super::model::{PaneBounds, PaneFormat, PaneId};
use super::tree::split_fits;

/// How much better a rival target must be to take the preview.
const HYSTERESIS: f64 = 0.1;

/// One visible pane as a drag sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct PaneGeometry {
    pub pane: PaneId,
    /// The pane's box in slot coordinates (the layout's).
    pub rect: PaneBounds,
    pub format: PaneFormat,
    /// The pane holds no document (a warm reader's empty root).
    pub empty: bool,
}

/// The workspace as measured at the start of a drag.
#[derive(Clone, Debug, PartialEq)]
pub struct DropGeometry {
    /// The workspace slot in CLIENT coordinates (the pointer's).
    pub workspace: PaneBounds,
    pub panes: Vec<PaneGeometry>,
    /// The workspace may take another pane (it is not full).
    pub can_add: bool,
}

impl DropGeometry {
    /// The pointer in slot coordinates; half-open at the far edges.
    pub fn to_slot(&self, client: (f64, f64)) -> Option<(f64, f64)> {
        let at = (client.0 - self.workspace.x, client.1 - self.workspace.y);
        contains(
            PaneBounds {
                x: 0.0,
                y: 0.0,
                ..self.workspace
            },
            at,
        )
        .then_some(at)
    }

    /// The pane under a slot point; a shared edge goes to one pane.
    fn pane_at(&self, at: (f64, f64)) -> Option<&PaneGeometry> {
        self.panes.iter().find(|pane| contains(pane.rect, at))
    }

    pub fn pane(&self, id: PaneId) -> Option<&PaneGeometry> {
        self.panes.iter().find(|pane| pane.pane == id)
    }

    /// Every target `pane` offers, in priority order.
    fn targets_of(&self, pane: &PaneGeometry) -> Vec<DropTarget> {
        if pane.empty {
            return vec![DropTarget::Here { pane: pane.pane }];
        }
        if !self.can_add {
            return Vec::new();
        }
        Edge::PRIORITY
            .into_iter()
            .filter(|edge| split_fits(pane.rect, edge.placement().0))
            .map(|edge| DropTarget::Split {
                pane: pane.pane,
                edge,
            })
            .collect()
    }

    /// Whether the geometry offers `target` at all.
    pub fn offers(&self, target: DropTarget) -> bool {
        self.pane(target.pane())
            .is_some_and(|pane| self.targets_of(pane).contains(&target))
    }

    /// The target the pointer chooses, given the current one.
    pub fn choose(&self, client: (f64, f64), current: Option<DropTarget>) -> Option<DropTarget> {
        let at = self.to_slot(client)?;
        let pane = self.pane_at(at)?;
        let targets = self.targets_of(pane);
        let mut best: Option<(DropTarget, f64)> = None;
        for target in targets.iter().copied() {
            let score = score(target, pane.rect, at);
            // Strictly greater: a draw keeps the earlier (higher-priority)
            // target.
            if best.is_none_or(|(_, top)| score > top) {
                best = Some((target, score));
            }
        }
        let (best, top) = best?;
        if let Some(current) = current
            && current != best
            && targets.contains(&current)
            && top < score(current, pane.rect, at) + HYSTERESIS
        {
            return Some(current);
        }
        Some(best)
    }
}

/// `at` inside `rect`, half-open.
fn contains(rect: PaneBounds, at: (f64, f64)) -> bool {
    at.0 >= rect.x && at.0 < rect.x + rect.width && at.1 >= rect.y && at.1 < rect.y + rect.height
}

/// A target's score: 1 at its edge, 0 at the opposite; `Here`
/// always scores 1.
pub fn score(target: DropTarget, rect: PaneBounds, at: (f64, f64)) -> f64 {
    let DropTarget::Split { edge, .. } = target else {
        return 1.0;
    };
    let distance = match edge {
        Edge::Left => (at.0 - rect.x) / rect.width,
        Edge::Right => (rect.x + rect.width - at.0) / rect.width,
        Edge::Top => (at.1 - rect.y) / rect.height,
        Edge::Bottom => (rect.y + rect.height - at.1) / rect.height,
    };
    (1.0 - distance).clamp(0.0, 1.0)
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

    fn pane(n: u64, rect: PaneBounds) -> PaneGeometry {
        PaneGeometry {
            pane: p(n),
            rect,
            format: PaneFormat::Markdown,
            empty: false,
        }
    }

    /// A workspace at client (40, 60): 1000×800, one pane filling it.
    fn single() -> DropGeometry {
        DropGeometry {
            workspace: rect(40.0, 60.0, 1000.0, 800.0),
            panes: vec![pane(1, rect(0.0, 0.0, 1000.0, 800.0))],
            can_add: true,
        }
    }

    /// Pane 1 on the left (600 wide), pane 2 on the right (400 wide).
    fn pair() -> DropGeometry {
        DropGeometry {
            workspace: rect(0.0, 0.0, 1000.0, 800.0),
            panes: vec![
                pane(1, rect(0.0, 0.0, 600.0, 800.0)),
                pane(2, rect(600.0, 0.0, 400.0, 800.0)),
            ],
            can_add: true,
        }
    }

    fn split(n: u64, edge: Edge) -> Option<DropTarget> {
        Some(DropTarget::Split { pane: p(n), edge })
    }

    // --- target derivation ------------------------------------------------

    #[test]
    fn a_roomy_pane_offers_all_four_edges_in_priority_order() {
        let g = single();
        let targets = g.targets_of(&g.panes[0]);
        let edges: Vec<Edge> = targets
            .iter()
            .map(|t| match t {
                DropTarget::Split { edge, .. } => *edge,
                DropTarget::Here { .. } => panic!("a pane with a document has no centre"),
            })
            .collect();
        assert_eq!(edges, Edge::PRIORITY.to_vec());
    }

    #[test]
    fn an_empty_pane_offers_only_itself() {
        let mut g = single();
        g.panes[0].empty = true;
        assert_eq!(
            g.targets_of(&g.panes[0]),
            vec![DropTarget::Here { pane: p(1) }]
        );
        // The whole pane is the target, corner to corner.
        for client in [(40.0, 60.0), (540.0, 460.0), (1039.0, 859.0)] {
            assert_eq!(
                g.choose(client, None),
                Some(DropTarget::Here { pane: p(1) })
            );
        }
    }

    // --- target rejection -------------------------------------------------

    #[test]
    fn a_narrow_pane_offers_no_side_by_side_split() {
        // 399 wide: halves of 200 and 199 — one under the minimum.
        let g = DropGeometry {
            workspace: rect(0.0, 0.0, 399.0, 800.0),
            panes: vec![pane(1, rect(0.0, 0.0, 399.0, 800.0))],
            can_add: true,
        };
        let targets = g.targets_of(&g.panes[0]);
        assert_eq!(
            targets,
            vec![
                split(1, Edge::Bottom).unwrap(),
                split(1, Edge::Top).unwrap()
            ]
        );
        // Hugging the left edge still cannot choose a left split.
        assert_eq!(g.choose((1.0, 400.0), None), split(1, Edge::Bottom));
    }

    #[test]
    fn a_pane_too_small_both_ways_offers_nothing() {
        // A narrow window with the rail open: 380×390.
        let g = DropGeometry {
            workspace: rect(260.0, 44.0, 380.0, 390.0),
            panes: vec![pane(1, rect(0.0, 0.0, 380.0, 390.0))],
            can_add: true,
        };
        assert!(g.targets_of(&g.panes[0]).is_empty());
        assert_eq!(g.choose((300.0, 100.0), None), None);
    }

    #[test]
    fn a_nested_pane_near_the_minimum_keeps_only_what_fits() {
        // B is 450×300: wide enough to halve, too short to stack.
        let g = DropGeometry {
            workspace: rect(0.0, 0.0, 900.0, 600.0),
            panes: vec![
                pane(1, rect(0.0, 0.0, 450.0, 600.0)),
                pane(2, rect(450.0, 0.0, 450.0, 300.0)),
                pane(3, rect(450.0, 300.0, 450.0, 300.0)),
            ],
            can_add: true,
        };
        assert_eq!(
            g.targets_of(&g.panes[1]),
            vec![
                split(2, Edge::Right).unwrap(),
                split(2, Edge::Left).unwrap()
            ]
        );
    }

    #[test]
    fn a_full_workspace_offers_no_split_anywhere() {
        let mut g = pair();
        g.can_add = false;
        for pane in &g.panes {
            assert!(g.targets_of(pane).is_empty());
        }
        assert_eq!(g.choose((300.0, 400.0), None), None);
    }

    // --- deterministic scoring and boundaries -----------------------------

    #[test]
    fn the_same_geometry_and_pointer_give_the_same_target() {
        let g = pair();
        for client in [(10.0, 10.0), (300.0, 400.0), (599.0, 5.0), (800.0, 790.0)] {
            let first = g.choose(client, None);
            for _ in 0..8 {
                assert_eq!(g.choose(client, None), first);
            }
        }
    }

    #[test]
    fn the_nearest_edge_wins_inside_a_pane() {
        let g = single();
        // Client → slot is (−40, −60).
        assert_eq!(
            g.choose((40.0 + 950.0, 60.0 + 400.0), None),
            split(1, Edge::Right)
        );
        assert_eq!(
            g.choose((40.0 + 50.0, 60.0 + 400.0), None),
            split(1, Edge::Left)
        );
        assert_eq!(
            g.choose((40.0 + 500.0, 60.0 + 30.0), None),
            split(1, Edge::Top)
        );
        assert_eq!(
            g.choose((40.0 + 500.0, 60.0 + 770.0), None),
            split(1, Edge::Bottom)
        );
    }

    #[test]
    fn the_exact_centre_resolves_by_priority() {
        let g = single();
        // Every edge scores 0.5: Right comes first.
        assert_eq!(
            g.choose((40.0 + 500.0, 60.0 + 400.0), None),
            split(1, Edge::Right)
        );
    }

    #[test]
    fn an_exact_zone_boundary_resolves_by_priority() {
        let g = single();
        // On the top-left diagonal, Left and Top draw: Left outranks Top.
        assert_eq!(
            g.choose((40.0 + 100.0, 60.0 + 80.0), None),
            split(1, Edge::Left)
        );
        // On the bottom-right diagonal, Right and Bottom draw: Right first.
        assert_eq!(
            g.choose((40.0 + 900.0, 60.0 + 720.0), None),
            split(1, Edge::Right)
        );
    }

    #[test]
    fn a_pane_corner_picks_one_of_its_two_edges() {
        let g = single();
        // The very corner: Left and Top both score 1 — Left outranks Top.
        assert_eq!(g.choose((40.0, 60.0), None), split(1, Edge::Left));
        // The nearer edge wins: 1 px from the right against 5 px from the
        // top.
        assert_eq!(
            g.choose((40.0 + 999.0, 60.0 + 5.0), None),
            split(1, Edge::Right)
        );
        assert_eq!(g.choose((40.0 + 999.0, 60.0), None), split(1, Edge::Top));
        // 1 px from the left (0.999) against 1 px from the bottom (0.99875).
        assert_eq!(
            g.choose((40.0 + 1.0, 60.0 + 799.0), None),
            split(1, Edge::Left)
        );
    }

    #[test]
    fn beside_a_divider_the_pane_under_the_pointer_is_split() {
        let g = pair();
        // The seam belongs to pane 2.
        assert_eq!(g.choose((598.0, 400.0), None), split(1, Edge::Right));
        assert_eq!(g.choose((600.0, 400.0), None), split(2, Edge::Left));
        assert_eq!(g.choose((603.0, 400.0), None), split(2, Edge::Left));
    }

    #[test]
    fn beside_a_divider_a_split_with_no_room_is_never_chosen() {
        // Pane 2 is 399 wide: hugging its left edge chooses a stack.
        let g = DropGeometry {
            workspace: rect(0.0, 0.0, 1000.0, 800.0),
            panes: vec![
                pane(1, rect(0.0, 0.0, 601.0, 800.0)),
                pane(2, rect(601.0, 0.0, 399.0, 800.0)),
            ],
            can_add: true,
        };
        let chosen = g.choose((602.0, 380.0), None);
        assert_eq!(chosen, split(2, Edge::Top));
        assert!(g.offers(chosen.unwrap()));
    }

    #[test]
    fn the_workspace_boundary_is_half_open() {
        let g = single();
        // The last pixel row/column is inside, the edge itself outside.
        assert!(g.choose((40.0 + 999.0, 60.0 + 400.0), None).is_some());
        assert_eq!(g.choose((40.0 + 1000.0, 60.0 + 400.0), None), None);
        assert_eq!(g.choose((40.0 + 500.0, 60.0 + 800.0), None), None);
        assert_eq!(g.choose((39.0, 460.0), None), None);
    }

    #[test]
    fn inside_the_workspace_but_outside_every_pane_offers_nothing() {
        // A pane smaller than the workspace: the uncovered strip is no
        // target.
        let g = DropGeometry {
            workspace: rect(0.0, 0.0, 1000.0, 800.0),
            panes: vec![pane(1, rect(0.0, 0.0, 700.0, 800.0))],
            can_add: true,
        };
        assert_eq!(g.choose((850.0, 400.0), None), None);
    }

    #[test]
    fn outside_the_workspace_offers_nothing() {
        let g = single();
        assert_eq!(g.choose((10.0, 10.0), None), None);
        assert_eq!(g.choose((2000.0, 400.0), None), None);
    }

    // --- hysteresis -------------------------------------------------------

    #[test]
    fn the_centre_boundary_does_not_flip_on_jitter() {
        let g = single();
        let centre_x = 40.0 + 500.0;
        let y = 60.0 + 400.0;
        // Start clearly left, then wobble around the centre line.
        let mut current = g.choose((40.0 + 100.0, y), None);
        assert_eq!(current, split(1, Edge::Left));
        for dx in [-3.0, 2.0, -1.0, 4.0, -2.0, 5.0, 0.0, 3.0] {
            current = g.choose((centre_x + dx, y), current);
            assert_eq!(current, split(1, Edge::Left), "flipped at dx {dx}");
        }
    }

    #[test]
    fn a_clear_move_past_the_margin_switches_the_target() {
        let g = single();
        let y = 60.0 + 400.0;
        let current = g.choose((40.0 + 100.0, y), None);
        assert_eq!(current, split(1, Edge::Left));
        // Right leads Left by 0.09: inside the margin, the preview stays.
        assert_eq!(g.choose((40.0 + 545.0, y), current), current);
        // At x = 560 Right leads by 0.12 > 0.1: the preview moves.
        assert_eq!(g.choose((40.0 + 560.0, y), current), split(1, Edge::Right));
    }

    #[test]
    fn entering_another_pane_switches_at_once() {
        let g = pair();
        let current = g.choose((590.0, 400.0), None);
        assert_eq!(current, split(1, Edge::Right));
        // One pixel into pane 2: its own target, no margin to beat.
        assert_eq!(g.choose((600.0, 400.0), current), split(2, Edge::Left));
    }

    #[test]
    fn a_current_target_the_geometry_no_longer_offers_is_dropped() {
        let g = pair();
        let stale = split(9, Edge::Left);
        assert_eq!(g.choose((100.0, 400.0), stale), split(1, Edge::Left));
    }
}
