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
}

fn memory() -> Id {
    Id::new("skill-outline-places")
}

/// Draws running places and lets finished or closed ones flare out.
pub(crate) fn show(ctx: &Context, asks: &[Ask], panes: &[(PaneId, Rect)], docs: DocumentsView<'_>) {
    let mut places = ctx.data(|d| d.get_temp::<Vec<Place>>(memory())).unwrap_or_default();
    for ask in asks.iter().filter(|a| !a.answer.landed()) {
        if let Anchor::Text(spans) = &ask.anchor
            && !places.iter().any(|p| p.id == ask.id)
        {
            places.push(Place {
                id: ask.id,
                pane: ask.pane,
                doc: ask.doc,
                spans: spans.clone(),
            });
        }
    }
    places.retain(|place| {
        if docs.reading(place.pane).is_none_or(|r| r.doc.get() != place.doc) {
            return false;
        }
        let working = asks.iter().any(|a| a.id == place.id && !a.answer.landed());
        let glow = crate::glow::state(ctx, Id::new(("skill-glow", place.id.0)), working);
        if let Some(glow) = glow {
            super::paint(ctx, place, panes, docs, (working, glow));
            if !working {
                ctx.request_repaint_after_secs(1.0 / 60.0);
            }
        }
        working || glow.is_some()
    });
    ctx.data_mut(|d| d.insert_temp(memory(), places));
}

/// A different route cannot retain a stale selection visual.
pub(crate) fn clear(ctx: &Context) {
    ctx.data_mut(|d| d.remove::<Vec<Place>>(memory()));
}
