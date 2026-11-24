//! Which jobs show their own work on screen this pass.
//!
//! A job's own visual, such as the deck's waiting card, stamps its key as
//! it paints; the stack leaves out what was stamped this pass or the last,
//! so a job shows in the corner only while its own visual is out of view.

use std::collections::HashMap;

use egui::{Context, Id, Rect};
use mareader_engine::activity::Key;
use mareader_engine::study_mode::{DeckState, Run};
use mareader_study::mode::Batch;
use mareader_workspace::PaneId;

type Stamps = HashMap<Key, u64>;

fn stamps_id() -> Id {
    Id::new("activity-in-view")
}

/// Says `key`'s own visual painted on screen this pass.
pub fn in_view(ctx: &Context, key: Key) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let stamps = d.get_temp_mut_or_default::<Stamps>(stamps_id());
        stamps.retain(|_, at| *at + 1 >= pass);
        stamps.insert(key, pass);
    });
}

/// Whether `key`'s own visual painted this pass or the last.
pub fn shown(ctx: &Context, key: Key) -> bool {
    let pass = ctx.cumulative_pass_nr();
    let stamps: Option<Stamps> = ctx.data(|d| d.get_temp(stamps_id()));
    stamps
        .and_then(|s| s.get(&key).copied())
        .is_some_and(|at| at + 1 >= pass)
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
