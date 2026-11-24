//! Which jobs show their own work on screen this pass.
//!
//! A job's own visual, such as the deck's waiting card, stamps its key as
//! it paints; the stack leaves out what was stamped this pass or the last,
//! so a job shows in the corner only while its own visual is out of view.
//! A word's glow stamps its mark, which every pane's explain of it shares.

use std::collections::HashMap;

use egui::{Context, Id, Rect};
use mareader_engine::activity::Key;
use mareader_engine::study::state::Key as Ask;
use mareader_engine::study_mode::{DeckState, Run};
use mareader_library::MarkId;
use mareader_study::mode::Batch;
use mareader_workspace::PaneId;

type Stamps<K> = HashMap<K, u64>;

fn stamps_id() -> Id {
    Id::new("activity-in-view")
}

fn glows_id() -> Id {
    Id::new("activity-glowing")
}

fn stamp<K: std::hash::Hash + Eq + Send + Sync + Clone + 'static>(ctx: &Context, id: Id, key: K) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let stamps = d.get_temp_mut_or_default::<Stamps<K>>(id);
        stamps.retain(|_, at| *at + 1 >= pass);
        stamps.insert(key, pass);
    });
}

fn stamped<K: std::hash::Hash + Eq + Send + Sync + Clone + 'static>(
    ctx: &Context,
    id: Id,
    key: &K,
) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let stamps: Option<Stamps<K>> = ctx.data(|d| d.get_temp(id));
    stamps
        .and_then(|s| s.get(key).copied())
        .is_some_and(|at| at + 1 >= pass)
}

/// Says `key`'s own visual painted on screen this pass.
pub fn in_view(ctx: &Context, key: Key) {
    stamp(ctx, stamps_id(), key);
}

/// Says `mark`'s words glow on screen this pass, while the AI explains them.
pub fn glowing(ctx: &Context, mark: MarkId) {
    stamp(ctx, glows_id(), mark);
}

/// Whether `key`'s own visual painted this pass or the last.
pub fn shown(ctx: &Context, key: Key) -> bool {
    let glows = match key {
        Key::Explain {
            ask: Ask::Mark(mark),
            ..
        } => stamped(ctx, glows_id(), &mark),
        _ => false,
    };
    glows || stamped(ctx, stamps_id(), &key)
}

/// The deck drew at `rect` in `pane`: its job is in view, and the stack
/// keeps clear of it.
pub fn deck(ctx: &Context, (pane, run): (PaneId, &Run), rect: Rect) {
    if let Some(from) = shown_batch(run) {
        in_view(ctx, Key::Cards { pane, from });
    }
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(taken_id(pane), (pass, rect)));
}

/// Where `pane`'s deck drew this pass or the last; none once it is gone.
pub fn taken(ctx: &Context, pane: PaneId) -> Option<Rect> {
    let pass = ctx.cumulative_pass_nr();
    let kept: Option<(u64, Rect)> = ctx.data(|d| d.get_temp(taken_id(pane)));
    kept.filter(|(at, _)| at + 1 >= pass).map(|(_, rect)| rect)
}

fn taken_id(pane: PaneId) -> Id {
    Id::new(("activity-taken", pane))
}

/// The first page of the batch whose deck shows: open, waiting or failed.
fn shown_batch(run: &Run) -> Option<usize> {
    if let Some(open) = run.shown_deck() {
        return Some(open.batch.from);
    }
    match &run.deck {
        DeckState::Making { batch, .. } | DeckState::Failed { batch, .. } => Some(batch.from),
        DeckState::None | DeckState::Open(_) => run.gate_batch().map(|b: &Batch| b.from),
    }
}
