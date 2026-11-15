//! Selecting text and following links, headless, on the sample PDF.

use egui::{Event, Modifiers, OutputCommand, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};
use mareader_engine::command::Command;
use mareader_pdf::{LinkTarget, TextLayer, sample};
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::selection::Sheet;
use mareader_ui_egui::ui_command::Sent;
use std::sync::Arc;

use crate::common::find;

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

fn reader() -> H {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0").ok();
    app.set_option("pdf", "sample:40").ok();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

/// The first page's text and where that page sits on screen.
fn page_one(h: &H) -> Result<(Arc<TextLayer>, Sheet), String> {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let reading = docs.reading(pane).ok_or("no document")?;
    let layer = reading.document.text_layer(0).map_err(|e| e.to_string())?;
    let sheet = *docs.sheets(pane).first().ok_or("no page on screen")?;
    Ok((Arc::new(layer), sheet))
}

/// Character `i`'s left or right edge, half way up, on screen.
fn edge(layer: &TextLayer, sheet: Sheet, i: usize, right: bool) -> Result<Pos2, String> {
    let c = layer.text.chars().get(i).ok_or("no such character")?;
    let r = sheet.to_screen(c.bounds);
    let x = if right {
        r.right() - 1.0
    } else {
        r.left() + 1.0
    };
    Ok(pos2(x, r.center().y))
}

fn button(h: &mut H, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn drag(h: &mut H, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.step();
    button(h, from, true);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.step();
    }
    button(h, to, false);
    h.run();
}

fn clicks(h: &mut H, at: Pos2, n: usize) {
    h.hover_at(at);
    h.step();
    for _ in 0..n {
        button(h, at, true);
        button(h, at, false);
    }
    h.run();
}

/// Copies, returning what reached the clipboard this frame.
fn copy(h: &mut H) -> Option<String> {
    h.event(Event::Copy);
    h.step();
    h.output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        })
}

fn selected(h: &H) -> bool {
    let pane = h.state().active_pane();
    h.state().documents().selection(pane).is_some()
}

#[test]
fn dragging_over_text_selects_it_and_copy_takes_exactly_that() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    assert!(
        h.state().documents().text_layers() > 0,
        "text read for the view"
    );
    let quick = find(&layer, "The quick")?;
    let fox = find(&layer, "fox")? + 2;
    drag(
        &mut h,
        edge(&layer, sheet, quick, false)?,
        edge(&layer, sheet, fox, true)?,
    );
    assert!(selected(&h));
    assert_eq!(copy(&mut h).as_deref(), Some("The quick brown fox"));
    // Escape lets go first: the bar under the line would take the press.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!selected(&h));
    // Dragged backwards, across lines, it's the same text in order.
    let sphinx = find(&layer, "Sphinx")?;
    drag(
        &mut h,
        edge(&layer, sheet, sphinx + 5, true)?,
        edge(&layer, sheet, fox - 2, false)?,
    );
    let text = copy(&mut h).ok_or("nothing copied")?;
    assert!(text.starts_with("fox jumps"), "{text}");
    assert!(text.ends_with("\nSphinx"), "{text}");
    Ok(())
}

#[test]
fn double_and_triple_clicks_take_a_word_and_a_line() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let quartz = find(&layer, "quartz")? + 2;
    let at = edge(&layer, sheet, quartz, false)?;
    clicks(&mut h, at, 2);
    assert_eq!(copy(&mut h).as_deref(), Some("quartz"));
    h.run_steps(40);
    clicks(&mut h, at, 3);
    let line = copy(&mut h);
    assert_eq!(
        line.as_deref(),
        Some("Sphinx of black quartz, judge my vow.")
    );
    // Escape clears it; a plain click elsewhere would too.
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(!selected(&h));
    assert_eq!(copy(&mut h), None);
    Ok(())
}

#[test]
fn text_shows_a_text_cursor_and_links_a_hand() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    h.hover_at(edge(&layer, sheet, find(&layer, "Sphinx")?, false)?);
    h.run();
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::Text
    );
    let link = layer.links.first().ok_or("no link")?;
    h.hover_at(sheet.to_screen(link.bounds).center());
    h.run();
    let cursor = h.output().platform_output.cursor_icon;
    assert_eq!(cursor, egui::CursorIcon::PointingHand);
    Ok(())
}

