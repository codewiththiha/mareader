use super::*;

pub(super) fn run(cx: &mut ShowContext<'_>) {
    prepare(cx);
    refit(cx);
    rebuild(cx);
    reveal(cx);
    finish(cx);
}

fn prepare(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let doc = cx.doc;
    let view_cx = cx.view_cx;
    let ctx = &cx.ctx;
    let now = cx.now;
    let frame = cx.frame;
    let sizes = cx.sizes;
    let bands = cx.bands;
    let Some(ui) = cx.ui.as_deref_mut() else {
        return;
    };
    view.ease = motion::zoom(ctx);
    view.rtl = doc.right_to_left();
    view.lead = doc.lead();
    // A book's vertical strip is a stream: each page cut to its text.
    if ui.rect_contains_pointer(frame) {
        let (factor, pointer) = ctx.input(|i| (i.zoom_delta(), i.pointer.hover_pos()));
        if let (Some(p), true) = (pointer, factor != 1.0) {
            let at = p - frame.min - vec2(0.0, view_cx.top_inset);
            view.zoom.pinch(factor, at, now);
            // A zoom set by hand is measured from the page read.
            view.zoom.fit_page = view.current;
        }
    }
    if std::mem::take(&mut view.fresh) && view.zoom.fit.is_some() {
        view.zoom.fit = Some(view_cx.page.fit);
    }
    // A book's set zoom is its type size: the pane reflows it instead.
    let keep = view_cx.page.auto_resize && !doc.is_reflowable();
    // Remembered on the page it was set on, before a turn moves it.
    let set_on = layout::fit_box(view.mode, sizes, (view.zoom.fit_page, view.lead));
    view.zoom.hold(keep, set_on);
    if let Some(wait) = view.follow_size(sizes, keep, now) {
        ctx.request_repaint_after_secs(wait as f32);
    }
    let margin = if view.mode == ViewMode::Horizontal {
        MARGIN
    } else if bands.is_some() {
        // A stream's column never touches the pane's edge.
        view_cx.page.margin.max(MARGIN)
    } else {
        view_cx.page.margin
    };
    let mut room = frame.size() - vec2(2.0 * margin, 2.0 * MARGIN + view_cx.top_inset);
    // The strip fits a page's height alone: it scrolls sideways.
    if view.mode == ViewMode::Horizontal && view.zoom.measure() == Fit::Page {
        room.x = f32::INFINITY;
    }
    if let Some(wait) = view.settle_sheet(sheet_for(view, doc, room), now) {
        ctx.request_repaint_after_secs(wait as f32);
    }
    cx.margin = margin;
    cx.room = room;
}

fn refit(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let ctx = &cx.ctx;
    let now = cx.now;
    let sizes = cx.sizes;
    let room = cx.room;
    let fit_box = layout::fit_box(view.mode, sizes, (view.zoom.fit_page, view.lead));
    let follow = motion::follow(ctx);
    if let Some(wait) = view
        .zoom
        .refit(fit_box, room, view.layout.is_some(), follow, now)
    {
        ctx.request_repaint_after_secs(wait);
    }
    let (shown, gliding) = view.zoom.advance(now);
    if gliding {
        ctx.request_repaint();
    }
    cx.shown = Some(shown);
    cx.gliding = gliding;
}

fn rebuild(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let view_cx = cx.view_cx;
    let frame = cx.frame;
    let sizes = cx.sizes;
    let bands = cx.bands;
    let Some(shown) = cx.shown else {
        return;
    };
    let margin = cx.margin;
    let viewport = frame.size() - vec2(0.0, view_cx.top_inset);
    let inputs = Inputs {
        mode: view.mode,
        frame: frame.size(),
        inset: view_cx.top_inset,
        zoom: shown,
        focus: if view.mode.paged() { view.focus } else { 0 },
        gap: if bands.is_some() {
            0.0
        } else {
            view_cx.page.gap
        },
        margin,
        align: match bands {
            Some(_) => view_cx.page.column,
            None => egui::Align::Center,
        },
        rtl: view.rtl,
        lead: view.lead,
    };
    let rebuilt = view.layout.as_ref().is_none_or(|l| l.inputs != inputs);
    if rebuilt {
        let new = match bands {
            Some(bands) => {
                let cut = |b: &Band| PageSize {
                    width: b.width,
                    height: b.height,
                };
                Layout::new(&bands.iter().map(cut).collect::<Vec<_>>(), inputs)
            }
            None => Layout::new(sizes, inputs),
        };
        match view.layout.take() {
            // Zooms and resizes keep the reading spot in place.
            Some(old) if view.reveal.is_none() && old.inputs.mode == view.mode => {
                let pivot = view.zoom.pivot;
                let pivots = (
                    pivot.unwrap_or(view.viewport / 2.0),
                    pivot.unwrap_or(viewport / 2.0),
                );
                if let Some(to) = hold(&old, &new, view.offset, pivots) {
                    view.scroll_to = Some((to, 2));
                }
            }
            _ => {
                view.reveal.get_or_insert(view.current);
            }
        }
        view.layout = Some(new);
        // Results laid in or out: the strip stays on what was read.
        let keep = view.keep.take();
        let spot = keep.and_then(|(p, at)| view.layout.as_ref()?.place(p, at));
        if let Some(spot) = spot {
            let to = (spot - viewport / 2.0).to_vec2().max(Vec2::ZERO);
            view.reveal = None;
            view.scroll_to = Some((to, 2));
        }
    }
    cx.rebuilt = rebuilt;
    cx.viewport = viewport;
}

