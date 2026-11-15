use super::*;

mod raster;

pub(super) fn run(cx: &mut ShowContext<'_>) {
    let Some(ui) = cx.ui.take() else {
        return;
    };
    let Some(area) = cx.scroll.as_mut().and_then(|s| s.area.take()) else {
        return;
    };
    let out = area
        .auto_shrink(false)
        // The view runs its own glides, so each delta lands this frame.
        .animated(false)
        .content_margin(0.0)
        // Dragging will select text, not scroll.
        .scroll_source(ScrollSource {
            drag: DragScroll::Never,
            ..Default::default()
        })
        .show_viewport(ui, |ui, viewport| {
            begin(cx, ui, viewport);
            raster::backdrop(cx, ui);
            raster::pages(cx, ui, viewport);
            prepare_ink(cx, ui);
            under(cx, ui);
            over(cx, ui);
            select(cx, ui);
            paint_ai(cx, ui);
            click(cx, ui);
            context_menu(cx);
            reach(cx, ui);
            if landing_demand(cx) {
                return;
            }
            prefetch(cx);
        });
    cx.paint.out = Some(out);
}

fn begin(cx: &mut ShowContext<'_>, ui: &mut Ui, viewport: Rect) {
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let view = &mut *cx.view;
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let keyed = input.keyed;
    let sideways = input.sideways;
    let jump = view.jump;
    let along = |d: f32| if sideways { vec2(d, 0.0) } else { vec2(0.0, d) };
    ui.set_min_size(layout.size);
    if let Some(points) = keyed {
        ui.scroll_with_delta_animation(-along(points), ScrollAnimation::none());
    }
    if let Some(j) = jump.filter(|j| !j.started) {
        let d = viewport.min.to_vec2() - j.to;
        let across = if sideways {
            vec2(0.0, d.y)
        } else {
            vec2(d.x, 0.0)
        };
        ui.scroll_with_delta_animation(across, ScrollAnimation::none());
        if let Some(scroll) = cx.scroll.as_mut() {
            scroll.jump_started = true;
        }
    }
}

fn prepare_ink(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let doc_id = cx.view.doc;
    let texts = &*cx.texts;
    let down = input.down;
    let vocab = &mut cx.vocab;
    // A result covers its page's text: nothing there selects or marks.
    let open: Vec<Sheet> = cx
        .paint
        .sheets
        .iter()
        .filter(|s| !cx.paint.covered.contains(&s.page))
        .copied()
        .collect();
    let text = Pages {
        doc: doc_id,
        sheets: &open,
        texts,
        sideways: !down,
        lift: view_cx.lift,
    };
    let placed = study::marks::place(&view_cx.marks, &text);
    // Shading sits under the reader's own marks.
    let shading = view_cx.shading.as_ref().zip(vocab.as_mut());
    let shaded = shading.map_or_else(vocab::Shading::default, |(look, v)| v.place(look, &text));
    let on_word = vocab::hovered(ui, &shaded).filter(|_| view_cx.picked.is_none());
    cx.paint.open = open;
    cx.paint.placed = placed;
    cx.paint.shaded = shaded;
    cx.paint.on_word = on_word;
}

fn under(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let t = input.t;
    let doc_id = cx.view.doc;
    let texts = &mut *cx.texts;
    let gloss = &mut *cx.gloss;
    let pass = cx.pass;
    let placed = &cx.paint.placed;
    let shaded = &cx.paint.shaded;
    let on_word = cx.paint.on_word;
    // Fills go under the page's ink, so marked text stays crisp.
    let mut under = study::marks::Under::new(t.page);
    if let Some(look) = &view_cx.shading {
        let under_mark = |i: usize| {
            let w = shaded.get(i);
            w.is_some_and(|w| placed.iter().any(|m| m.contains(w.bounds().center())))
        };
        let lit = on_word.filter(|i| !under_mark(*i));
        vocab::add(shaded, look, lit, &mut under);
        let id = ui.id().with(("shade-rest", doc_id));
        if let Some(i) = vocab::dwell(ui, id, shaded, lit, look)
            && let Some(w) = shaded.get(i)
            && let Some(layer) = texts.get(doc_id, w.page)
        {
            let word = study::words::text_of(&layer, w.chars.clone());
            let asked = study::words::asked_at(&layer, w.page, w.chars.clone());
            gloss.push(Gloss::Rest {
                context: asked.map_or_else(|| word.clone(), |a| a.context),
                word,
                at: w.bounds(),
            });
        }
    }
    study::marks::add(placed, view_cx.highlights.opacity, &mut under);
    under.show(ui, pass, ui.clip_rect(), &cx.paint.quads);
}

