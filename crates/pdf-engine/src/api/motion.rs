//! The render scheduler's motion input.
//!
//! The virtualizer knows where the reader is going; the engine knows what a
//! raster costs. Neither can do the scheduling alone, so this module carries
//! one frame's motion across the bridge — pages rather than items, because
//! the engine has no idea what an item is, and windows rather than offsets,
//! because what a scheduler needs is "which pages are owed what", not the
//! pixels that decided it.
//!
//! Two calls, at two very different rates: [`configure_motion`] once per
//! document (the budget the memory ledger enforces), and
//! [`set_scroll_motion`] once per scroll frame (the coalesced one — the
//! adapter's rAF gate, not one per wheel event).

use crate::bridge;

use super::guard_pdf_reader;

/// The movement classification the engine schedules against.
///
/// A mirror of the virtualizer's `ScrollPhase`, spelled out here because the
/// two crates do not depend on each other and the bridge should not make one
/// depend on the other for five names. The spelling is not pinned by a unit
/// test that could drift with it: `engine_contract.rs` under this crate's
/// `tests` reads the engine's own table out of `public/engine/motion.ts` and
/// fails when the two disagree — the same trick the facade contract uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionPhase {
    /// Not moving: everything the reader can see is owed full quality, and
    /// the lanes are free to spend themselves on the ring around it.
    #[default]
    Idle,
    /// Reading speed.
    Slow,
    /// Ordinary scrolling.
    Normal,
    /// Covering ground: pages arrive faster than they can be read.
    Fast,
    /// A throw: the destination matters and the pages between do not.
    Fling,
}

impl MotionPhase {
    /// The wire spelling the engine parses.
    pub const fn wire(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Slow => "slow",
            Self::Normal => "normal",
            Self::Fast => "fast",
            Self::Fling => "fling",
        }
    }

    /// Whether pages the reader is flying past are owed nothing more
    /// expensive than a preview raster.
    pub const fn is_sweeping(self) -> bool {
        matches!(self, Self::Fast | Self::Fling)
    }
}

/// One page range, 1-based and inclusive. `None` is the whole document — a
/// caller with no window to publish (a paginated mode, a strip that has not
/// measured yet) leaves the tier open rather than closing it.
pub type PageRange = Option<(u32, u32)>;

/// One scroll frame, in the engine's terms.
///
/// Every page number is 1-based, matching the engine's own page numbering;
/// the caller converts from the virtualizer's 0-based item indices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMotion {
    /// How fast the reader is moving, classified. The classification IS the
    /// speed on this side of the bridge: the raw pixels-per-second number has
    /// nothing left to decide once the phase is known, so it does not cross.
    pub phase: MotionPhase,
    /// Sign of the speed: `1` forward through the document, `-1` back, `0` at
    /// rest. The scheduler uses it to weight pages behind the reader down —
    /// a page already read is worth less than one about to be reached.
    pub direction: i8,
    /// The page the reader is projected to reach. The one page that outranks
    /// its neighbours even when no window includes it.
    pub predicted_page: u32,
    /// The full-quality window.
    pub full: PageRange,
    /// The preview window: a superset of [`Self::full`].
    pub preview: PageRange,
    /// How long a page mounted this frame should wait before its render is
    /// queued, milliseconds. Not a sleep — a job superseded inside this
    /// window never runs at all.
    pub delay_ms: u32,
    /// How many raster lanes to run. Fewer while sweeping: the goal there is
    /// the shortest time to the first useful page, not the most pages
    /// started.
    pub workers: u32,
}

impl ScrollMotion {
    /// A reader who is not moving and has published no windows: everything is
    /// owed full quality and nothing is being paced. The engine's own
    /// starting state, so a document opened into a paginated mode (which
    /// never publishes motion) behaves exactly as it did before motion
    /// existed.
    pub const fn idle() -> Self {
        Self {
            phase: MotionPhase::Idle,
            direction: 0,
            predicted_page: 0,
            full: None,
            preview: None,
            delay_ms: 0,
            workers: 0,
        }
    }
}

