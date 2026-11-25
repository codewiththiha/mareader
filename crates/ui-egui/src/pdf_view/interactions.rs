//! Page-level interactive overlays: pictures, footnotes, bubbles, copy, bars.

use super::*;

/// A book's picture, or a PDF's figure, under a double click at `p`.
pub(super) fn open_picture(
    ctx: &egui::Context,
    (sheets, texts, id): (&[Sheet], &TextLayers, DocId),
    doc: &Arc<Document>,
    p: Pos2,
) -> Option<crate::image_viewer::Viewer> {
    use crate::image_viewer::Viewer;
    let sheet = sheets.iter().find(|s| s.shown.contains(p))?;
    let (x, y) = sheet.to_page(p);
    if let Some(picture) = doc.picture_at(sheet.page, (x, y)) {
        return Some(Viewer::picture(ctx, picture));
    }
    let layer = texts.get(id, sheet.page)?;
    let inside = |b: &&Bounds| (b.x0..=b.x1).contains(&x) && (b.y0..=b.y1).contains(&y);
    // The smallest is the figure, not the scan's page behind it.
    let area = |b: &&Bounds| (b.x1 - b.x0) * (b.y1 - b.y0);
    let at = *layer
        .figures
        .iter()
        .filter(inside)
        .min_by(|a, b| area(a).total_cmp(&area(b)))?;
    Some(Viewer::figure(ctx, Arc::clone(doc), sheet.page, at))
}

/// The open footnote's card; a press outside or Escape closes it.
pub(super) fn footnote(
    ctx: &egui::Context,
    view: &mut PdfView,
    place: &mut Place,
    room: Rect,
    count: usize,
    t: &Tokens,
) {
    use crate::footnote::{self, Press};
    let Some(note) = &view.footnote else {
        return;
    };
    // Its page scrolled away, the card has nothing to point at.
    let Some(at) = footnote::anchor(note, &view.sheets) else {
        view.footnote = None;
        return;
    };
    let id = egui::Id::new(("footnote", place.doc, note.page, note.text.as_str()));
    let (card, press) = footnote::show(ctx, id, (note, at, room), t);
    // The press that opened it lands on its mark, maybe this very frame.
    let outside = ctx.input(|i| {
        let origin = i.pointer.press_origin();
        let away = |p: Pos2| !card.contains(p) && !at.expand(4.0).contains(p);
        i.pointer.any_pressed() && origin.is_some_and(away)
    });
    let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
    match press {
        Some(Press::Go(page)) => {
            view.footnote = None;
            view.go_to(place, page, count);
        }
        Some(Press::Close) => view.footnote = None,
        None if outside || escape => view.footnote = None,
        None => {}
    }
}

/// The page and box of the shown link to `target` nearest `near`.
pub(super) fn mark_of(
    sheets: &[Sheet],
    texts: &TextLayers,
    doc: DocId,
    target: &LinkTarget,
    near: Option<Pos2>,
) -> Option<(usize, Bounds)> {
    let mut best: Option<(f32, usize, Bounds)> = None;
    for sheet in sheets {
        let Some(layer) = texts.get(doc, sheet.page) else {
            continue;
        };
        for link in layer.links.iter().filter(|l| l.target == *target) {
            let rect = sheet.to_screen(link.bounds);
            let d = near.map_or(0.0, |p| rect.distance_to_pos(p));
            if best.is_none_or(|(b, ..)| d < b) {
                best = Some((d, sheet.page, link.bounds));
            }
        }
    }
    best.map(|(_, page, at)| (page, at))
}

/// The AI's bubbles, over everything else on their pages.
pub(super) fn ai_bubbles(
    ui: &Ui,
    sheets: &[Sheet],
    ai: &page_ai::PaneAi,
    t: &Tokens,
    gloss: &mut Vec<Gloss>,
) {
    for o in ai.overlays.iter().filter(|o| !o.bubbles.is_empty()) {
        let Some(s) = sheets.iter().find(|s| s.page == o.page) else {
            continue;
        };
        let page = o.page;
        let at = (s.rect, s.shown, ai.top);
        match page_ai::paint::bubbles(ui, at, page, &o.bubbles, t) {
            Some(page_ai::paint::Press::Pick(index, at)) => {
                gloss.push(Gloss::Bubble { page, index, at });
            }
            Some(page_ai::paint::Press::Menu(index, at)) => {
                gloss.push(Gloss::BubbleMenu { page, index, at });
            }
            None => {}
        }
    }
}

