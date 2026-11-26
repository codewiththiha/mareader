//! Keeps selection places through the shared glow's exit animation.

use std::ops::Range;

use egui::{Context, Id, Rect};
use mareader_engine::skills::{Anchor, Ask, AskId};
use mareader_workspace::PaneId;

use crate::documents::DocumentsView;

#[derive(Clone)]
pub(super) struct Place {
    pub id: AskId,
    pub pane: PaneId,
    pub doc: u64,
    pub spans: Vec<(usize, Range<usize>)>,
    pub clock: Clock,
}

#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub travel: f64,
    last: f64,
    working: bool,
}

impl Clock {
    fn new(now: f64, working: bool) -> Self {
        Self { travel: now, last: now, working }
    }

    fn advance(&mut self, now: f64, working: bool) {
        if self.working {
            self.travel += (now - self.last).max(0.0);
        }
        self.last = now;
        self.working = working;
    }
}

fn memory() -> Id {
    Id::new("skill-outline-places")
}

/// Draws running places and lets finished or closed ones flare out.
pub(crate) fn show(ctx: &Context, asks: &[Ask], panes: &[(PaneId, Rect)], docs: DocumentsView<'_>) {
    let mut places = ctx
        .data(|d| d.get_temp::<Vec<Place>>(memory()))
        .unwrap_or_default();
    let now = ctx.input(|i| i.time);
    for ask in asks.iter().filter(|a| !a.answer.landed() || a.open) {
        if let Anchor::Text(spans) = &ask.anchor {
            if let Some(place) = places.iter_mut().find(|p| p.id == ask.id) {
                place.pane = ask.pane;
                place.doc = ask.doc;
                place.spans.clone_from(spans);
            } else {
                places.push(Place {
                    id: ask.id,
                    pane: ask.pane,
                    doc: ask.doc,
                    spans: spans.clone(),
                    clock: Clock::new(now, !ask.answer.landed()),
                });
            }
        }
    }
    places.retain_mut(|place| {
        if docs
            .reading(place.pane)
            .is_none_or(|r| r.doc.get() != place.doc)
        {
            return false;
        }
        let ask = asks.iter().find(|a| a.id == place.id);
        let working = ask.is_some_and(|a| !a.answer.landed());
        let focused = ask.is_some_and(|a| a.open && a.answer.landed());
        let active = working || focused;
        place.clock.advance(now, working);
        let id = Id::new(("skill-glow", place.id.0));
        let glow = crate::glow::state(ctx, id, active);
        if let Some(mut glow) = glow {
            let whole = ctx.animate_bool_with_time(
                id.with("focus-whole"),
                focused || !active,
                crate::motion::secs(ctx, 0.3),
            );
            glow.whole = glow.whole.max(whole);
            super::paint(ctx, place, panes, docs, (working, glow));
            if !active {
                ctx.request_repaint_after_secs(1.0 / 60.0);
            }
        }
        active || glow.is_some()
    });
    ctx.data_mut(|d| d.insert_temp(memory(), places));
}

/// A different route cannot retain a stale selection visual.
pub(crate) fn clear(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Vec<Place>>(memory()));
}

#[cfg(test)]
mod tests {
    use super::Clock;

    #[test]
    fn travel_stops_when_work_finishes_and_resumes_on_retry() {
        let mut clock = Clock::new(10.0, true);
        clock.advance(11.0, true);
        assert_eq!(clock.travel, 11.0);
        clock.advance(11.5, false);
        assert_eq!(clock.travel, 11.5);
        clock.advance(15.0, false);
        assert_eq!(clock.travel, 11.5);
        clock.advance(16.0, true);
        assert_eq!(clock.travel, 11.5);
        clock.advance(17.0, true);
        assert_eq!(clock.travel, 12.5);
    }
}
