//! The pill over a studying pane's bottom: where the reader is, how the next
//! deck is coming along, and the way to it.
//!
//! It shows while the gate is out of reach, sitting where page AI's pill
//! sits, above the page bar; page AI's runs stop while a pane studies, so
//! the two never meet.

use egui::{Align2, Context, CornerRadius, Frame, Id, Margin, RichText, Stroke, pos2};
use mareader_engine::study_mode::{Action, DeckState, Run};
use mareader_study::mode::{Batch, Via};

use super::Pane;
use super::deck::send;
use crate::theme::Tokens;
use crate::ui_command::Commands;
use crate::widgets;
use crate::z_order::{self, Tier};

/// The pill's button, for screen readers and tests.
pub const OPEN: &str = "Open words";
/// Space kept between the pill and the page bar, or the veil.
const LIFT: f32 = 12.0;
/// Room above the pill a pane needs to show it.
const ROOM: f32 = 48.0;

/// How the deck at the gate is coming along, and whether it is still busy.
pub fn status(deck: &DeckState) -> (&'static str, bool) {
    match deck {
        DeckState::None => ("next words: waiting", true),
        DeckState::Making { job, .. } if job.via() == Via::Ai => ("next words: writing", true),
        DeckState::Making { .. } => ("next words: looking up", true),
        DeckState::Failed { .. } => ("next words: failed", false),
        DeckState::Open(_) => ("next words ready", false),
    }
}

/// The batch the reader is in, as `batch 2/5 · page 1/3`.
pub fn place(run: &Run, current: usize) -> Option<String> {
    let batch = run
        .batches
        .iter()
        .find(|b| (b.from..=b.to).contains(&current))?;
    let count = run.batches.len();
    let index = batch.index.saturating_add(1);
    let page = current.saturating_sub(batch.from).saturating_add(1);
    let pages = pages(batch);
    Some(format!(
        "Proactive · batch {index}/{count} · page {page}/{pages}"
    ))
}

fn pages(batch: &Batch) -> usize {
    batch.to.saturating_sub(batch.from).saturating_add(1)
}

/// The pill at the bottom of `pane`, above the veil's top when one shows.
pub fn show(ctx: &Context, pane: &Pane<'_>, veil_top: Option<f32>, out: &mut Commands) {
    let floor = pane.rect.bottom() - crate::page_bar::BAR_H - LIFT;
    let bottom = veil_top.map_or(floor, |top| floor.min(top - LIFT));
    if pane.run.gate_batch().is_none() || bottom < pane.rect.top() + ROOM {
        return;
    }
    let t = Tokens::get(ctx);
    let at = pos2(pane.rect.center().x, bottom);
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::symmetric(12, 4));
    let shown = z_order::area(ctx, Tier::PageCard, Id::new(("study-pill", pane.id)))
        .pivot(Align2::CENTER_BOTTOM)
        .fixed_pos(at)
        .constrain(false)
        .show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_max_width(pane.rect.width() - 16.0);
                ui.horizontal(|ui| {
                    if let Some(place) = place(pane.run, pane.current) {
                        ui.label(RichText::new(place).size(t.text_small).color(t.ink));
                    }
                    let (said, busy) = status(&pane.run.deck);
                    if busy {
                        ui.add(egui::Spinner::new().size(10.0));
                    }
                    ui.label(RichText::new(said).size(t.text_small).color(t.muted));
                    let open = widgets::primary_button(ui, None, OPEN, t.accent, &t);
                    if ui.add(open).clicked() {
                        send(out, Action::NextBatch(pane.id));
                    }
                });
            });
        });
    // Toasts give the way to the deck the room.
    crate::toast::keep_clear(ctx, shown.response.rect);
}
