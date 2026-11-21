//! Explicit z-order tiers for every floating surface in the UI.
//!
//! No module outside `z_order` chooses a raw [`Order`]. Surfaces declare a
//! semantic [`Tier`]; [`enforce`] keeps layers sharing an [`Order`] bucket in
//! strict back-to-front tier order at the end of every frame.

use std::collections::{HashMap, HashSet};

use egui::{Area, Context, Id, LayerId, Order, Painter};

/// Semantic stacking tier, ordered strictly back-to-front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// Base viewport (`CentralPanel`, docked `SidePanel`) and passive marks
    /// painted directly into the background layer after document pages.
    Canvas,
    /// Per-pane passive overlays above the canvas and below all chrome.
    PaneMark,
    /// Per-pane hover-revealed chrome (`page-bar`).
    PaneBar,
    /// Window-edge chrome (`titlebar`, `import-run`).
    WindowChrome,
    /// Status pill tucked behind a floating tool (`toast-island`).
    AttachedPill,
    /// Non-interactable drag guides above chrome and below floating tools.
    ChromeOverlay,
    /// Page-anchored hover cards, word/AI/note bubbles, selection toolbar, and
    /// page-AI menus: above page chrome, strictly below floating tools.
    PageCard,
    /// Modeless floating tools and parked handles (`dict-window`,
    /// `dict-edge-tab`, `park::reveal`, `read-aloud-*`, `find-bar`,
    /// `study-picking`, `lift-card`).
    FloatingTool,
    /// Floating overlay sidebar (`sidebar-float`), sliding over floating tools
    /// and staying below popovers and modals.
    Sidebar,
    /// Ephemeral anchored popovers, dropdowns and context menus outside modals.
    Popover,
    /// App-blocking modal dialogs and full-window lightbox.
    Modal,
    /// `egui::Popup` menus and dropdowns opened while a modal is active.
    ModalPopover,
    /// Non-blocking notification stack (`toasts`, `toasts-open`).
    Notification,
    /// Hover tooltips, edge-tab hover previews and active drag ghosts.
    Tooltip,
    /// Window traffic-light backings and stand-in dots (`lights`).
    WindowLights,
    /// Full-window cosmetic grain pass (`film-grain`).
    PostEffect,
    /// Full-window theme transition snapshot (`crossfade`).
    Transition,
}

impl Tier {
    /// Every tier in strict back-to-front order.
    pub const ALL: [Self; 17] = [
        Self::Canvas,
        Self::PaneMark,
        Self::PaneBar,
        Self::WindowChrome,
        Self::AttachedPill,
        Self::ChromeOverlay,
        Self::PageCard,
        Self::FloatingTool,
        Self::Sidebar,
        Self::Popover,
        Self::Modal,
        Self::ModalPopover,
        Self::Notification,
        Self::Tooltip,
        Self::WindowLights,
        Self::PostEffect,
        Self::Transition,
    ];

    /// Coarse egui [`Order`] bucket housing this tier.
    pub const fn order(self) -> Order {
        match self {
            Self::Canvas => Order::Background,
            Self::PaneMark
            | Self::PaneBar
            | Self::WindowChrome
            | Self::AttachedPill
            | Self::ChromeOverlay => Order::Middle,
            Self::PageCard
            | Self::FloatingTool
            | Self::Sidebar
            | Self::Popover
            | Self::Modal
            | Self::ModalPopover => Order::Foreground,
            Self::Notification | Self::Tooltip | Self::WindowLights | Self::PostEffect => {
                Order::Tooltip
            }
            Self::Transition => Order::Debug,
        }
    }

    /// Monotonic back-to-front rank across all buckets.
    pub const fn rank(self) -> u8 {
        self as u8
    }
}

/// The three `Order` buckets that host multiple tiers of floating areas.
const SHARED_BUCKETS: [Order; 3] = [Order::Middle, Order::Foreground, Order::Tooltip];

