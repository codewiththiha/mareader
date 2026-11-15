use super::*;

pub(super) fn run(cx: &mut ShowContext<'_>) {
    scroll::finish(cx);
    selection_actions(cx);
    links(cx);
    windows(cx);
}

fn selection_actions(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let doc = cx.doc;
    let view_cx = cx.view_cx;
    let ctx = &cx.ctx;
    let frame = cx.frame;
    let texts = &mut *cx.texts;
    let gloss = &mut *cx.gloss;
    if view_cx.active {
        copy(ctx, view, doc, texts);
        let room = Rect::from_min_max(frame.min + vec2(0.0, view_cx.top_inset), frame.max);
        let pressed = selection_bar(ctx, view, texts, room, &view_cx.highlights);
        match pressed {
            Some(BarPress::Copy) => {
                if let Some(text) = selected_text(view, doc, texts) {
                    ctx.copy_text(text);
                    gloss.push(Gloss::Copied);
                }
            }
            Some(BarPress::Explain) => gloss.push(Gloss::Explain),
            Some(BarPress::LookUp) => gloss.push(Gloss::LookUp),
            Some(BarPress::Note) => gloss.push(Gloss::Note),
            Some(BarPress::ReadAloud) => gloss.push(Gloss::ReadAloud),
            Some(BarPress::Highlight(pen, color)) => gloss.push(Gloss::Highlight(pen, color)),
            None => {}
        }
    }
    if !view_cx.active {
        view.footnote = None;
    }
}

fn links(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let doc = cx.doc;
    let ctx = &cx.ctx;
    let sizes = cx.sizes;
    let texts = &mut *cx.texts;
    match cx.paint.follow.take() {
        Some(LinkTarget::Page(page)) => view.go_to(page, sizes.len()),
        Some(LinkTarget::Note { page, text }) => {
            let target = LinkTarget::Note {
                page,
                text: text.clone(),
            };
            match mark_of(&view.sheets, texts, view.doc, &target, cx.paint.did.click) {
                Some((on, at)) => {
                    view.footnote = Some(crate::footnote::Footnote {
                        page: on,
                        at,
                        text,
                        target: page,
                    });
                }
                None => view.go_to(page, sizes.len()),
            }
        }
        // Checked when read; checked again before leaving the app.
        Some(LinkTarget::Web(url)) if web_address(url.as_bytes()).is_some() => {
            ctx.open_url(egui::OpenUrl::new_tab(url));
        }
        Some(LinkTarget::Media { path, .. }) => {
            crate::play::play(Arc::clone(doc), path);
        }
        Some(LinkTarget::Transcript { card, track }) => view.transcripts.click(card, track),
        Some(LinkTarget::Trigger { link, .. }) => {
            view.triggers.click(doc.trigger(link));
            ctx.request_repaint();
        }
        Some(LinkTarget::Script { chapter, node }) => {
            let target = LinkTarget::Script { chapter, node };
            let mark = mark_of(&view.sheets, texts, view.doc, &target, cx.paint.did.click);
            view.triggers.script.click(chapter, node, mark);
            ctx.request_repaint();
        }
        _ => {}
    }
}

fn windows(cx: &mut ShowContext<'_>) {
    let view = &mut *cx.view;
    let doc = cx.doc;
    let view_cx = cx.view_cx;
    let ctx = &cx.ctx;
    let frame = cx.frame;
    let sizes = cx.sizes;
    let texts = &mut *cx.texts;
    if view_cx.active {
        let room = Rect::from_min_max(frame.min + vec2(0.0, view_cx.top_inset), frame.max);
        let paged = &mut view.triggers.script;
        crate::scripts::edit(ctx, paged, &view.sheets, room, &view_cx.tokens);
    } else {
        view.triggers.script.editing = None;
    }
    if view_cx.active {
        let room = Rect::from_min_max(frame.min + vec2(0.0, view_cx.top_inset), frame.max);
        footnote(ctx, view, room, sizes.len(), &view_cx.tokens);
    }
    if let Some(p) = cx.paint.did.double {
        view.viewer =
            open_picture(ctx, (&view.sheets, texts, view.doc), doc, p).or(view.viewer.take());
    }
    if let Some(viewer) = &mut view.viewer
        && !crate::image_viewer::show(ctx, viewer, &view_cx.tokens)
    {
        view.viewer = None;
    }
}