#[test]
fn an_internal_link_turns_this_pane_and_a_web_link_opens_the_browser() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let [chapter, web] = layer.links.as_slice() else {
        return Err("the sample page has two links".into());
    };
    let pane = h.state().active_pane();
    // The browser is asked in the frame of the click itself.
    let at = sheet.to_screen(web.bounds).center();
    h.hover_at(at);
    h.step();
    button(&mut h, at, true);
    button(&mut h, at, false);
    let opened = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::OpenUrl(o) => Some(o.url.clone()),
            _ => None,
        });
    assert_eq!(opened.as_deref(), Some(sample::WEB_LINK));
    assert_eq!(h.state().documents().current(pane), Some(0));
    // "Next chapter, page 6".
    clicks(&mut h, sheet.to_screen(chapter.bounds).center(), 1);
    h.run_steps(10);
    assert_eq!(h.state().documents().current(pane), Some(5));
    Ok(())
}

#[test]
fn a_book_s_links_turn_its_pages_and_open_the_browser() -> Out {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("epub", "sample:2"),
        ("view", "single"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    let doc = h
        .state()
        .documents()
        .reading(pane)
        .ok_or("no book")?
        .document;
    let (page, layer) = (0..doc.page_count())
        .find_map(|p| Some((p, doc.text_layer(p).ok().filter(|l| l.links.len() > 1)?)))
        .ok_or("no page with links")?;
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run();
    let sheet = *h
        .state()
        .documents()
        .sheets(pane)
        .first()
        .ok_or("no sheet")?;
    assert_eq!(sheet.page, page);
    let web = layer
        .links
        .iter()
        .find(|l| matches!(l.target, LinkTarget::Web(_)));
    let web = sheet.to_screen(web.ok_or("no web link")?.bounds).center();
    h.hover_at(web);
    h.run();
    let cursor = h.output().platform_output.cursor_icon;
    assert_eq!(cursor, egui::CursorIcon::PointingHand);
    h.step();
    button(&mut h, web, true);
    button(&mut h, web, false);
    let opened = h
        .output()
        .platform_output
        .commands
        .iter()
        .find_map(|c| match c {
            OutputCommand::OpenUrl(o) => Some(o.url.clone()),
            _ => None,
        });
    assert_eq!(
        opened.as_deref(),
        Some("https://github.com/codewiththiha/mareader2")
    );
    let inside = layer.links.iter().find_map(|l| match l.target {
        LinkTarget::Page(to) if to != page => Some((l.bounds, to)),
        _ => None,
    });
    let (bounds, to) = inside.ok_or("no link to another page")?;
    clicks(&mut h, sheet.to_screen(bounds).center(), 1);
    h.run_steps(10);
    assert_eq!(h.state().documents().current(pane), Some(to));
    Ok(())
}

#[test]
fn dragging_past_the_bottom_scrolls_and_selects_into_the_next_page() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let from = edge(&layer, sheet, find(&layer, "Sphinx")?, false)?;
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    // Held below the window, the column scrolls on by itself.
    let below = pos2(from.x, 760.0);
    for _ in 0..90 {
        h.hover_at(below);
        h.step();
    }
    button(&mut h, below, false);
    h.run();
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let pages = docs.selection(pane).ok_or("nothing selected")?.pages();
    assert!(pages.start == 0 && pages.end >= 2, "{pages:?}");
    let text = copy(&mut h).ok_or("nothing copied")?;
    assert!(text.starts_with("Sphinx of black quartz"), "{text}");
    assert!(text.contains("\nPage 2 of 40\n"), "{text}");
    Ok(())
}

