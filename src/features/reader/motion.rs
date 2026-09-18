//! The reader's motion policy, and the bridge that carries it to the engine.
//!
//! One scheduler is split across the wasm boundary, because the two halves
//! know different things. The virtualizer knows the geometry and the movement:
//! how fast the reader is going, which way, which page they are projected to
//! reach, and which pages are owed what. The engine knows what a raster costs
//! and holds the only queue in front of pdf.js. Neither can prioritise a render
//! on its own, so the strip publishes the frame and the engine spends its lanes
//! on it.
//!
//! What crosses is deliberately small and quantized: a phase, a sign, five page
//! numbers, a delay and a lane count. No offsets (the engine has no geometry),
//! no raw velocity (the phase IS the speed, classified), and nothing that
//! changes on a frame where the answer did not — [`use_motion_bridge`] compares
//! the whole payload and stays quiet when a scroll moved the reader inside the
//! same page.

use leptos::prelude::*;
use virtual_list::Window;
use virtual_list_leptos::{AdaptivePolicy, RenderPlan, ScrollPhase, Virtualizer};

use pdf_engine::api as engine;
use pdf_engine::api::motion::{MotionBudget, MotionPhase, ScrollMotion};

/// The policy both scrolling strips run, and the source of every number the
/// engine is given about tiers and memory. One constant so the two sides of
/// the boundary cannot drift: the virtualizer's windows and the engine's
/// raster budget are the same policy, read twice.
pub(crate) const POLICY: AdaptivePolicy = AdaptivePolicy::reader();

/// The classified movement, spelled the way the engine parses it. The two
/// enums live in crates that do not depend on each other, and this match is
/// the one place they meet — `crates/pdf-engine/tests/engine_contract.rs` pins
/// the strings against the engine's own table.
fn wire_phase(phase: ScrollPhase) -> MotionPhase {
    match phase {
        ScrollPhase::Idle => MotionPhase::Idle,
        ScrollPhase::Slow => MotionPhase::Slow,
        ScrollPhase::Normal => MotionPhase::Normal,
        ScrollPhase::Fast => MotionPhase::Fast,
        ScrollPhase::Fling => MotionPhase::Fling,
    }
}

/// One item window as the engine reads it: 1-based inclusive page numbers.
fn wire_window(window: Option<Window>) -> Option<(u32, u32)> {
    window.map(|window| (window.first as u32 + 1, window.last as u32 + 1))
}

/// The frame's plan, translated. A plan with no mount window is a document
/// with no pages, and the honest translation of that is "no motion published"
/// rather than a prediction of page 1.
fn wire_motion(plan: RenderPlan) -> ScrollMotion {
    if plan.mount.is_none() {
        return ScrollMotion::idle();
    }
    ScrollMotion {
        phase: wire_phase(plan.phase),
        direction: plan.direction,
        predicted_page: plan.predicted_index as u32 + 1,
        full: wire_window(plan.full),
        preview: wire_window(plan.preview),
        delay_ms: plan.delay_ms,
        workers: plan.workers as u32,
    }
}

/// Publish the raster budget the engine's memory ledger enforces. Once per
/// document, from the open flow: the numbers are the policy's, and nothing
/// about them depends on which book is open.
pub(crate) fn publish_budget() {
    engine::configure_motion(&MotionBudget {
        max_bytes: POLICY.memory.max_bytes as f64,
        preview_bytes: POLICY.memory.preview_bytes as f64,
        max_preview_pages: POLICY.memory.max_preview_pages as u32,
        preview_scale: POLICY.rendering.preview_scale,
    });
}