/// Per-context layer registry, stored in egui temporary memory.
#[derive(Clone, Default)]
struct Registry {
    pass: u64,
    /// Layers drawn in the current pass with their declared tier.
    this_pass: HashMap<LayerId, Tier>,
    /// Last declared tier for every registered layer, across passes.
    known: HashMap<LayerId, Tier>,
    /// Tier assigned to each open `egui::Popup` when it first appeared.
    popup_tiers: HashMap<Id, Tier>,
}

fn registry_id() -> Id {
    Id::new("mareader-z-order-registry")
}

/// Layer id for `id` at `tier`.
pub fn layer(tier: Tier, id: Id) -> LayerId {
    LayerId::new(tier.order(), id)
}

/// Records `layer` at `tier` for the current frame.
pub fn register(ctx: &Context, tier: Tier, layer: LayerId) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let reg = d.get_temp_mut_or_default::<Registry>(registry_id());
        if reg.pass != pass {
            reg.pass = pass;
            reg.this_pass.clear();
        }
        reg.this_pass.insert(layer, tier);
        reg.known.insert(layer, tier);
    });
}

/// Records a popup layer as [`Tier::ModalPopover`] when a modal is active,
/// or [`Tier::Popover`] otherwise.
pub fn register_popup(ctx: &Context, layer: LayerId) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let reg = d.get_temp_mut_or_default::<Registry>(registry_id());
        if reg.pass != pass {
            reg.pass = pass;
            reg.this_pass.clear();
        }
        let modal_open = reg.this_pass.values().any(|t| *t == Tier::Modal);
        let tier = *reg.popup_tiers.entry(layer.id).or_insert_with(|| {
            if modal_open {
                Tier::ModalPopover
            } else {
                Tier::Popover
            }
        });
        reg.this_pass.insert(layer, tier);
        reg.known.insert(layer, tier);
    });
}

/// Creates a floating [`Area`] at `tier` and registers it for enforcement.
pub fn area(ctx: &Context, tier: Tier, id: impl Into<Id>) -> Area {
    let id = id.into();
    let layer_id = layer(tier, id);
    register(ctx, tier, layer_id);
    Area::new(id).order(tier.order())
}

/// Lets area `id` come up under its tier's open layers; call before showing it.
pub fn keep_under(ctx: &Context, tier: Tier, id: Id) {
    let below = layer(tier, id);
    let key = Id::new(("z-order-keep-under", id));
    let fresh = !ctx.memory(|m| m.areas().is_visible(&below));
    // egui raises a new area as its pass ends; the rest rise next pass.
    let due = ctx.data_mut(|d| {
        let was = d.get_temp_mut_or_default::<bool>(key);
        std::mem::replace(was, fresh)
    });
    if fresh {
        ctx.request_repaint();
    }
    if !due {
        return;
    }
    let shown: Vec<LayerId> = ctx.memory(|m| {
        let up = |l: &LayerId| *l != below && m.areas().is_visible(l);
        m.layer_ids().filter(up).collect()
    });
    for open in shown {
        if tier_of(ctx, open) == Some(tier) {
            ctx.move_to_top(open);
        }
    }
}

/// Painter for a non-interactable overlay at `tier`.
pub fn painter(ctx: &Context, tier: Tier, id: impl Into<Id>) -> Painter {
    ctx.layer_painter(layer(tier, id.into()))
}

/// Painter appending directly onto the document canvas layer after pages.
pub fn canvas_painter(ctx: &Context) -> Painter {
    ctx.layer_painter(LayerId::background())
}

/// True if `layer` belongs to the base document canvas bucket.
pub const fn is_canvas(layer: LayerId) -> bool {
    matches!(layer.order, Order::Background)
}

/// True if `layer` sits in the bucket that hosts `egui::Popup` menus.
pub const fn is_popup_order(layer: LayerId) -> bool {
    matches!(layer.order, Order::Foreground)
}