fn reveal(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let ctx = &cx.ctx;
    let now = cx.now;
    let rebuilt = cx.rebuilt;
    let viewport = cx.viewport;
    if let Some(page) = view.reveal.take()
        && let Some(to) = view.layout.as_ref().and_then(|l| l.reveal(page))
    {
        view.landed = (!view.mode.paged()).then_some((page, None));
        // A strip glides to the page; a new layout or a turn lands there.
        let glide = motion::jump(ctx);
        if !rebuilt && !view.mode.paged() && glide.duration.max > 0.0 {
            let until = now + f64::from(glide.duration.max) + 0.2;
            let end = view.layout.as_ref().map_or(Vec2::ZERO, |l| l.size) - viewport;
            let to = to.min(end.max(Vec2::ZERO));
            // Along the strip it glides with the keys' glide; across, it lands.
            let pick = |v: Vec2| {
                if view.mode == ViewMode::Horizontal {
                    v.x
                } else {
                    v.y
                }
            };
            view.aim = Some(Glide::new(pick(view.offset), pick(to), now, glide));
            view.jump = Some(Jump {
                to,
                started: false,
                until,
            });
        } else {
            view.scroll_to = Some((to, 2));
            view.jump = None;
        }
    }
}

fn finish(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let doc = cx.doc;
    let view_cx = cx.view_cx;
    let ctx = &cx.ctx;
    let now = cx.now;
    let frame = cx.frame;
    let Some(ui) = cx.ui.as_deref_mut() else {
        return;
    };
    let viewport = cx.viewport;
    land(view);
    if let Some(wait) = view.zoom.sharpen(now) {
        ctx.request_repaint_after_secs(wait);
    }
    // Keys move along the strip: sideways in the horizontal one.
    let sideways = view.mode == ViewMode::Horizontal;
    let len = if sideways { viewport.x } else { viewport.y };
    // Auto-scroll belongs to the focused pane, as in mareader.
    if !view_cx.active {
        view.drift = None;
    }
    let keyed = keyed_scroll(view, ctx, len, now);

    let Some(layout) = &view.layout else {
        return;
    };
    let ppp = ctx.pixels_per_point();
    let wanted = Scale::covering(view.zoom.sharp() * ppp);
    let max_side = ctx.input(|i| i.max_texture_side) as f32;
    // Past the GPU's and hayro's limits, a page is drawn in tiles.
    let resting = now - view.moved_at >= TILE_REST;
    let urgency = if view_cx.active {
        Urgency::ActivePane
    } else {
        Urgency::OtherPane
    };
    let mut t = view_cx.tokens;
    let reflow = doc.is_reflowable();
    // A book's pictures and emoji keep their colours unless themed.
    let (mut picture_tone, mut emoji_tone) = (PageTone::NONE, PageTone::NONE);
    if reflow {
        let mut text = t.book.inked(view_cx.page.ink);
        // Blend's paper, as the PDF beside it shows it.
        if let Some(raw) = view_cx.paper {
            text.paper = t.page.apply(raw.to_unit());
        }
        let themed = t.page.for_pictures(&text);
        let pick = |on: bool| if on { themed } else { PageTone::NONE };
        (picture_tone, emoji_tone) = (
            pick(view_cx.page.theme_pictures),
            pick(view_cx.page.theme_emojis),
        );
        t.page = text;
    }
    // egui turns a plain wheel sideways only if that's the sole direction.
    let down = view.mode != ViewMode::Horizontal || layout.size.y > frame.height() + 0.5;
    ui.style_mut().always_scroll_the_only_direction = true;
    let viewport = frame.size() - vec2(0.0, view_cx.top_inset);
    cx.input = Some(InputState {
        viewport,
        keyed,
        sideways,
        down,
        ppp,
        scale: wanted,
        max_side,
        resting,
        urgency,
        t,
        reflow,
        picture_tone,
        emoji_tone,
    });
}