fn over(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let t = input.t;
    let down = input.down;
    let selection = &mut cx.view.selection;
    let placed = &cx.paint.placed;
    let sheets = &cx.paint.sheets;
    let text = Pages {
        doc: cx.view.doc,
        sheets: &cx.paint.open,
        texts: &*cx.texts,
        sideways: !down,
        lift: view_cx.lift,
    };
    study::marks::badges(ui, placed);
    for s in sheets
        .iter()
        .filter(|s| view_cx.bookmarks.contains(&s.page))
    {
        crate::bookmarks::ribbon(ui.painter(), s.rect, t.accent);
    }
    if let Some(picked) = &view_cx.picked {
        study::marks::ring(ui.painter(), placed, picked, t.accent);
    }
    if let Some(wash) = &view_cx.ai.wash {
        page_ai::paint::wash(ui, &text, wash, &t);
    }
    if let Some(lit) = &view_cx.aloud {
        crate::read_aloud::paint(ui, &text, lit, &t);
    }
    if let Some(lit) = &view_cx.found {
        crate::search::paint(ui, &text, lit, &t);
    }
    if let Some(sel) = selection.filter(|s| !s.is_empty()) {
        selection::paint(ui, &text, sel, t.accent.gamma_multiply(0.32));
    }
}

fn select(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let t = input.t;
    let down = input.down;
    let selection = &mut cx.view.selection;
    let marker = &mut cx.view.marker;
    let gloss = &mut *cx.gloss;
    let text = Pages {
        doc: cx.view.doc,
        sheets: &cx.paint.open,
        texts: &*cx.texts,
        sideways: !down,
        lift: view_cx.lift,
    };
    let did = selection::interact(ui, ui.clip_rect(), &text, selection);
    // The read-aloud pin goes over the text, so it takes the press.
    if let Some(m) = marker.as_mut() {
        let begun = selection.is_some_and(|s| !s.is_empty());
        if (did.click.is_some() || begun) && !m.held() {
            m.dismiss(ui.input(|i| i.time));
        }
        match marker::show(ui, ui.clip_rect(), &text, m, &t) {
            marker::Step::Stay => {}
            marker::Step::Read(from) => gloss.push(Gloss::ReadFrom(from)),
            marker::Step::Gone => *marker = None,
        }
    }
    cx.paint.did = did;
}

fn paint_ai(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let t = input.t;
    let view_cx = cx.view_cx;
    let frame = cx.frame;
    let gloss = &mut *cx.gloss;
    let sheets = &cx.paint.sheets;
    ai_bubbles(ui, sheets, &view_cx.ai.overlays, &t, gloss);
    if let Some(pill) = &view_cx.ai.pill
        && let Some(press) = page_ai::paint::pill(ui, frame, pill, &t)
    {
        gloss.push(Gloss::Pill(press));
    }
}

fn click(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let gloss = &mut *cx.gloss;
    let placed = &cx.paint.placed;
    let shaded = &cx.paint.shaded;
    cx.paint.follow = cx.paint.did.follow.clone();
    if let Some(at) = cx.paint.did.lift {
        gloss.push(Gloss::Lift(at));
    }
    let on_mark = |p: Option<Pos2>| p.and_then(|p| study::marks::hit(placed, p));
    // A long press picks; the click its release makes does nothing.
    if study::marks::swallowed(ui, cx.paint.did.click.is_some()) {
        cx.paint.follow = None;
    } else if let Some(m) = on_mark(cx.paint.did.click) {
        cx.paint.follow = None;
        gloss.push(match view_cx.picked {
            Some(_) => Gloss::Toggle(m.id),
            None => Gloss::Open {
                mark: m.id,
                at: m.bounds(),
            },
        });
    } else if let (Some(p), Some(look), None) =
        (cx.paint.did.click, &view_cx.shading, &view_cx.picked)
        && look.click
        && let Some(w) = vocab::hit(shaded, p).and_then(|i| shaded.get(i))
    {
        cx.paint.follow = None;
        gloss.push(Gloss::Shaded {
            page: w.page,
            chars: w.chars.clone(),
            at: w.bounds(),
        });
    }
    if let Some(id) = study::marks::long_press(ui, placed) {
        gloss.push(Gloss::Pick(id));
    }
}