#[test]
fn the_wheel_scrolls_during_a_drag_and_the_selection_follows() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    let from = edge(&layer, sheet, find(&layer, "Sphinx")?, false)?;
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    // A small move starts the drag; the button stays down after.
    let held = from + egui::vec2(40.0, 0.0);
    for _ in 0..4 {
        h.hover_at(held);
        h.step();
    }
    let top = h
        .state()
        .documents()
        .sheets(h.state().active_pane())
        .first()
        .map(|s| s.rect.top());
    for _ in 0..2 {
        h.event(Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -600.0),
            phase: egui::TouchPhase::Move,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(30);
    }
    let pane = h.state().active_pane();
    let moved = h
        .state()
        .documents()
        .sheets(pane)
        .first()
        .map(|s| s.rect.top());
    assert!(
        moved < top,
        "the page moved up under the pointer: {top:?} -> {moved:?}"
    );
    button(&mut h, held, false);
    h.run();
    let pages = h
        .state()
        .documents()
        .selection(pane)
        .ok_or("nothing selected")?
        .pages();
    assert_eq!(
        pages,
        0..2,
        "the selection reaches the page now under the pointer"
    );
    let text = copy(&mut h).ok_or("nothing copied")?;
    assert!(text.starts_with("Sphinx of black quartz"), "{text}");
    assert!(text.contains("\nPage 2 of 40"), "{text}");
    Ok(())
}

#[test]
fn a_book_s_text_selects_and_copies_with_wrapped_lines_joined() -> Out {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("epub", "sample:2"),
        ("view", "single"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    // Page 0 is the cover; chapter 1 starts the next.
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page: 1 }));
    h.run();
    let docs = h.state().documents();
    let doc = docs.reading(pane).ok_or("no book")?.document;
    let layer = doc.text_layer(1)?;
    let sheet = *docs.sheets(pane).first().ok_or("no sheet")?;
    assert_eq!(sheet.page, 1);
    // From "The river" over the line's end to "unloaded", which wraps.
    let from = find(&layer, "The river")?;
    let to = find(&layer, "unloaded")? + "unloaded".len() - 1;
    let line = |i: usize| layer.text.chars().get(i).map(|c| c.line);
    assert!(line(from) < line(to), "the words cross a line end");
    drag(
        &mut h,
        edge(&layer, sheet, from, false)?,
        edge(&layer, sheet, to, true)?,
    );
    let text = copy(&mut h).ok_or("nothing copied")?;
    assert!(text.starts_with("The river ran slow"), "{text}");
    assert!(text.ends_with("the boats unloaded"), "{text}");
    assert!(!text.contains('\n'), "{text}");
    // A double click takes a word, a triple the paragraph.
    let customs = edge(&layer, sheet, find(&layer, "customs")? + 2, false)?;
    h.run_steps(40);
    clicks(&mut h, customs, 2);
    assert_eq!(copy(&mut h).as_deref(), Some("customs"));
    h.run_steps(40);
    clicks(&mut h, customs, 3);
    let paragraph = copy(&mut h).ok_or("nothing copied")?;
    assert!(paragraph.starts_with("1.0 The river"), "{paragraph}");
    assert!(paragraph.trim_end().ends_with("Bold end."), "{paragraph}");
    Ok(())
}

/// The first page's top on screen.
fn top(h: &H) -> Option<f32> {
    let pane = h.state().active_pane();
    let sheets = h.state().documents().sheets(pane);
    sheets.iter().find(|s| s.page == 0).map(|s| s.rect.top())
}

#[test]
fn dragging_empty_space_pans_and_a_quick_release_flings() -> Out {
    let mut h = reader();
    let (layer, sheet) = page_one(&h)?;
    // Text under the pointer first: the page's text layer loads.
    h.hover_at(edge(&layer, sheet, find(&layer, "Sphinx")?, false)?);
    h.run();
    // The page's left margin, clear of the title bar: no text, no link.
    let from = pos2(sheet.rect.left() + 6.0, sheet.rect.top() + 300.0);
    h.hover_at(from);
    h.run();
    let cursor = h.output().platform_output.cursor_icon;
    assert_eq!(cursor, egui::CursorIcon::Grab);
    let before = top(&h).ok_or("page one off screen")?;
    // A slow drag, resting before the release: the page follows exactly.
    let to = from - egui::vec2(0.0, 120.0);
    h.step();
    button(&mut h, from, true);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.step();
    }
    assert_eq!(
        h.output().platform_output.cursor_icon,
        egui::CursorIcon::Grabbing
    );
    h.run_steps(12);
    button(&mut h, to, false);
    h.run();
    let after = top(&h).ok_or("page one scrolled away")?;
    assert!((before - after - 120.0).abs() < 2.0, "{before} -> {after}");
    assert!(!selected(&h), "panning selects nothing");
    // A quick one glides on past the pointer.
    let from = to;
    let to = from - egui::vec2(0.0, 120.0);
    h.hover_at(from);
    h.step();
    button(&mut h, from, true);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.step();
    }
    button(&mut h, to, false);
    h.run();
    // Page one may glide off screen altogether.
    let flung = top(&h).unwrap_or(f32::NEG_INFINITY);
    assert!(after - flung > 160.0, "{after} -> {flung}");
    Ok(())
}

