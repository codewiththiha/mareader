//! A peek at where a link inside the book goes: the page small and its
//! first words, after a short hover, so a jump is chosen knowingly.

use egui::{Id, Margin, RichText, Sense, Stroke};

use super::*;
use crate::bubble::{Bubble, Side};

/// Seconds a pointer rests on a link before its peek shows.
const DELAY: f64 = 0.4;
/// The page's width in the card; its height follows the page.
const THUMB_W: f32 = 84.0;
const THUMB_MAX_H: f32 = 120.0;
/// The card's content width: the page and the words beside it.
const WIDTH: f32 = 300.0;
/// Characters of the page's text shown at most.
const CHARS: usize = 200;
/// A page's size while it isn't known: US Letter.
const LETTER: (f32, f32) = (612.0, 792.0);

/// Shows the peek for the page link under the pointer, once it has rested.
pub(super) fn show(cx: &mut ShowContext<'_>) {
    let id = Id::new(("link-peek", cx.place.doc));
    let hovered = match &cx.paint.did.hover {
        Some((LinkTarget::Page(page), at)) => Some((*page, *at)),
        _ => None,
    };
    let pressed = cx.ctx.input(|i| i.pointer.any_down());
    let Some((page, at)) = hovered.filter(|_| !pressed && cx.paint.follow.is_none()) else {
        cx.ctx.data_mut(|d| d.remove::<(usize, f64)>(id));
        return;
    };
    let now = cx.now;
    let since = cx.ctx.data_mut(|d| {
        let rested = d.get_temp_mut_or_insert_with(id, || (page, now));
        if rested.0 != page {
            *rested = (page, now);
        }
        rested.1
    });
    let wait = DELAY - (now - since);
    if wait > 0.0 {
        let wait = std::time::Duration::from_secs_f64(wait);
        cx.ctx.request_repaint_after(wait);
        return;
    }
    card(cx, (id.with(since.to_bits()), page, at));
}

/// The card: the page small on the left, its number and first words beside.
fn card(cx: &mut ShowContext<'_>, (id, page, at): (Id, usize, Rect)) {
    let t = cx.view_cx.tokens;
    let size = cx.sizes.get(page).map_or(LETTER, |s| (s.width, s.height));
    let tall = size.1 / size.0.max(1.0);
    let thumb_h = (THUMB_W * tall).min(THUMB_MAX_H);
    let thumb = vec2(thumb_h / tall.max(0.01), thumb_h);
    let words = first_words(cx, page);
    let mut room = cx.frame;
    room.min.y += cx.view_cx.top_inset;
    let bubble = Bubble {
        id,
        at,
        room,
        width: WIDTH,
        margin: Margin::same(10),
        snug: false,
        prefer: Side::Below,
    };
    let ctx = cx.ctx.clone();
    bubble.show(&ctx, &t, |ui, _| {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let (rect, _) = ui.allocate_exact_size(thumb, Sense::hover());
            paint_thumb(cx, ui, (page, rect, size.0));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                let title = format!("Page {}", page + 1);
                let title = RichText::new(title).font(crate::theme::medium(12.0));
                ui.label(title.color(t.ink));
                let said = words.unwrap_or_default();
                ui.label(RichText::new(said).size(t.text_small).color(t.muted));
            });
        });
    });
}

/// The target page at `rect`, asked for at the card's size until it comes.
fn paint_thumb(cx: &mut ShowContext<'_>, ui: &Ui, (page, rect, width): (usize, Rect, f32)) {
    let ppp = cx.ctx.pixels_per_point();
    let key = PageKey {
        doc: cx.place.doc,
        page,
        scale: Scale::covering(rect.width() / width.max(1.0) * ppp),
        part: Part::Whole,
    };
    let look = cx.pages.lookup(key);
    if !matches!(look, Lookup::Sharp(_) | Lookup::Failed) {
        cx.demand.push((Urgency::Thumbnail, key.into()));
        cx.ctx.request_repaint();
    }
    let document = Arc::as_ref(cx.doc);
    let tones = (look, cx.view_cx.tokens, cx.view_cx.page);
    let quads = crate::thumbnails::small(ui, (document, page), (rect, width), tones);
    if let (Some(pass), false) = (cx.pass, quads.is_empty()) {
        let shape = PagesCallback::shape(pass.clone(), ui.clip_rect(), quads);
        ui.painter().add(shape);
    }
    let edge = Stroke::new(1.0, cx.view_cx.tokens.line);
    ui.painter()
        .rect_stroke(rect, 2.0, edge, egui::StrokeKind::Outside);
}

/// The page's first words, asked for while they aren't in.
fn first_words(cx: &mut ShowContext<'_>, page: usize) -> Option<String> {
    let doc = cx.place.doc;
    let Some(layer) = cx.texts.get(doc, page) else {
        if !cx.texts.touch(doc, page) {
            cx.demand.push((Urgency::Text, Want::Text { doc, page }));
        }
        cx.ctx.request_repaint();
        return None;
    };
    let text = layer.text.text(0..layer.text.len().min(CHARS * 2));
    Some(clip(&text, CHARS))
}

/// `text`'s whole words up to `most` characters, an ellipsis if cut.
fn clip(text: &str, most: usize) -> String {
    let mut said = String::new();
    for word in text.split_whitespace() {
        if said.chars().count() + word.chars().count() >= most {
            said.push('…');
            break;
        }
        if !said.is_empty() {
            said.push(' ');
        }
        said.push_str(word);
    }
    said
}

#[cfg(test)]
mod tests {
    use super::clip;

    #[test]
    fn a_long_page_is_clipped_at_a_whole_word() {
        assert_eq!(clip("one two  three\nfour", 12), "one two…");
        assert_eq!(clip("short", 12), "short");
    }
}
