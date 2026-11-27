//! Exact word navigation in the reader.

use super::*;

#[test]
fn anchored_word_opens_its_exact_page_without_find_and_glows() -> Out {
    use mareader_engine::study::words::{read, span};
    use mareader_library::{MeaningBy, NewWord};
    use mareader_session::Selection;
    use std::sync::Arc;
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
    let (page, form, chars, spot) = (1..doc.page_count())
        .find_map(|page| {
            let layer = Arc::new(doc.text_layer(page).ok()?);
            let (text, indices) = read(&layer);
            let string: String = text.iter().collect();
            mareader_cefr::tokenize(&string)
                .into_iter()
                .find_map(|token| {
                    if token.len() < 6 {
                        return None;
                    }
                    let from = indices.get(token.start).copied().flatten()?;
                    let to = indices
                        .get(token.end.checked_sub(1)?)
                        .copied()
                        .flatten()?
                        .checked_add(1)?;
                    let chars = from..to;
                    let spot = span(Selection::within(page, chars.clone()), |_| {
                        Some(Arc::clone(&layer))
                    })?;
                    let form = layer.text.text(chars.clone());
                    Some((page, form, chars, spot))
                })
        })
        .ok_or("an anchored word")?;
    let lemma = form.to_lowercase();
    let id = h
        .state_mut()
        .seed_library_word(
            book,
            NewWord {
                lemma: lemma.clone(),
                form: form.clone(),
                context: format!("A {form} here."),
                spot: Some(spot),
                meaning: "a saved meaning".into(),
                meaning_by: Some(MeaningBy::Ai),
                ..NewWord::default()
            },
        )
        .ok_or("record word")?;
    assert_eq!(
        h.state().library().sightings_of(id)[0].spot.value,
        Some(spot)
    );
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Words)));
    h.run_steps(20);
    h.get_by_label(&format!("Details for {lemma}"))
        .click_secondary();
    h.run_steps(5);
    h.get_by_label("Go to word in book").click();
    h.run_steps(90);
    assert_eq!(h.state().route(), Route::Reader);
    assert_eq!(
        h.state().session().book_in(h.state().active_pane()),
        Some(book)
    );
    assert!(!h.state().find().is_open());
    let docs = h.state().documents();
    let sheet = docs
        .sheets(pane)
        .iter()
        .find(|s| s.page == page)
        .ok_or("destination page visible")?;
    let layer = doc.text_layer(page)?;
    let bounds = layer.text.line_boxes(chars).next().ok_or("word bounds")?;
    let rect = sheet.to_screen(bounds).expand(10.0);
    assert!(sheet.shown.intersects(rect), "word on screen");
    let lit = h.render()?;
    if let Ok(path) = std::env::var("MAREADER_VOCAB_SHOT") {
        lit.save(path)?;
    }
    h.run_steps(420);
    let faded = h.render()?;
    let min_x = rect.min.x.max(0.0) as u32;
    let min_y = rect.min.y.max(0.0) as u32;
    let max_x = (rect.max.x as u32).min(lit.width());
    let max_y = (rect.max.y as u32).min(lit.height());
    let changed = (min_y..max_y)
        .flat_map(|y| (min_x..max_x).map(move |x| (x, y)))
        .filter(|(x, y)| lit.get_pixel(*x, *y) != faded.get_pixel(*x, *y))
        .count();
    assert!(changed > 10, "word outline visibly fades: {changed} pixels");
    h.get_by_label("Go back to vocabs").click();
    h.run_steps(15);
    assert_eq!(h.state().session().room(), Room::Words);
    press(&mut h, &format!("Show where {lemma} was met"));
    h.run_steps(4);
    press(&mut h, &open_sentence(&lemma));
    h.run_steps(80);
    assert!(!h.state().find().is_open(), "sentence link does not search");
    assert!(
        h.state()
            .documents()
            .sheets(pane)
            .iter()
            .any(|s| s.page == page)
    );
    h.get_by_label("Go back to vocabs").click();
    h.run_steps(15);
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("other.pdf");
    std::fs::write(&path, mareader_pdf::sample::pdf(2, "Other"))?;
    let other = mareader_engine::documents::OpenRequest::path(path);
    h.state_mut().queue(Sent::Engine(Command::Open(other)));
    h.run_steps(40);
    let other_book = h.state().session().book_in(pane).ok_or("other book")?;
    assert_ne!(book, other_book);
    h.state_mut()
        .seed_library_word(
            other_book,
            NewWord {
                lemma: lemma.clone(),
                form: form.clone(),
                context: format!("Other {form} here."),
                ..NewWord::default()
            },
        )
        .ok_or("other sighting")?;
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Words)));
    h.run_steps(15);
    h.get_by_label("Merged").click();
    h.run_steps(10);
    let title = h
        .state()
        .library()
        .book(book)
        .ok_or("book title")?
        .title
        .value
        .clone();
    let book_label = mareader_ui_egui::space::open_book(&title);
    h.query_all_by_label(&book_label)
        .next()
        .ok_or("book link")?
        .click();
    h.run_steps(80);
    assert!(!h.state().find().is_open(), "book link does not search");
    assert!(
        h.state()
            .documents()
            .sheets(pane)
            .iter()
            .any(|s| s.page == page)
    );
    Ok(())
}