#[test]
fn a_book_s_footnote_opens_in_place_and_can_be_followed() -> Out {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("epub", "sample:2"),
        ("view", "single"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    let doc = h
        .state()
        .documents()
        .reading(pane)
        .ok_or("no book")?
        .document;
    let note = |l: &mareader_pdf::Link| match &l.target {
        LinkTarget::Note { page, text } => Some((l.bounds, *page, text.clone())),
        _ => None,
    };
    let (page, (bounds, to, text)) = (0..doc.page_count())
        .find_map(|p| Some((p, doc.text_layer(p).ok()?.links.iter().find_map(note)?)))
        .ok_or("no footnote")?;
    assert_eq!(text, "A note opens in a card on its mark.");
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run();
    let sheet = *h
        .state()
        .documents()
        .sheets(pane)
        .first()
        .ok_or("no sheet")?;
    let mark = sheet.to_screen(bounds).center();
    clicks(&mut h, mark, 1);
    h.run();
    // The note shows on its mark; the page stays.
    h.get_by_label(&text);
    assert_eq!(h.state().documents().current(pane), Some(page));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label(&text).is_none(), "Escape closes it");
    // Clicked again at once, the mark would count as a double click.
    h.run_steps(40);
    clicks(&mut h, mark, 1);
    h.run();
    h.get_by_label(&format!("Go to page {}", to + 1)).click();
    h.run_steps(10);
    h.run();
    assert!(h.query_by_label(&text).is_none());
    assert_eq!(h.state().documents().current(pane), Some(to));
    Ok(())
}

/// A point on `shown` away from the bars over the pane's top and foot.
fn clear_of_bars(h: &H, shown: egui::Rect) -> Result<Pos2, String> {
    let pane = h.state().pane_rects().first().map(|(_, r)| *r);
    let clear = pane.ok_or("no pane")?.shrink2(egui::vec2(0.0, 90.0));
    let both = shown.intersect(clear);
    match both.is_positive() {
        true => Ok(both.center()),
        false => Err(format!("{shown:?} is under the bars of {clear:?}")),
    }
}

