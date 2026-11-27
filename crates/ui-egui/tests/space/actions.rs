//! Context menus, deletion and returning to a vocabulary row.

use super::*;
use egui_kittest::kittest::Queryable as _;

fn right_click(h: &mut H, label: &str) {
    h.query_all_by_label(label)
        .next()
        .expect("row")
        .click_secondary();
    h.run_steps(4);
}

fn confirm_delete(h: &mut H) {
    h.get_by_label("Delete").click();
    h.run();
    h.get_by_label("Are you sure?").click();
    h.run_steps(3);
}

#[test]
fn context_delete_one_book_and_merged_delete_all_books() {
    let mut h = words();
    h.set_size(egui::vec2(1200.0, 1400.0));
    h.run_steps(5);
    let id = h
        .state()
        .session()
        .library_model()
        .words()
        .find(|w| w.lemma == "gaunt")
        .unwrap()
        .id;
    assert_eq!(
        h.state().session().library_model().sightings_of(id).len(),
        2
    );
    right_click(&mut h, "Details for gaunt");
    h.get_by_label("Delete…").click();
    h.run();
    confirm_delete(&mut h);
    assert_eq!(
        h.state().session().library_model().sightings_of(id).len(),
        1
    );
    assert!(h.state().session().library_model().word(id).is_some());

    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    let id = h
        .state()
        .session()
        .library_model()
        .words()
        .find(|w| w.lemma == "gaunt")
        .unwrap()
        .id;
    right_click(&mut h, "Details for gaunt");
    h.get_by_label("Delete…").click();
    h.run();
    confirm_delete(&mut h);
    assert!(
        h.state()
            .session()
            .library_model()
            .sightings_of(id)
            .is_empty()
    );
    assert!(h.state().session().library_model().word(id).is_none());
}

#[test]
fn cards_and_list_have_context_selection() {
    for cards in [false, true] {
        let mut h = reader(&[
            ("books", "3"),
            ("words", "demo"),
            ("space", "words"),
            ("space_view", if cards { "cards" } else { "list" }),
        ]);
        h.get_by_label("Merged").click();
        h.run();
        right_click(&mut h, "Details for gaunt");
        h.get_by_label("Select").click();
        h.run();
        assert!(h.query_by_label("1 selected").is_some());
        h.get_by_label("Clear selection").click();
        h.run();
        assert!(h.query_by_label("1 selected").is_none());
        right_click(&mut h, "Details for gaunt");
        h.get_by_label("Select all visible").click();
        h.run();
        assert!(h.query_all_by_label_contains("selected").next().is_some());
    }
}

#[test]
fn context_navigation_can_return_to_its_word() -> Out {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let raven = dir
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub")
        .canonicalize()?;
    let mut h = reader(&[("epub", raven.to_str().ok_or("UTF-8 path")?)]);
    h.run_steps(3);
    act(&mut h, Action::SeedWords);
    h.state_mut()
        .queue(Sent::Engine(Command::ShowSpace(Room::Words)));
    h.run_steps(3);
    right_click(&mut h, "Details for lintel");
    h.get_by_label("Go to word in book").click();
    h.run_steps(30);
    assert_eq!(h.state().route(), Route::Reader);
    assert_eq!(
        h.state().find().query(),
        "ducked beneath the lintel of the low"
    );
    let expanded = h.get_by_label("Go back to vocabs").rect();
    h.run_steps(340);
    let folded = h.get_by_label("Go back to vocabs").rect();
    assert!(
        folded.width() < expanded.width(),
        "return control folds after five seconds"
    );
    h.hover_at(folded.center());
    h.run_steps(10);
    let hovered = h.get_by_label("Go back to vocabs").rect();
    assert!(
        hovered.width() > folded.width(),
        "hover expands the control"
    );
    h.get_by_label("Go back to vocabs").click();
    h.run_steps(30);
    assert_eq!(h.state().route(), Route::Space);
    assert!(h.query_by_label("Details for lintel").is_some());
    Ok(())
}

#[test]
fn select_all_targets_only_visible_rows() {
    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    right_click(&mut h, "Details for gaunt");
    h.get_by_label("Select all visible").click();
    h.run();
    h.get_by_label("Delete selected…").click();
    h.run();
    confirm_delete(&mut h);
    let lib = h.state().session().library_model();
    assert!(lib.words().count() < 8);
    assert!(
        lib.words().any(|w| w.lemma == "reverie"),
        "offscreen item stays"
    );
}

#[test]
fn ai_dictionary_deletion_requires_opt_in() {
    use mareader_dict::{Dictionaries, Query, Saved};
    use std::sync::Arc;
    for opt_in in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut dict = Dictionaries::default();
        dict.attach(dir.path().to_owned(), Arc::new(|| {})).unwrap();
        dict.set_inline(true);
        let entry = Saved {
            word: "gaunt".into(),
            pos: "adjective".into(),
            lang: "en".into(),
            meaning: "thin".into(),
            ..Saved::default()
        };
        dict.ask(Query::Remember {
            saved: vec![entry],
            replace: false,
        });
        assert!(dict.poll().1.iter().all(|r| r.failed.is_none()));
        let path = dir.path().to_str().unwrap();
        let mut h = reader(&[
            ("books", "3"),
            ("words", "demo"),
            ("space", "words"),
            ("dictionaries", path),
        ]);
        h.get_by_label("Merged").click();
        h.run();
        right_click(&mut h, "Details for gaunt");
        h.get_by_label("Delete…").click();
        h.run();
        if opt_in {
            h.query_all_by_label("Also delete from AI dictionary")
                .last()
                .unwrap()
                .click();
            h.run();
        }
        confirm_delete(&mut h);
        h.run_steps(20);
        dict.ask(Query::Remembered {
            words: vec!["gaunt".into()],
            lang: "en".into(),
        });
        let found = dict.poll().1.into_iter().next().unwrap();
        assert_eq!(!found.saved.is_empty(), !opt_in);
    }
}

#[test]
fn return_rises_above_an_expanding_ai_stack() -> Out {
    use mareader_engine::page_ai::{Action as PageAi, How};
    use mareader_settings::PageTask;
    use mareader_ui_egui::activity::stack::STACK;

    let raven = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../epub/tests/books/the-raven-gutenberg-17192.epub")
        .canonicalize()?;
    let mut h = reader(&[
        ("epub", raven.to_str().ok_or("UTF-8 path")?),
        ("provider", "ollama"),
        ("page_ai_hold", "true"),
    ]);
    act(&mut h, Action::SeedWords);
    h.state_mut().queue(Sent::Engine(Command::ShowSpace(Room::Words)));
    h.run_steps(3);
    right_click(&mut h, "Details for lintel");
    h.get_by_label("Go to word in book").click();
    h.run_steps(30);
    let pane = h.state().active_pane();
    for page in [1, 2] {
        h.state_mut().queue(Sent::Engine(Command::PageAi(PageAi::Run {
            pane,
            page,
            task: PageTask::Simplify,
            how: How::default(),
        })));
        h.run_steps(20);
    }
    let stack = h.get_by_label(STACK).rect();
    let back = h.get_by_label("Go back to vocabs").rect();
    assert!(back.bottom() <= stack.top(), "folded: {back:?}, {stack:?}");
    h.hover_at(stack.center());
    for _ in 0..25 {
        h.run_steps(1);
        let stack = h.get_by_label(STACK).rect();
        let back = h.get_by_label("Go back to vocabs").rect();
        assert!(back.bottom() <= stack.top(), "expanding: {back:?}, {stack:?}");
    }
    Ok(())
}
