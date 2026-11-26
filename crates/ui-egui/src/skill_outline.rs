//! A temporary, text-shaped outline for a selection skill in flight.

use egui::{Context, Id, Rect};
use mareader_engine::activity::Key;
use mareader_workspace::PaneId;

mod feedback;
mod shape;

use crate::documents::DocumentsView;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

const MAX_LINES: usize = 256;

pub(crate) use feedback::{clear, show};

/// Draw the visible pages of a selection, reporting its visual while working.
fn paint(
    ctx: &Context,
    place: &mut feedback::Place,
    panes: &[(PaneId, Rect)],
    docs: DocumentsView<'_>,
    (working, glow): (bool, crate::glow::Glow),
) {
    let Some(room) = panes
        .iter()
        .find(|(p, _)| *p == place.pane)
        .map(|(_, r)| *r)
    else {
        return;
    };
    let tokens = Tokens::get(ctx);
    let painter = z_order::painter(ctx, Tier::PaneMark, Id::new(("skill-outline", place.id.0)));
    z_order::register(ctx, Tier::PaneMark, painter.layer_id());
    let mut visible = false;
    for sheet in docs.sheets(place.pane) {
        let clip = room.intersect(sheet.shown);
        if !clip.is_positive() {
            continue;
        }
        let Some((_, range)) = place.spans.iter().find(|(page, _)| *page == sheet.page) else {
            continue;
        };
        let Some(layer) = docs.cached_layer(place.pane, sheet.page) else {
            continue;
        };
        // Keep the contour's ends beyond the viewport.
        let near = clip.expand(80.0);
        let lines: Vec<_> = layer
            .text
            .line_boxes(range.clone())
            .map(|b| sheet.to_screen(b))
            .filter(|r| r.is_finite() && r.is_positive() && near.intersects(*r))
            .take(MAX_LINES)
            .collect();
        let painter = painter.with_clip_rect(clip);
        for (group_index, group) in shape::groups(&lines).into_iter().enumerate() {
            let points = shape::outline(group);
            if let Some(line) = group.first() {
                visible |= crate::glow::selection::paint(
                    (ctx, &painter, &tokens),
                    &points,
                    line.expand(0.5),
                    (glow, place.clock.travel),
                    &mut place.cache,
                    (sheet.page, group_index),
                );
            }
        }
    }
    if working && visible {
        crate::activity::seen::in_view(
            ctx,
            Key::Skill {
                pane: place.pane,
                ask: place.id,
            },
        );
    }
}