#[test]
fn a_double_click_opens_a_book_s_picture_large() -> Out {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("epub", "sample:2"),
        ("view", "single"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 1400.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    let doc = h
        .state()
        .documents()
        .reading(pane)
        .ok_or("no book")?
        .document;
    let (page, picture) = (0..doc.page_count())
        .find_map(|p| Some((p, *doc.pictures(p).first()?)))
        .ok_or("no picture")?;
    h.state_mut()
        .queue(Sent::Engine(Command::GoTo { pane, page }));
    h.run();
    let sheet = *h
        .state()
        .documents()
        .sheets(pane)
        .iter()
        .find(|s| s.page == page)
        .ok_or("no sheet")?;
    let at = clear_of_bars(&h, sheet.to_screen(picture))?;
    clicks(&mut h, at, 2);
    // Decoded on its own thread, then shown.
    for _ in 0..200 {
        h.step();
        if h.query_by_label("Zoom in").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run();
    let shown = h.query_by_label_contains("Picture, ").is_some();
    assert!(shown, "the picture's viewer");
    let zoom = |h: &H| {
        h.query_by_label_contains("%")
            .and_then(|n| n.accesskit_node().label())
    };
    let before = zoom(&h);
    h.get_by_label("Zoom in").click();
    h.run();
    assert_ne!(zoom(&h), before, "zoomed");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.query_by_label("Zoom in").is_none(), "Escape closes it");
    assert_eq!(h.state().documents().current(pane), Some(page));
    Ok(())
}

#[test]
fn a_double_click_opens_a_pdf_s_figure_large() -> Out {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("pdf", "figures:3"),
        ("view", "single"),
        ("animations", "false"),
    ] {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 1400.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let pane = h.state().active_pane();
    let doc = h
        .state()
        .documents()
        .reading(pane)
        .ok_or("no pdf")?
        .document;
    let figure = *doc.text_layer(0)?.figures.first().ok_or("no figure")?;
    let sheet = *h
        .state()
        .documents()
        .sheets(pane)
        .first()
        .ok_or("no sheet")?;
    // Its text layer is read, so the figure is known.
    let at = clear_of_bars(&h, sheet.to_screen(figure))?;
    h.hover_at(at);
    h.run();
    // No `run` while the picture decodes: its spinner never settles.
    for _ in 0..2 {
        button(&mut h, at, true);
        button(&mut h, at, false);
    }
    for _ in 0..200 {
        h.step();
        if h.query_by_label("Zoom in").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    h.run_steps(5);
    assert!(
        h.query_by_label("Figure on page 1").is_some(),
        "the figure's viewer"
    );
    h.get_by_label("Close picture").click();
    h.run();
    assert!(h.query_by_label("Zoom in").is_none(), "Close closes it");
    Ok(())
}

#[test]
fn a_held_pane_lifts_and_moves_where_it_is_let_go() -> Out {
    let mut app = MareaderApp::default();
    for (k, v) in [
        ("workers", "0"),
        ("animations", "false"),
        ("panes", "2"),
        ("focus", "1"),
        ("pdf", "sample:5"),
        ("focus", "2"),
        ("pdf", "sample:5"),
    ] {
        app.set_option(k, v).ok();
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    let [first, second] = h.state().panes()[..] else {
        return Err("two panes".into());
    };
    let rect = |h: &H, pane| {
        let rects = h.state().pane_rects().to_vec();
        rects
            .into_iter()
            .find(|(id, _)| *id == pane)
            .map(|(_, r)| r)
    };
    let sheet = *h
        .state()
        .documents()
        .sheets(first)
        .first()
        .ok_or("no sheet")?;
    // The page's right margin, level with its body: no text there.
    let blank = mareader_pdf::Bounds {
        x0: 560.0,
        y0: 300.0,
        x1: 580.0,
        y1: 320.0,
    };
    let at = sheet.to_screen(blank).center();
    let before = rect(&h, first);
    h.hover_at(at);
    h.run();
    button(&mut h, at, true);
    // Held still past a second, the pane lifts.
    for _ in 0..70 {
        h.step();
    }
    assert!(h.query_by_label_contains("Moving ").is_some(), "lifted");
    // Its space closes up: the other pane fills it, a true preview.
    let home = before.ok_or("no first pane")?;
    let to = rect(&h, second).ok_or("no second pane")?;
    assert!(
        rect(&h, first).is_none(),
        "the lifted pane leaves the layout"
    );
    assert!(to.left() <= home.left() + 1.0, "the rest fills its space");
    let right = egui::pos2(to.right() - 20.0, to.center().y);
    h.hover_at(right);
    h.step();
    h.get_by_label("Release to move right of this pane");
    button(&mut h, right, false);
    h.run();
    assert_eq!(h.state().panes(), [second, first]);
    assert!(h.query_by_label_contains("Moving ").is_none());
    // Over the middle it would swap; Escape puts it back instead.
    let sheet = *h
        .state()
        .documents()
        .sheets(first)
        .first()
        .ok_or("no sheet")?;
    let at = sheet.to_screen(blank).center();
    h.hover_at(at);
    h.run();
    button(&mut h, at, true);
    for _ in 0..70 {
        h.step();
    }
    let middle = rect(&h, second).ok_or("no second pane")?.center();
    h.hover_at(middle);
    h.step();
    h.get_by_label("Release to swap panes");
    h.key_press(egui::Key::Escape);
    button(&mut h, middle, false);
    h.run();
    assert_eq!(h.state().panes(), [second, first], "Escape keeps them");
    assert!(
        rect(&h, first).is_some_and(|r| r.width() < 800.0),
        "split again"
    );
    Ok(())
}
