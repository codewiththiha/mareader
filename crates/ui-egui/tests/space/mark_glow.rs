//! Returning to a real highlight from Space's Notes.

use super::*;
use mareader_library::{MarkKind, Pen};

#[test]
fn notes_highlight_gets_a_traveling_accent_glow() -> Out {
    let raven = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub")
        .canonicalize()?;
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    app.set_option("epub", raven.to_str().ok_or("UTF-8 path")?)?;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(30);
    let pane = h.state().active_pane();
    let book = h.state().session().book_in(pane).ok_or("book")?;
    let doc = h
        .state()
        .session()
        .documents()
        .document_of(pane)
        .ok_or("document")?;
    let (page, text) = (0..doc.page_count())
        .find_map(|page| {
            let layer = doc.text_layer(page).ok()?;
            let start = (0..layer.text.len().saturating_sub(24))
                .find(|&i| (i..i + 24).all(|n| layer.place(n).is_some()))?;
            let text = layer.text.text(start..start + 24);
            (!text.trim().is_empty()).then_some((page, text))
        })
        .ok_or("text")?;
    h.state_mut()
        .queue(Sent::Engine(Command::SelectText { pane, page, text }));
    h.run_steps(8);
    h.state_mut().queue(Sent::Engine(Command::Highlight {
        pane,
        pen: Pen::Full,
        color: None,
    }));
    h.run_steps(8);
    let mark = h
        .state()
        .library()
        .marks(book)
        .into_iter()
        .find(|m| m.kind == MarkKind::Highlight)
        .cloned()
        .ok_or("saved highlight")?;
    let spans =
        mareader_engine::skills::resolve(&doc, mark.spot, &mark.word).ok_or("resolved mark")?;
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Notes)));
    h.run_steps(20);
    h.query_all_by_label_contains("Open ")
        .next()
        .ok_or("highlight row")?
        .click();
    h.run_steps(90);
    assert_eq!(h.state().route(), Route::Reader);
    assert!(!h.state().find().is_open());
    let sheet = *h
        .state()
        .documents()
        .sheets(pane)
        .iter()
        .find(|s| s.page == page)
        .ok_or("highlight page")?;
    let layer = doc.text_layer(page)?;
    let range = spans.first().ok_or("highlight span")?.1.clone();
    let bounds = layer
        .text
        .line_boxes(range)
        .next()
        .ok_or("highlight bounds")?;
    let rect = sheet.to_screen(bounds).expand(20.0);
    assert!(sheet.shown.intersects(rect));
    let first = h.render()?;
    if let Ok(path) = std::env::var("MAREADER_NOTE_SHOT") {
        first.save(path)?;
    }
    h.run_steps(25);
    let second = h.render()?;
    if let Ok(path) = std::env::var("MAREADER_NOTE_SHOT2") {
        second.save(path)?;
    }
    let (x0, y0) = (rect.min.x.max(0.0) as u32, rect.min.y.max(0.0) as u32);
    let (x1, y1) = (
        (rect.max.x as u32).min(first.width()),
        (rect.max.y as u32).min(first.height()),
    );
    let moved = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| first.get_pixel(*x, *y) != second.get_pixel(*x, *y))
        .count();
    assert!(moved > 10, "the highlight's accent arc moves: {moved}");
    let mut settings = h.state().settings().clone();
    settings.motion.enabled = false;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(settings))));
    h.run_steps(5);
    let still = h.render()?;
    h.run_steps(25);
    let held = h.render()?;
    let changes = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| still.get_pixel(*x, *y) != held.get_pixel(*x, *y))
        .count();
    assert_eq!(changes, 0, "motion-off destination holds still");
    h.run_steps(380);
    let gone = h.render()?;
    let changes = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| held.get_pixel(*x, *y) != gone.get_pixel(*x, *y))
        .count();
    assert!(changes > 10, "the temporary destination glow ends");
    h.get_by_label("Go back to notes").click();
    h.run_steps(15);
    assert_eq!(h.state().session().room(), Room::Notes);
    Ok(())
}

#[test]
fn pdf_highlight_from_notes_glows_without_search() -> Out {
    let mut app = MareaderApp::default();
    app.set_option("workers", "0")?;
    let path = std::env::temp_dir().join(format!("mareader-glow-{}.pdf", std::process::id()));
    std::fs::write(&path, mareader_pdf::sample::pdf(5, "Sample"))?;
    app.set_option("pdf", path.to_str().ok_or("PDF path")?)?;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(400)
        .wgpu()
        .build_eframe(|_| app);
    h.run_steps(30);
    let pane = h.state().active_pane();
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: "quick".into(),
    }));
    h.run_steps(5);
    h.state_mut().queue(Sent::Engine(Command::Highlight {
        pane,
        pen: Pen::Full,
        color: None,
    }));
    h.run_steps(8);
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Notes)));
    h.run_steps(20);
    h.get_by_label("Open quick").click();
    h.run_steps(90);
    assert_eq!(h.state().route(), Route::Reader);
    assert!(!h.state().find().is_open());
    let first = h.render()?;
    h.run_steps(25);
    let second = h.render()?;
    let (layer, sheet) = crate::common::page(&h, 0)?;
    let from = crate::common::find(&layer, "quick")?;
    let bounds = layer
        .text
        .line_boxes(from..from + 5)
        .next()
        .ok_or("bounds")?;
    let rect = sheet.to_screen(bounds).expand(20.0);
    let (x0, y0) = (rect.min.x.max(0.0) as u32, rect.min.y.max(0.0) as u32);
    let (x1, y1) = (
        (rect.max.x as u32).min(first.width()),
        (rect.max.y as u32).min(first.height()),
    );
    let moved = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .filter(|(x, y)| first.get_pixel(*x, *y) != second.get_pixel(*x, *y))
        .count();
    assert!(moved > 10, "PDF highlight glow moves: {moved}");
    Ok(())
}

#[test]
fn motion_off_snaps_the_return() -> Out {
    let raven = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub")
        .canonicalize()?;
    let mut h = reader(&[("epub", raven.to_str().ok_or("UTF-8 path")?)]);
    act(&mut h, Action::SeedWords);
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Words)));
    h.run_steps(3);
    let mut settings = h.state().settings().clone();
    settings.motion.enabled = false;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(settings))));
    h.run_steps(2);
    h.query_all_by_label("Details for lintel")
        .next()
        .ok_or("word row")?
        .click_secondary();
    h.run_steps(5);
    h.get_by_label("Go to word in book").click();
    h.run_steps(30);
    let full = h.get_by_label("Go back to vocabs").rect();
    h.run_steps(340);
    let folded = h.get_by_label("Go back to vocabs").rect();
    h.hover_at(folded.center());
    h.run_steps(3);
    let hovered = h.get_by_label("Go back to vocabs").rect();
    assert!(
        (hovered.width() - full.width()).abs() < 1.0,
        "motion disabled snaps open"
    );
    Ok(())
}