/// Declared or inferred tier for `layer`.
pub fn tier_of(ctx: &Context, layer: LayerId) -> Option<Tier> {
    let known = ctx.data(|d| {
        let reg = d.get_temp::<Registry>(registry_id())?;
        reg.known
            .get(&layer)
            .copied()
            .or_else(|| reg.popup_tiers.get(&layer.id).copied())
    });
    if known.is_some() {
        return known;
    }
    if layer.order == Order::Foreground && egui::Popup::is_id_open(ctx, layer.id) {
        return Some(Tier::Popover);
    }
    match layer.order {
        Order::Background => Some(Tier::Canvas),
        Order::Tooltip => Some(Tier::Tooltip),
        Order::Debug => Some(Tier::Transition),
        Order::Middle | Order::Foreground => None,
    }
}

/// True if `layer` is a popover, modal, notification or higher surface.
pub fn is_popover_or_above(ctx: &Context, layer: LayerId) -> bool {
    tier_of(ctx, layer).is_some_and(|t| t >= Tier::Popover)
}

/// True if `layer` sits at a tier strictly above `ceiling`.
pub fn is_above_tier(ctx: &Context, layer: LayerId, ceiling: Tier) -> bool {
    tier_of(ctx, layer).map_or_else(|| layer.order > ceiling.order(), |t| t > ceiling)
}

/// True if `upper` currently stacks strictly above `lower`.
pub fn is_above(ctx: &Context, upper: LayerId, lower: LayerId) -> bool {
    if upper.order != lower.order {
        return upper.order > lower.order;
    }
    ctx.memory(|m| {
        let mut lo_pos = None;
        let mut hi_pos = None;
        for (i, id) in m.layer_ids().enumerate() {
            if id == lower {
                lo_pos = Some(i);
            }
            if id == upper {
                hi_pos = Some(i);
            }
        }
        matches!((lo_pos, hi_pos), (Some(lo), Some(hi)) if hi > lo)
    })
}

/// Reconciles active layers sharing an [`Order`] bucket to strict tier order.
pub fn enforce(ctx: &Context) {
    let pass = ctx.cumulative_pass_nr();
    let (all_layers, visible) = ctx.memory(|m| {
        let all: Vec<LayerId> = m.layer_ids().collect();
        let vis: HashSet<LayerId> = all
            .iter()
            .copied()
            .filter(|l| m.areas().is_visible(l))
            .collect();
        (all, vis)
    });

    let open_popups: Vec<LayerId> = all_layers
        .iter()
        .copied()
        .filter(|l| {
            l.order == Order::Foreground
                && visible.contains(l)
                && egui::Popup::is_id_open(ctx, l.id)
        })
        .collect();

    let active_tiers = sync_registry(ctx, pass, &open_popups);
    let mut promote = Vec::new();
    let mut needs_repaint = false;

    for bucket in SHARED_BUCKETS {
        let active: Vec<(LayerId, u8)> = all_layers
            .iter()
            .copied()
            .filter(|l| l.order == bucket)
            .filter_map(|l| active_tiers.get(&l).map(|t| (l, t.rank())))
            .collect();
        let (bucket_promote, inverted) = plan_bucket(&active);
        promote.extend(bucket_promote);
        needs_repaint |= inverted;
    }

    for layer in promote {
        ctx.move_to_top(layer);
    }
    if needs_repaint {
        ctx.request_repaint();
    }
}

/// Updates popup tiers for this pass and returns active `(layer, tier)` pairs.
fn sync_registry(ctx: &Context, pass: u64, open_popups: &[LayerId]) -> HashMap<LayerId, Tier> {
    ctx.data_mut(|d| {
        let reg = d.get_temp_mut_or_default::<Registry>(registry_id());
        if reg.pass != pass {
            reg.pass = pass;
            reg.this_pass.clear();
        }
        let open_ids: HashSet<Id> = open_popups
            .iter()
            .map(|l| l.id)
            .chain(reg.this_pass.keys().map(|l| l.id))
            .collect();
        reg.popup_tiers.retain(|id, _| open_ids.contains(id));

        let modal_open = reg.this_pass.values().any(|t| *t == Tier::Modal);
        let mut active = reg.this_pass.clone();

        for &popup in open_popups {
            if active.contains_key(&popup) {
                continue;
            }
            let tier = *reg.popup_tiers.entry(popup.id).or_insert_with(|| {
                if modal_open {
                    Tier::ModalPopover
                } else {
                    Tier::Popover
                }
            });
            active.insert(popup, tier);
        }
        active
    })
}