/// Wire one strip's virtualizer to the engine's scheduler.
///
/// An effect on the plan, and a comparison before the call: a scroll frame
/// that moved the reader within the same page produces the same payload, and
/// publishing it again would only re-score a queue against motion that has not
/// changed. What does re-publish is the settle — which is the frame that
/// matters most, because it is the one that promotes the preview ring.
pub(crate) fn use_motion_bridge(virtualizer: &Virtualizer) {
    let plan = virtualizer.render_plan();
    let published = StoredValue::new_local(None::<ScrollMotion>);
    Effect::new(move |_| {
        let motion = wire_motion(plan.get());
        if published.get_value() == Some(motion) {
            return;
        }
        published.set_value(Some(motion));
        engine::set_scroll_motion(&motion);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use virtual_list_leptos::RenderQuality;

    fn window(first: usize, last: usize) -> Window {
        Window { first, last }
    }

    #[test]
    fn an_empty_document_publishes_no_motion() {
        // No mount window means no pages, and a prediction of "page 1" for a
        // document that has none would be a window the engine believes.
        let plan = RenderPlan {
            phase: ScrollPhase::Fling,
            direction: 1,
            predicted_index: 41,
            ..RenderPlan::default()
        };
        assert_eq!(wire_motion(plan), ScrollMotion::idle());
        assert_eq!(wire_motion(plan).phase, MotionPhase::Idle);
    }

    #[test]
    fn item_indices_cross_as_page_numbers() {
        let plan = RenderPlan {
            phase: ScrollPhase::Normal,
            direction: 1,
            predicted_index: 9,
            full: Some(window(8, 10)),
            preview: Some(window(5, 13)),
            mount: Some(window(2, 18)),
            delay_ms: 8,
            workers: 2,
            ..RenderPlan::default()
        };
        let motion = wire_motion(plan);
        // 0-based items become 1-based pages, and the window keeps its
        // inclusive ends — the off-by-one that would put the reader's own page
        // outside the window it is the centre of.
        assert_eq!(motion.predicted_page, 10);
        assert_eq!(motion.full, Some((9, 11)));
        assert_eq!(motion.preview, Some((6, 14)));
        assert_eq!(motion.delay_ms, 8);
        assert_eq!(motion.workers, 2);
        assert_eq!(motion.direction, 1);
    }

    #[test]
    fn every_phase_has_a_wire_spelling() {
        for (phase, want) in [
            (ScrollPhase::Idle, MotionPhase::Idle),
            (ScrollPhase::Slow, MotionPhase::Slow),
            (ScrollPhase::Normal, MotionPhase::Normal),
            (ScrollPhase::Fast, MotionPhase::Fast),
            (ScrollPhase::Fling, MotionPhase::Fling),
        ] {
            assert_eq!(wire_phase(phase), want);
        }
    }

    #[test]
    fn the_budget_is_the_policy_the_strips_run() {
        // The engine's ceiling and the virtualizer's tiers are one policy read
        // twice; if they ever disagree, the side that is wrong is the one that
        // stops matching this test.
        assert_eq!(POLICY.memory.max_bytes, 96 * 1024 * 1024);
        assert!(POLICY.memory.preview_bytes < POLICY.memory.max_bytes);
        assert!(POLICY.rendering.preview_scale > 0.0);
        assert!(POLICY.rendering.preview_screens > POLICY.rendering.full_screens);
        // The mount ceiling the strips are built with has to admit the preview
        // ring, or the ring is a policy for pages that are never mounted.
        assert!(POLICY.memory.max_preview_pages <= reader_core::view::MOUNTED_PAGES);
    }

    #[test]
    fn the_plan_the_bridge_publishes_tiers_its_own_pages() {
        // The end-to-end claim, on the pure half of it: a plan for a reader
        // moving down gives the page ahead of them a better tier than the one
        // behind, and the page they are predicted to land on is full quality
        // even when it sits past the full tier.
        let plan = RenderPlan {
            phase: ScrollPhase::Fast,
            direction: 1,
            predicted_index: 12,
            visible: Some(window(9, 10)),
            full: Some(window(9, 11)),
            preview: Some(window(8, 14)),
            mount: Some(window(4, 18)),
            ..RenderPlan::default()
        };
        assert_eq!(plan.quality(9), RenderQuality::Full);
        assert_eq!(plan.quality(11), RenderQuality::Full);
        assert_eq!(plan.quality(13), RenderQuality::Preview);
        assert_eq!(plan.quality(5), RenderQuality::Placeholder);
        assert_eq!(wire_motion(plan).predicted_page, 13);
    }
}
