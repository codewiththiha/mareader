//! Reveal a saved vocabulary occurrence without opening the search bar.

use std::ops::Range;

use egui::{Context, Id, Stroke, StrokeKind};
use mareader_library::{BookId, Spot};
use mareader_workspace::PaneId;

use super::*;
use crate::z_order::{self, Tier};

/// The source stays with the spot so stale coordinates cannot point elsewhere.
pub(super) struct Location {
    book: BookId,
    spot: Spot,
    source: String,
    requested: f64,
    resolved: Option<Resolved>,
    visible_at: Option<f64>,
}

struct Resolved {
    pane: PaneId,
    doc: u64,
    spans: Vec<(usize, Range<usize>)>,
}

impl Location {
    pub fn new(book: BookId, spot: Spot, source: String, requested: f64) -> Self {
        Self {
            book,
            spot,
            source,
            requested,
            resolved: None,
            visible_at: None,
        }
    }
}

impl MareaderApp {
    pub(super) fn show_vocab_location(&mut self, ctx: &Context) {
        let Some(mut location) = self.ui.vocab_location.take() else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if self.route() != Route::Reader || now - location.requested > 30.0 {
            return;
        }
        let pane = self.active();
        let Some(reading) = self.documents().reading(pane) else {
            self.ui.vocab_location = Some(location);
            ctx.request_repaint();
            return;
        };
        if self.session.book_in(pane) != Some(location.book) || !reading.document.is_complete() {
            self.ui.vocab_location = Some(location);
            ctx.request_repaint();
            return;
        }
        if location.resolved.is_none() {
            let spans = mareader_engine::skills::resolve(
                &reading.document,
                location.spot,
                &location.source,
            );
            let Some(spans) = spans else {
                self.ui.toasts.push(Toast::warning(
                    "The saved word location no longer matches this book.",
                ));
                return;
            };
            location.resolved = Some(Resolved {
                pane,
                doc: reading.doc.get(),
                spans,
            });
        }
        let Some(resolved) = &location.resolved else {
            return;
        };
        if resolved.pane != pane || resolved.doc != reading.doc.get() {
            return;
        }
        let Some((page, chars)) = resolved.spans.first().cloned() else {
            return;
        };
        if location.visible_at.is_none() {
            self.ui
                .documents
                .reveal_hit(&mut self.session, pane, page, chars);
        }
        let docs = self.documents();
        let Some((_, room)) = self.ui.pane_rects.iter().find(|(p, _)| *p == pane) else {
            self.ui.vocab_location = Some(location);
            ctx.request_repaint();
            return;
        };
        draw(
            ctx,
            docs,
            pane,
            *room,
            &resolved.spans,
            &mut location.visible_at,
            now,
        );
        if location.visible_at.is_some_and(|at| now - at >= 6.0) {
            return;
        }
        self.ui.vocab_location = Some(location);
        ctx.request_repaint_after_secs(1.0 / 30.0);
    }
}

/// Paint just the resolved word's text boxes with the theme accent.
fn draw(
    ctx: &Context,
    docs: DocumentsView<'_>,
    pane: PaneId,
    room: Rect,
    spans: &[(usize, Range<usize>)],
    visible_at: &mut Option<f64>,
    now: f64,
) {
    let tokens = theme::Tokens::get(ctx);
    let painter = z_order::painter(ctx, Tier::PaneMark, Id::new("vocab-location"));
    z_order::register(ctx, Tier::PaneMark, painter.layer_id());
    for sheet in docs.sheets(pane) {
        let clip = room.intersect(sheet.shown);
        if !clip.is_positive() {
            continue;
        }
        let Some((_, range)) = spans.iter().find(|(p, _)| *p == sheet.page) else {
            continue;
        };
        let Some(layer) = docs.cached_layer(pane, sheet.page) else {
            continue;
        };
        let painter = painter.with_clip_rect(clip);
        for bounds in layer.text.line_boxes(range.clone()) {
            let rect = sheet.to_screen(bounds);
            if !rect.is_finite() || !rect.is_positive() || !clip.intersects(rect) {
                continue;
            }
            let since = *visible_at.get_or_insert(now);
            let strength = ((6.0 - (now - since)) / 2.0).clamp(0.0, 1.0) as f32;
            for (expand, width, alpha) in [(3.0, 1.5, 0.95), (5.0, 3.0, 0.30), (7.0, 5.0, 0.10)] {
                painter.rect_stroke(
                    rect.expand(expand),
                    4.0,
                    Stroke::new(width, tokens.accent.gamma_multiply(alpha * strength)),
                    StrokeKind::Middle,
                );
            }
        }
    }
}
