use egui::{Rect, Ui, Vec2};

use super::*;

pub(super) fn backdrop(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let Some(pass) = cx.pass else {
        return;
    };
    let view = &mut *cx.view;
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let frame = cx.frame;
    let has_bands = cx.bands.is_some();
    let reflow = input.reflow;
    let drawn = &mut *cx.drawn;
    // One sheet under every page, so patterns run across seams.
    let anchor = Anchor {
        origin: ui.max_rect().min,
        zoom: layout.zoom(),
    };
    let rect = (frame, view_cx.corner);
    let make =
        |paper: Option<Rgb>, t: &Tokens| backdrop_quad(rect, anchor, (reflow, has_bands), paper, t);
    let to = make(view_cx.paper, &input.t);
    let quads = match input.fade {
        None => to.into_iter().collect(),
        Some(fade) => blend_backdrop(make(fade.paper, &fade.t), to, fade.progress),
    };
    if !quads.is_empty() {
        let shape = PagesCallback::shape(pass.clone(), frame, quads.clone());
        ui.painter().add(shape);
        drawn.extend(quads);
    }
}

fn backdrop_quad(
    (frame, corner): (Rect, f32),
    anchor: Anchor,
    (reflow, has_bands): (bool, bool),
    blend_paper: Option<Rgb>,
    t: &Tokens,
) -> Option<Quad> {
    // Blend's paper under the shadows; a stream always has its own.
    let white = Rgb(255, 255, 255);
    let paper = match reflow {
        // A book's tone holds its paper already.
        true => (blend_paper.is_some() || has_bands).then_some(white),
        false => blend_paper.or(has_bands.then_some(white)),
    }?;
    let tone = t.page.backdrop(paper.to_unit());
    Some(Quad::backdrop(frame, corner, tone, anchor))
}

fn blend_backdrop(from: Option<Quad>, to: Option<Quad>, progress: f32) -> Vec<Quad> {
    match (from, to) {
        (Some(old), Some(mut new)) if old.tone != new.tone => {
            new.tint = fade_tint(progress);
            vec![old, new]
        }
        (Some(old), Some(_)) => vec![old],
        (Some(mut old), None) => {
            old.tint = fade_tint(1.0 - progress);
            vec![old]
        }
        (None, Some(mut new)) => {
            new.tint = fade_tint(progress);
            vec![new]
        }
        (None, None) => Vec::new(),
    }
}

pub(super) fn pages(cx: &mut ShowContext<'_>, ui: &mut Ui, viewport: Rect) {
    let origin = ui.max_rect().min.to_vec2();
    let view = &mut *cx.view;
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let shown = layout.visible(viewport);
    cx.paint.shown_pages = shown.clone();
    for page in shown.clone() {
        paint_one(cx, ui, origin, page);
    }
    let frame = cx.frame;
    let view_cx = cx.view_cx;
    let pass = cx.pass;
    let drawn = &mut *cx.drawn;
    cx.paint.paper_at = mareader_pdf::position(cx.paint.sheets.iter().map(|s| {
        let seen = s.shown;
        (s.page, if seen.is_positive() { seen.area() } else { 0.0 })
    }));
    // One callback draws every GPU page, over the shadows.
    for quad in &mut cx.paint.quads {
        quad.round_to(frame, view_cx.corner);
    }
    if let (Some(pass), false) = (pass, cx.paint.quads.is_empty()) {
        let shape = PagesCallback::shape(pass.clone(), ui.clip_rect(), cx.paint.quads.clone());
        ui.painter().add(shape);
        drawn.extend(cx.paint.quads.iter().cloned());
    }
}

fn paint_one(cx: &mut ShowContext<'_>, ui: &mut Ui, origin: Vec2, page: usize) {
    let Some(plan) = plan(cx, page) else {
        return;
    };
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let (ppp, reflow, t) = (input.ppp, input.reflow, input.t);
    let (urgency, picture_tone, emoji_tone) = (input.urgency, input.picture_tone, input.emoji_tone);
    let fade = input.fade;
    let (view_cx, doc, sizes, bands, frame) = (cx.view_cx, cx.doc, cx.sizes, cx.bands, cx.frame);
    let doc_id = cx.place.doc;
    let view = &mut *cx.view;
    let (pages, demand, texts) = (&mut *cx.pages, &mut *cx.demand, &mut *cx.texts);
    let Some(scroll) = cx.scroll.as_ref() else {
        return;
    };
    let wanted = |page: usize| scroll.landing.as_ref().is_none_or(|r| r.contains(&page));
    let Some(rect) = view.layout.as_ref().and_then(|layout| layout.rect(page)) else {
        return;
    };
    let anchor = Anchor {
        origin: ui.max_rect().min,
        zoom: view.layout.as_ref().map_or(1.0, |layout| layout.zoom()),
    };
    let rect = rect.translate(origin).round_to_pixels(ppp);
    let (rect, clip) = match bands.and_then(|b| b.get(page)) {
        Some(band) => streamed(rect, band, sizes.get(page), anchor.zoom, ppp),
        None => (rect, ui.clip_rect()),
    };
    let clip = clip.intersect(ui.clip_rect());

    let look = pages.lookup(plan.base());
    if wanted(page) && !matches!(look, Lookup::Sharp(_) | Lookup::Failed) {
        demand.push((urgency, plan.base().into()));
    }
    if wanted(page) && !texts.touch(doc_id, page) {
        demand.push((Urgency::Text, Want::Text { doc: doc_id, page }));
    }
    let fixed_page = doc.is_fixed_page(page);
    let paged = page_tokens(t, input.fixed_tone, fixed_page);
    let round = (frame, view_cx.corner);
    let style = (&paged, view_cx.page.shadow && bands.is_none());
    let first = cx.paint.quads.len();
    paint_page(
        ui,
        ((rect, clip), anchor, round),
        look,
        style,
        &mut cx.paint.quads,
    );
    paint_tiles(cx, ui, page, plan, (rect, clip), anchor, paged.page);
    let width = sizes.get(page).map_or(1.0, |s| s.width);
    if reflow && !fixed_page {
        let dest = (&mut cx.paint.quads, first);
        let tones = (picture_tone, emoji_tone);
        paint_media(doc, page, (rect, width), dest, tones);
    }
    let overlay = view_cx.ai.overlays.iter().find(|o| o.page == page);
    if let Some(shown) = overlay.and_then(|o| o.card.as_ref()) {
        let tones = (&t, picture_tone, emoji_tone);
        let place = ((rect, clip), anchor);
        page_ai::paint::card(ui, place, shown, tones, &mut cx.paint.quads);
        cx.paint.covered.push(page);
    }
    if let Some(fade) = fade {
        let from_page = page_tokens(fade.t, fade.fixed_tone, fixed_page).page;
        let to = (paged.page, picture_tone, emoji_tone);
        let from = (from_page, fade.picture_tone, fade.emoji_tone);
        blend_quads(&mut cx.paint.quads, first, to, from, fade.progress);
    }
    cx.paint.sheets.push(Sheet {
        page,
        rect,
        shown: rect.intersect(clip),
        scale: rect.width() / width,
    });
}