/// Computes which layers in one bucket need `move_to_top`, plus whether an
/// inversion was found in the current pass order.
fn plan_bucket(active: &[(LayerId, u8)]) -> (Vec<LayerId>, bool) {
    if active.len() < 2 {
        return (Vec::new(), false);
    }

    // Track the lowest rank sitting ahead of a strictly lower-ranked layer.
    let mut min_after = u8::MAX;
    let mut smallest_misplaced: Option<u8> = None;
    for &(_, rank) in active.iter().rev() {
        if rank > min_after {
            smallest_misplaced = Some(match smallest_misplaced {
                Some(prev) => prev.min(rank),
                None => rank,
            });
        } else {
            min_after = rank;
        }
    }

    let Some(cut) = smallest_misplaced else {
        // Monotonically ordered: shield higher tiers from lower-tier clicks.
        let min_rank = active.iter().map(|&(_, r)| r).min().unwrap_or(0);
        let max_rank = active.iter().map(|&(_, r)| r).max().unwrap_or(0);
        if min_rank == max_rank {
            return (Vec::new(), false);
        }
        let promote = active
            .iter()
            .filter(|&&(_, r)| r > min_rank)
            .map(|&(l, _)| l)
            .collect();
        return (promote, false);
    };

    // Promote `>= cut` at once if ordered; else `== cut` first, rest next pass.
    let tail_sorted = is_sorted_ranks(active.iter().filter(|&&(_, r)| r >= cut).map(|&(_, r)| r));
    let promote = active
        .iter()
        .filter(|&&(_, r)| if tail_sorted { r >= cut } else { r == cut })
        .map(|&(l, _)| l)
        .collect();
    (promote, true)
}

fn is_sorted_ranks(mut ranks: impl Iterator<Item = u8>) -> bool {
    let Some(mut prev) = ranks.next() else {
        return true;
    };
    for next in ranks {
        if next < prev {
            return false;
        }
        prev = next;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_order_buckets_are_monotonically_non_decreasing() {
        for pair in Tier::ALL.windows(2) {
            let lo = pair[0];
            let hi = pair[1];
            assert!(lo < hi);
            assert!(lo.rank() < hi.rank());
            assert!(lo.order() <= hi.order());
        }
    }

    #[test]
    fn plan_bucket_shields_higher_tiers_and_repairs_inversions() {
        let tool = layer(Tier::FloatingTool, Id::new("tool"));
        let pop = layer(Tier::Popover, Id::new("pop"));
        let modal = layer(Tier::Modal, Id::new("modal"));

        // Settled order shields higher tiers without requesting a repaint.
        let settled = [
            (tool, Tier::FloatingTool.rank()),
            (pop, Tier::Popover.rank()),
            (modal, Tier::Modal.rank()),
        ];
        let (promote, inverted) = plan_bucket(&settled);
        assert!(!inverted);
        assert_eq!(promote, vec![pop, modal]);

        // Inverted order flags repaint and promotes the misplaced tier.
        let flipped = [
            (modal, Tier::Modal.rank()),
            (tool, Tier::FloatingTool.rank()),
            (pop, Tier::Popover.rank()),
        ];
        let (promote, inverted) = plan_bucket(&flipped);
        assert!(inverted);
        assert_eq!(promote, vec![modal]);

        // Siblings of identical tier need no forced promotion.
        let same = [
            (tool, Tier::FloatingTool.rank()),
            (
                layer(Tier::FloatingTool, Id::new("other")),
                Tier::FloatingTool.rank(),
            ),
        ];
        let (promote, inverted) = plan_bucket(&same);
        assert!(!inverted);
        assert!(promote.is_empty());
    }
}