fn context_menu(cx: &mut ShowContext<'_>) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let down = input.down;
    let gloss = &mut *cx.gloss;
    let selection = &mut cx.view.selection;
    let placed = &cx.paint.placed;
    let text = Pages {
        doc: cx.view.doc,
        sheets: &cx.paint.open,
        texts: &*cx.texts,
        sideways: !down,
        lift: view_cx.lift,
    };
    let sheets = &cx.paint.sheets;
    let on_mark = |p: Option<Pos2>| p.and_then(|p| study::marks::hit(placed, p));
    if let (Some(m), Some(at), None) = (
        on_mark(cx.paint.did.context),
        cx.paint.did.context,
        &view_cx.picked,
    ) {
        let words = m.bounds();
        gloss.push(Gloss::Menu {
            mark: m.id,
            at,
            words,
        });
    } else if let (Some(at), None) = (cx.paint.did.context, &view_cx.picked)
        && !on_selection(&text, *selection, at)
        && let Some(s) = sheets.iter().find(|s| s.shown.contains(at))
    {
        let layer = text.texts.get(text.doc, s.page);
        let (x, y) = s.to_page(at);
        let caret = (layer.as_ref()).filter(|l| l.text.on_line(x, y)).map(|l| {
            l.text
                .char_at(x, y)
                .unwrap_or_else(|| l.text.caret_at(x, y))
        });
        let place = layer.and_then(|layer| {
            let i = layer.text.caret_at(x, y);
            let before = i.checked_sub(1).and_then(|i| layer.place(i));
            layer.place(i).or(before)
        });
        let place = place.map(|(block, offset)| mareader_session::Anchor { block, offset });
        gloss.push(Gloss::PageMenu {
            page: s.page,
            at,
            place,
            caret,
        });
    } else if view_cx.menu_shot
        && let Some(m) = placed.last()
    {
        let at = m.bounds().center_bottom();
        let words = m.bounds();
        gloss.push(Gloss::Menu {
            mark: m.id,
            at,
            words,
        });
    }
}

fn reach(cx: &mut ShowContext<'_>, ui: &mut Ui) {
    let view_cx = cx.view_cx;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let t = input.t;
    let down = input.down;
    let gloss = &mut *cx.gloss;
    let texts = &*cx.texts;
    let doc_id = cx.view.doc;
    let placed = &cx.paint.placed;
    let shaded = &cx.paint.shaded;
    let text = Pages {
        doc: doc_id,
        sheets: &cx.paint.open,
        texts,
        sideways: !down,
        lift: view_cx.lift,
    };
    let sheets = &cx.paint.sheets;
    let on = reach::On {
        pages: &text,
        placed,
        shaded,
        words: view_cx.shading.as_ref().is_some_and(|l| l.click),
        accent: t.accent,
    };
    let press = reach::show(ui, &on);
    let picking = view_cx.picked.is_some();
    if let Some(target) = at_stop(press, placed, shaded, picking, gloss) {
        cx.paint.follow = Some(target);
    }
    if view_cx.picked.is_some() {
        // A mark across two pages is placed on each.
        let mut page: Vec<MarkId> = Vec::new();
        for m in placed {
            if !page.contains(&m.id) {
                page.push(m.id);
            }
        }
        let all = view_cx.marks.iter().map(|m| m.id).collect();
        gloss.push(Gloss::Seen { page, all });
    }
    if let Some(m) = placed.iter().find(|m| Some(m.id) == view_cx.card) {
        gloss.push(Gloss::Anchor(Some(m.bounds())));
    } else if view_cx.card.is_some() {
        // A page whose text hasn't come may hold the mark.
        let shown = sheets.iter().filter(|s| s.shown.is_positive());
        if shown
            .into_iter()
            .any(|s| texts.get(doc_id, s.page).is_none())
        {
            gloss.push(Gloss::Anchor(None));
        }
    }
}

fn prefetch(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let sizes = cx.sizes;
    let shown = cx.paint.shown_pages.clone();
    // The strips' neighbours, or the next and last turn's pages.
    let (near, count) = match view.mode {
        ViewMode::Single | ViewMode::Spread => (layout.pages(), sizes.len()),
        ViewMode::Vertical | ViewMode::Horizontal => (shown.clone(), sizes.len()),
    };
    let reach = PREFETCH * if view.mode == ViewMode::Spread { 2 } else { 1 };
    let before = near.start.saturating_sub(reach)..near.start;
    let after = near.end..(near.end + reach).min(count);
    for page in before.chain(after) {
        let Some(key) = raster::key(cx, page) else {
            continue;
        };
        if !cx.pages.touch(key) {
            cx.demand.push((Urgency::Prefetch, key.into()));
        }
    }
}

fn landing_demand(cx: &mut ShowContext<'_>) -> bool {
    let Some(scroll) = cx.scroll.as_ref() else {
        return false;
    };
    let Some(landing) = &scroll.landing else {
        return false;
    };
    let shown = &cx.paint.shown_pages;
    let Some(input) = cx.input.as_ref() else {
        return false;
    };
    let urgency = input.urgency;
    for page in landing.clone().filter(|p| !shown.contains(p)) {
        let Some(key) = raster::key(cx, page) else {
            continue;
        };
        if !matches!(cx.pages.lookup(key), Lookup::Sharp(_) | Lookup::Failed) {
            cx.demand.push((urgency, key.into()));
        }
    }
    true
}