/// A key at a focused stop does what a click there would.
pub(super) fn at_stop(
    press: Option<Press>,
    placed: &[study::marks::Placed],
    shaded: &[vocab::Shaded],
    picking: bool,
    gloss: &mut Vec<Gloss>,
) -> Option<LinkTarget> {
    let mark = |i: usize| placed.get(i);
    match press? {
        Press::Go(Stop::Link(target)) => return Some(target),
        Press::Go(Stop::Mark(i)) => {
            let m = mark(i)?;
            gloss.push(match picking {
                true => Gloss::Toggle(m.id),
                false => Gloss::Open {
                    mark: m.id,
                    at: m.bounds(),
                },
            });
        }
        Press::Go(Stop::Word(i)) => {
            let w = shaded.get(i).filter(|_| !picking)?;
            gloss.push(Gloss::Shaded {
                page: w.page,
                chars: w.chars.clone(),
                at: w.bounds(),
            });
        }
        Press::Menu(i) => {
            let m = mark(i).filter(|_| !picking)?;
            let at = m.bounds().left_bottom();
            let words = m.bounds();
            gloss.push(Gloss::Menu {
                mark: m.id,
                at,
                words,
            });
        }
    }
    None
}

/// ⌘C or Ctrl+C copies the selection, unless a text field has focus.
pub(super) fn copy(ctx: &egui::Context, place: &Place, doc: &Document, texts: &TextLayers) {
    let asked = ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Copy)));
    if !asked || ctx.egui_wants_keyboard_input() {
        return;
    }
    if let Some(text) = selected_text(place, doc, texts) {
        ctx.copy_text(text);
    }
}

/// The selection's text; none if nothing is selected.
pub(super) fn selected_text(place: &Place, doc: &Document, texts: &TextLayers) -> Option<String> {
    let sel = place.selection.filter(|s| !s.is_empty())?;
    let id = place.doc;
    // Pages scrolled out of the cache are read again, here.
    Some(sel.text(|page| {
        texts
            .get(id, page)
            .or_else(|| doc.text_layer(page).ok().map(Arc::new))
    }))
}

/// The bar under a finished selection, while any of it is on screen.
pub(super) fn selection_bar(
    ctx: &egui::Context,
    view: &PdfView,
    place: &Place,
    texts: &TextLayers,
    room: Rect,
    look: &HighlightSettings,
) -> Option<BarPress> {
    let sel = place.selection.filter(|s| !s.is_empty())?;
    let id = place.doc;
    let bar_id = egui::Id::new(("study-bar", id));
    // Hidden while a press elsewhere may be selecting; a press on it stays.
    let bar = crate::z_order::layer(crate::z_order::Tier::PageCard, bar_id);
    let (down, from) = ctx.input(|i| (i.pointer.any_down(), i.pointer.press_origin()));
    let on = from.and_then(|p| ctx.layer_id_at(p));
    // Its skills' list opens beside it: a press there keeps it too.
    if down && on != Some(bar) && !crate::side_list::owns(ctx, on) {
        return None;
    }
    let mut shown = Rect::NOTHING;
    for sheet in &view.sheets {
        let Some(layer) = texts.get(id, sheet.page) else {
            continue;
        };
        let Some(range) = sel.on_page(sheet.page, layer.text.len()) else {
            continue;
        };
        for b in layer.text.line_boxes(range) {
            let r = sheet.to_screen(b).intersect(sheet.shown).intersect(room);
            if r.is_positive() {
                shown = shown.union(r);
            }
        }
    }
    if !shown.is_positive() {
        return None;
    }
    study::show_bar(ctx, bar_id, shown, room, look)
}