fn plan(cx: &ShowContext<'_>, page: usize) -> Option<Plan> {
    let input = cx.input.as_ref()?;
    let wanted = input.scale;
    let max_side = input.max_side;
    let doc_id = cx.place.doc;
    let doc = cx.doc;
    let sizes = cx.sizes;
    let size = sizes.get(page).copied().unwrap_or(PageSize {
        width: 1.0,
        height: 1.0,
    });
    let long = size.width.max(size.height);
    let cap = Scale::within(doc.fitted_scale(page, max_side / long).unwrap_or(1.0));
    let whole = |scale| PageKey {
        doc: doc_id,
        page,
        scale,
        part: Part::Whole,
    };
    if wanted <= cap {
        return Some(Plan::Whole(whole(wanted)));
    }
    let backdrop = Scale::within((BACKDROP_SIDE / long).min(cap.value()));
    let tiling = Tiling::new(size, wanted.value());
    Some(Plan::Tiled {
        backdrop: whole(backdrop),
        tiling,
        scale: wanted,
    })
}

pub(super) fn key(cx: &ShowContext<'_>, page: usize) -> Option<PageKey> {
    Some(plan(cx, page)?.base())
}

fn paint_tiles(
    cx: &mut ShowContext<'_>,
    ui: &mut Ui,
    page: usize,
    plan: Plan,
    (rect, clip): (Rect, Rect),
    anchor: Anchor,
    tone: PageTone,
) {
    let Plan::Tiled {
        backdrop,
        tiling,
        scale,
    } = plan
    else {
        return;
    };
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let Some(scroll) = cx.scroll.as_ref() else {
        return;
    };
    let landing = &scroll.landing;
    let wanted = landing.as_ref().is_none_or(|r| r.contains(&page));
    let key = PageKey { scale, ..backdrop };
    let tiles = Tiles {
        page: rect,
        clip,
        anchor,
        tiling,
        key,
    };
    let ask = input.resting && wanted;
    tiles.paint(
        ui,
        &mut *cx.pages,
        ask.then_some((&mut *cx.demand, input.urgency)),
        (&mut cx.paint.quads, tone),
    );
}

fn page_tokens(mut t: Tokens, fixed_tone: PageTone, fixed_page: bool) -> Tokens {
    if fixed_page {
        t.page = fixed_tone;
    }
    t
}

fn paint_media(
    doc: &Document,
    page: usize,
    (rect, width): (Rect, f32),
    (quads, first): (&mut Vec<Quad>, usize),
    (picture_tone, emoji_tone): (PageTone, PageTone),
) {
    let to_screen = |b: Bounds| {
        let k = rect.width() / width;
        Rect::from_min_max(
            rect.min + vec2(b.x0, b.y0) * k,
            rect.min + vec2(b.x1, b.y1) * k,
        )
    };
    let shown = doc.pictures(page).into_iter().map(to_screen);
    keep_pictures(quads, first, shown, picture_tone);
    let emojis = doc.emojis(page).into_iter().map(to_screen);
    keep_pictures(quads, first, emojis, emoji_tone);
}

fn fade_tint(progress: f32) -> egui::Color32 {
    let a = (progress.clamp(0.0, 1.0) * 255.0).round() as u8;
    egui::Color32::from_rgba_premultiplied(a, a, a, a)
}

fn blend_quads(
    quads: &mut Vec<Quad>,
    first: usize,
    to: (PageTone, PageTone, PageTone),
    from: (PageTone, PageTone, PageTone),
    progress: f32,
) {
    let map = |tone: PageTone| {
        if tone == to.0 {
            from.0
        } else if tone == to.1 {
            from.1
        } else if tone == to.2 {
            from.2
        } else {
            tone
        }
    };
    if quads
        .get(first..)
        .is_none_or(|tail| tail.iter().all(|q| map(q.tone) == q.tone))
    {
        return;
    }
    let tint = fade_tint(progress);
    let tail = quads.split_off(first);
    for mut q in tail {
        let old_tone = map(q.tone);
        if old_tone != q.tone {
            let mut under = q.clone();
            under.tone = old_tone;
            quads.push(under);
            q.tint = tint;
        }
        quads.push(q);
    }
}