impl Default for ScrollMotion {
    fn default() -> Self {
        Self::idle()
    }
}

/// Publish one frame's motion to the engine's render scheduler.
///
/// Fire-and-forget and silent without the engine (a shelf-side call, a
/// document that never opened): the scheduler's own default is the settled
/// one, so a missed publication costs an optimisation, never correctness.
pub fn set_scroll_motion(motion: &ScrollMotion) {
    if !guard_pdf_reader() {
        return;
    }
    let (first_full, last_full) = motion.full.unwrap_or((0, 0));
    let (first_preview, last_preview) = motion.preview.unwrap_or((0, 0));
    bridge::set_scroll_motion(
        motion.phase.wire(),
        motion.direction,
        motion.predicted_page,
        first_full,
        last_full,
        first_preview,
        last_preview,
        motion.delay_ms,
        motion.workers,
    );
}

/// What the reader is willing to spend on rasters, and how the tiers share
/// it. Set once per document, before any render is queued.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionBudget {
    /// Total raster budget, bytes: full surfaces, preview surfaces and the
    /// thumbnail cache together.
    pub max_bytes: f64,
    /// The preview tier's own ceiling, bytes — the tier a long fling grows,
    /// and the one that must not spend the full tier's money.
    pub preview_bytes: f64,
    /// Hard ceiling on simultaneously held preview surfaces, whatever they
    /// cost: the byte budget's backstop for a document of enormous pages.
    pub max_preview_pages: u32,
    /// The output-scale multiplier a preview raster is rendered at. Below
    /// about a quarter the page stops reading as the page; above about a half
    /// it stops being cheap.
    pub preview_scale: f64,
}

impl Default for MotionBudget {
    fn default() -> Self {
        Self {
            max_bytes: 96.0 * 1024.0 * 1024.0,
            preview_bytes: 24.0 * 1024.0 * 1024.0,
            max_preview_pages: 8,
            preview_scale: 0.45,
        }
    }
}

/// Publish the raster budget the engine's memory ledger enforces.
pub fn configure_motion(budget: &MotionBudget) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::configure_motion(
        budget.max_bytes,
        budget.preview_bytes,
        budget.max_preview_pages,
        budget.preview_scale,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_spellings_are_the_five_the_engine_parses() {
        let wires: Vec<&str> = [
            MotionPhase::Idle,
            MotionPhase::Slow,
            MotionPhase::Normal,
            MotionPhase::Fast,
            MotionPhase::Fling,
        ]
        .iter()
        .map(|phase| phase.wire())
        .collect();
        assert_eq!(wires, ["idle", "slow", "normal", "fast", "fling"]);
    }

    #[test]
    fn an_idle_motion_publishes_no_windows() {
        let motion = ScrollMotion::idle();
        assert_eq!(motion.phase, MotionPhase::default());
        assert!(motion.full.is_none() && motion.preview.is_none());
        assert_eq!(motion.delay_ms, 0);
        assert!(!motion.phase.is_sweeping());
        assert_eq!(motion, ScrollMotion::default());
    }

    #[test]
    fn only_the_two_fast_phases_sweep() {
        assert!(!MotionPhase::Idle.is_sweeping());
        assert!(!MotionPhase::Slow.is_sweeping());
        assert!(!MotionPhase::Normal.is_sweeping());
        assert!(MotionPhase::Fast.is_sweeping());
        assert!(MotionPhase::Fling.is_sweeping());
    }

    #[test]
    fn the_budget_leaves_the_preview_tier_a_share_of_its_own() {
        let budget = MotionBudget::default();
        assert!(budget.preview_bytes < budget.max_bytes);
        assert!(budget.max_preview_pages > 0);
        // A preview must be materially cheaper than the raster it stands in
        // for, or the tier buys nothing.
        assert!(budget.preview_scale > 0.0 && budget.preview_scale < 0.75);
    }
}
