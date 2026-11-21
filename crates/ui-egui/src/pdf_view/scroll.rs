use egui::{Pos2, Rect};

use super::*;

pub(super) fn run(cx: &mut ShowContext<'_>) {
    // Read before the view is borrowed: both are fields of `cx`.
    let mode = cx.place.mode;
    let doc = cx.place.doc;
    let view = &mut *cx.view;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let viewport = input.viewport;
    let down = input.down;
    let scroll_doc = *view.scroll_id.get_or_insert(doc);
    let mut area = ScrollArea::new([true, down]).id_salt(("pdf", scroll_doc, mode));
    if let Some((to, _)) = view.scroll_to {
        area = area.scroll_offset(to);
    }
    let jump = view.jump;
    // While a jump glides, only the pages it lands on are worth drawing.
    let landing = jump.map(|j| layout.visible(Rect::from_min_size(Pos2::ZERO + j.to, viewport)));
    cx.scroll = Some(ScrollState {
        area: Some(area),
        landing,
        jump_started: false,
    });
}

pub(super) fn finish(cx: &mut ShowContext<'_>) {
    finish_jump(cx);
    hold_at_gate(cx);
    finish_scroll(cx);
}

/// A strip scrolls no further than the end of study mode's gate page, nor,
/// with the lock on, above the top of its start page.
///
/// The frame that overshoots shows only veiled pages; the next one is held.
fn hold_at_gate(cx: &mut ShowContext<'_>) {
    let hold = cx.view.study_span;
    if cx.place.mode.paged() || hold == StudySpan::default() {
        return;
    }
    cx.place.current = hold.clamp(cx.place.current);
    let (Some(out), Some(input)) = (cx.paint.out.as_ref(), cx.input.as_ref()) else {
        return;
    };
    let Some(layout) = cx.view.layout.as_ref() else {
        return;
    };
    let top = |r: Rect| (r.min.to_vec2() - Vec2::splat(GAP)).max(Vec2::ZERO);
    let least = layout.rect(hold.floor).map_or(Vec2::ZERO, top);
    let end = hold.gate.and_then(|gate| layout.rect(gate));
    let bottom = |r: Rect| (r.max.to_vec2() - input.viewport).max(least);
    let most = end.map_or(Vec2::INFINITY, bottom);
    let mode = cx.place.mode;
    let fit = |v: Vec2| match mode {
        ViewMode::Horizontal => vec2(v.x.clamp(least.x, most.x), v.y),
        _ => vec2(v.x, v.y.clamp(least.y, most.y)),
    };
    let offset = out.state.offset;
    let held = fit(offset);
    if held != offset {
        let to = cx.view.scroll_to.map_or(held, |(to, _)| fit(to));
        cx.view.scroll_to = Some((to, 2));
        cx.ctx.request_repaint();
    }
}

fn finish_jump(cx: &mut ShowContext<'_>) {
    let Some(out) = cx.paint.out.as_ref() else {
        return;
    };
    let view = &mut *cx.view;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let ctx = &cx.ctx;
    let now = cx.now;
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let viewport = input.viewport;
    let Some(scroll) = cx.scroll.as_ref() else {
        return;
    };
    let jump_started = scroll.jump_started;
    if let Some(j) = &mut view.jump {
        j.started |= jump_started;
        // Its glide ended, or a wheel or a key took over.
        let arrived = (out.state.offset - j.to).length() < 1.0;
        if arrived || view.aim.is_none() || now > j.until {
            view.jump = None;
            // Taken over midway: the page under the centre is read.
            if !arrived {
                cx.place.landed = None;
                view.landed_at = None;
            }
        } else {
            ctx.request_repaint();
        }
    }
    // A jump keeps its page as the one read until it lands.
    if !cx.place.mode.paged() && view.scroll_to.is_none() && view.jump.is_none() {
        let centre = Pos2::ZERO + out.state.offset + viewport / 2.0;
        cx.place.current = layout.locate(centre).map_or(0, |(page, _)| page);
        // A short page jumped to stays the one read until the strip moves.
        let offset = out.state.offset;
        let still = match view.landed_at {
            None => {
                view.landed_at = Some(offset);
                true
            }
            Some(at) => {
                let still = (offset - at).length() < 1.0;
                if still {
                    view.landed_at = Some(at);
                }
                still
            }
        };
        if !still {
            cx.place.landed = None;
        }
        if let Some(page) = cx.place.landed {
            cx.place.current = page;
        }
    }
}

fn finish_scroll(cx: &mut ShowContext<'_>) {
    let Some(out) = cx.paint.out.as_ref() else {
        return;
    };
    let view = &mut *cx.view;
    let Some(input) = cx.input.as_ref() else {
        return;
    };
    let ctx = &cx.ctx;
    let now = cx.now;
    let Some(layout) = view.layout.as_ref() else {
        return;
    };
    let viewport = input.viewport;
    let sizes = cx.sizes;
    cx.place.progress = progress(cx.place, layout, out, sizes.len());
    cx.place.visible = cx.paint.shown_pages.clone();
    view.sheets = std::mem::take(&mut cx.paint.sheets);
    // Nothing shown, as mid-jump: the backdrop holds its colour.
    view.paper_at = cx.paint.paper_at.or(view.paper_at);
    if view.offset != out.state.offset || view.viewport != viewport {
        view.moved_at = now;
    }
    if now - view.moved_at < TILE_REST {
        ctx.request_repaint_after_secs((TILE_REST - (now - view.moved_at)) as f32 + 0.01);
    }
    view.offset = out.state.offset;
    view.viewport = viewport;
    view.zoom.end_frame(cx.gliding, now);
    // A new area clamps offsets to last frame's size, so retry once.
    if let Some((to, tries)) = view.scroll_to {
        let reached = (out.state.offset - to).length() < 0.5;
        view.scroll_to = (!reached && tries > 1).then_some((to, tries - 1));
        if view.scroll_to.is_some() {
            ctx.request_repaint();
        }
    }
}
