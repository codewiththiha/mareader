//! Reveal a saved vocabulary occurrence without opening the search bar.

use std::ops::Range;

use egui::{Context, Id};
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
    cache: Vec<crate::glow::outline::Cache>,
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
            cache: Vec::new(),
        }
    }
}

impl MareaderApp {
    pub(super) fn prepare_note_location(
        &mut self,
        ctx: &Context,
        book: BookId,
        mark: mareader_library::MarkId,
    ) {
        self.ui.vocab_location = self
            .session
            .library_model()
            .mark(mark)
            .filter(|m| m.book.value == book && m.kind == mareader_library::MarkKind::Highlight)
            .map(|m| Location::new(book, m.spot, m.word.clone(), ctx.input(|i| i.time)));
    }

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
            let spans = text_span(&reading.document, location.spot).and_then(|spot| {
                mareader_engine::skills::resolve(&reading.document, spot, &location.source)
            });
            let Some(spans) = spans else {
                self.ui.toasts.push(Toast::warning(
                    "The saved text location no longer matches this book.",
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
            (&mut location.visible_at, &mut location.cache),
            now,
        );
        if location.visible_at.is_some_and(|at| now - at >= 6.0) {
            return;
        }
        self.ui.vocab_location = Some(location);
        if crate::motion::enabled(ctx) {
            ctx.request_repaint_after_secs(1.0 / 60.0);
        } else {
            ctx.request_repaint_after_secs(0.2);
        }
    }
}

/// Turn old text and page-box marks into source-validated text spans.
fn text_span(doc: &mareader_session::Document, spot: Spot) -> Option<Spot> {
    match spot {
        Spot::Text { block, start, end } => Some(Spot::Blocks {
            from: [block, start],
            to: [block, end],
        }),
        Spot::Page {
            page,
            rect: [x, y, w, h],
        } => {
            if ![x, y, w, h, x + w, y + h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
                return None;
            }
            let layer = doc.text_layer(page).ok()?;
            let in_box = |c: &mareader_pdf::TextChar| {
                let b = c.bounds;
                let (cx, cy) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
                (x..=x + w).contains(&cx) && (y..=y + h).contains(&cy)
            };
            let chars = layer.text.chars();
            let from = chars.iter().position(in_box)?;
            let to = chars.iter().rposition(in_box)?.checked_add(1)?;
            Some(Spot::Chars {
                from: [page, from],
                to: [page, to],
            })
        }
        other => Some(other),
    }
}

/// Paint a traveling accent flare along the selected text's contour.
fn draw(
    ctx: &Context,
    docs: DocumentsView<'_>,
    pane: PaneId,
    room: Rect,
    spans: &[(usize, Range<usize>)],
    state: (&mut Option<f64>, &mut Vec<crate::glow::outline::Cache>),
    now: f64,
) {
    let (visible_at, cache) = state;
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
        let lines: Vec<_> = layer
            .text
            .line_boxes(range.clone())
            .map(|b| sheet.to_screen(b))
            .filter(|r| r.is_finite() && r.is_positive() && clip.expand(80.0).intersects(*r))
            .take(256)
            .collect();
        if !lines.iter().any(|r| clip.intersects(*r)) {
            continue;
        }
        let since = *visible_at.get_or_insert(now);
        let strength = if crate::motion::enabled(ctx) {
            ((6.0 - (now - since)) / 2.0).clamp(0.0, 1.0) as f32
        } else {
            1.0
        };
        let painter = painter.with_clip_rect(clip);
        for (group_index, group) in crate::skill_outline::shape::groups(&lines)
            .into_iter()
            .enumerate()
        {
            let points = crate::skill_outline::shape::outline(group);
            if let Some(line) = group.first() {
                let outline = crate::glow::Outline::path(
                    &points,
                    line.expand(0.5),
                    cache,
                    (sheet.page, group_index),
                    now - since,
                    1.0,
                );
                crate::glow::paint_accent(
                    ctx,
                    &painter,
                    &tokens,
                    outline,
                    0.0,
                    crate::glow::Glow {
                        strength,
                        whole: 0.0,
                    },
                );
            }
        }
    }
}
