//! Space, headless: the library bar's way in to the hub, the words page's
//! tabs and their counts, a word memorized or forgotten from its row, and
//! its sentence opening the book there, and a library card's counts.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::route::Route;
use mareader_engine::space::{Room, Tally, tally};
use mareader_library::WordStatus;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::space::open_sentence;
use mareader_ui_egui::ui_command::{Sent, UiCommand};

use crate::common::{act, press, reader};

type H = Harness<'static, MareaderApp>;
type Out = Result<(), Box<dyn std::error::Error>>;

/// Three books and the made-up words studied in them, on `page`.
fn shelf(page: &str) -> H {
    reader(&[("books", "3"), ("words", "demo"), ("route", page)])
}

/// The same books on Space's words page.
fn words() -> H {
    reader(&[("books", "3"), ("words", "demo"), ("space", "words")])
}

fn status(h: &H, lemma: &str) -> Option<WordStatus> {
    let lib = h.state().session().library_model();
    let found = lib.words().find(|w| w.lemma == lemma);
    found.map(|w| w.status.value)
}

#[test]
fn the_library_bar_opens_the_hub_and_its_card_every_sighting() {
    let mut h = shelf("library");
    assert_eq!(h.state().route(), Route::Library);
    h.get_by_label("Space").click();
    h.run();
    assert_eq!(h.state().route(), Route::Space);
    assert_eq!(h.state().session().room(), Room::Hub);
    h.get_by_label("Vocabs").click();
    h.run();
    assert_eq!(h.state().session().room(), Room::Words);
    // Two of the eight words were met in a second book.
    assert!(h.query_by_label("All 10").is_some());
    assert!(h.query_by_label("Memorized 4").is_some());
    assert!(h.query_by_label("Missed 2").is_some());
    h.get_by_label("Merged").click();
    h.run();
    assert!(h.query_by_label("All 8").is_some(), "a row per word");
}

#[test]
fn a_tab_keeps_only_its_status() {
    let mut h = words();
    h.get_by_label("Missed 2").click();
    h.run();
    assert!(h.query_by_label("Memorize ponder").is_some());
    assert!(h.query_by_label("Un-memorize arcane").is_none());
    assert!(
        h.query_by_label("All 10").is_some(),
        "counts ignore the tab"
    );
}

#[test]
fn a_row_memorizes_and_forgets_its_word() {
    let mut h = words();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Missed));
    press(&mut h, "Memorize ponder");
    h.run();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Memorized));
    assert!(h.query_by_label("Memorized 5").is_some());
    press(&mut h, "Un-memorize ponder");
    h.run();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Learning));
    press(&mut h, "Forget ponder");
    h.run();
    assert!(h.query_all_by_label("Also delete from AI dictionary").next().is_some());
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Learning));
    h.get_by_label("Delete").click();
    h.run();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Learning));
    h.get_by_label("Are you sure?").click();
    h.run();
    assert_eq!(status(&h, "ponder"), None);
    assert!(h.query_by_label("All 9").is_some());
}

#[test]
fn escape_goes_back_to_the_hub_then_the_library() {
    let mut h = words();
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Space);
    assert_eq!(h.state().session().room(), Room::Hub);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Library);
}

#[test]
fn a_rows_sentence_opens_its_book_and_finds_it_there() -> Out {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let raven = dir.join("../epub/tests/books/the-raven-gutenberg-17192.epub");
    let raven = raven.canonicalize()?;
    let mut h = reader(&[("epub", raven.to_str().ok_or("a path in UTF-8")?)]);
    h.run_steps(3);
    act(&mut h, Action::SeedWords);
    let words = Sent::Engine(Command::ShowSpace(Room::Words));
    h.state_mut().queue(words);
    h.run();
    press(&mut h, "Show where lintel was met");
    h.run();
    press(&mut h, &open_sentence("lintel"));
    for _ in 0..20 {
        h.run_steps(1);
    }
    assert_eq!(h.state().route(), Route::Reader, "back in the book");
    let find = h.state().find();
    assert!(find.is_open(), "the find bar looks for the sentence");
    assert_eq!(find.query(), "ducked beneath the lintel of the low");
    Ok(())
}

#[test]
fn a_library_cards_counts_open_space_on_that_book() -> Out {
    let mut h = shelf("library");
    act(&mut h, Action::SeedMarks);
    h.run();
    let counts = tally(h.state().session().library_model());
    // A book whose word count no other book shares, so its label is its own.
    let words: Vec<usize> = counts.values().map(|n| n.words).collect();
    let alone = |n: &&Tally| n.words > 0 && words.iter().filter(|w| **w == n.words).count() == 1;
    let n = counts.values().find(alone);
    let n = *n.ok_or("a book with its own word count")?;
    h.get_by_label(&format!("{} words", n.words)).click();
    h.run();
    assert_eq!(h.state().route(), Route::Space);
    assert_eq!(h.state().session().room(), Room::Words);
    let all = format!("All {}", n.words);
    assert!(h.query_by_label(&all).is_some(), "that book's alone");
    h.state_mut().queue(Sent::Ui(UiCommand::ShowLibrary));
    h.run();
    let noted = counts.values().find(|n| n.notes > 0);
    let notes = match noted.ok_or("a book with notes")?.notes {
        1 => "1 note".to_owned(),
        n => format!("{n} notes"),
    };
    let card = h.query_all_by_label(&notes).next();
    card.ok_or("its notes on the card")?.click();
    h.run();
    assert_eq!(h.state().session().room(), Room::Notes);
    Ok(())
}

#[test]
fn word_details_open_from_list_and_escape_returns_to_words() {
    let mut h = words();
    h.get_by_label("Details for arcane").click();
    h.run_steps(30);
    assert!(h.query_by_label("Close word details").is_some());
    assert!(h.query_by_label("Correct").is_some());
    assert!(h.query_by_label("known by very few").is_some());
    assert!(h.query_by_label("obscure").is_some());
    h.key_press(egui::Key::Escape);
    h.run_steps(30);
    assert!(h.query_by_label("Close word details").is_none());
    assert_eq!(h.state().session().room(), Room::Words);
    assert_eq!(h.state().route(), Route::Space);
}

#[test]
fn normal_word_detail_fits_without_outer_scroll() {
    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    h.get_by_label("Details for gaunt").click();
    h.run_steps(30);
    let before = h.get_by_label("haggard").rect();
    h.hover_at(h.get_by_label("Close word details").rect().center() + egui::vec2(-30.0, 45.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -180.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    assert_eq!(h.get_by_label("haggard").rect(), before);
}

#[test]
fn narrow_word_details_scroll_without_moving_the_header() {
    let mut h = words();
    h.get_by_label("Details for arcane").click();
    h.run_steps(30);
    h.set_size(egui::vec2(420.0, 500.0));
    h.run_steps(15);
    let before = h.get_by_label("Correct").rect().top();
    let close = h.get_by_label("Close word details").rect();
    h.hover_at(egui::pos2(40.0, 205.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -250.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    assert!(h.get_by_label("Correct").rect().top() < before - 20.0);
    assert_eq!(h.get_by_label("Close word details").rect(), close);
}

#[test]
fn recall_scrolls_independently_of_the_word_and_header() {
    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    h.get_by_label("Details for gaunt").click();
    h.run_steps(30);
    let first = h
        .query_all_by_label("Wrong")
        .next()
        .expect("first attempt")
        .rect();
    let close = h.get_by_label("Close word details").rect();
    let meaning = h.get_by_label("haggard").rect();
    h.hover_at(first.center());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -180.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    let moved = h
        .query_all_by_label("Wrong")
        .next()
        .expect("first attempt")
        .rect();
    assert!(
        moved.top() < first.top() - 20.0,
        "recall scrolls in its own area"
    );
    assert_eq!(h.get_by_label("Close word details").rect(), close);
    assert_eq!(h.get_by_label("haggard").rect(), meaning);
}

#[test]
fn narrow_recall_keeps_room_for_multiple_attempts_after_scrolling() {
    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    h.get_by_label("Details for gaunt").click();
    h.run_steps(30);
    h.set_size(egui::vec2(420.0, 500.0));
    h.run_steps(15);
    h.hover_at(egui::pos2(40.0, 205.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -350.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    let second = h
        .query_all_by_label("Close")
        .next()
        .expect("second verdict")
        .rect();
    let hint = h.get_by_label("Scroll to browse attempts").rect();
    assert!(
        second.bottom() < hint.top(),
        "the panel shows more than one attempt"
    );
    let close = h.get_by_label("Close word details").rect();
    h.hover_at(second.center());
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -160.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    let after = h
        .query_all_by_label("Close")
        .next()
        .expect("second verdict")
        .rect();
    assert!(after.top() < second.top() - 20.0, "the inner panel scrolls");
    assert_eq!(h.get_by_label("Scroll to browse attempts").rect(), hint);
    assert_eq!(h.get_by_label("Close word details").rect(), close);
}

#[test]
fn long_context_scrolls_without_moving_recall() {
    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    h.get_by_label("Details for gaunt").click();
    h.run_steps(30);
    let text = "The gaunt hound watched from the porch, its ears raised toward the distant road";
    let before = h.get_by_label_contains(text).rect();
    let recall = h
        .query_all_by_label("Wrong")
        .next()
        .expect("attempt")
        .rect();
    let hint = h.get_by_label("Scroll to read").rect();
    h.hover_at(hint.center() - egui::vec2(0.0, 60.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -180.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    assert!(h.get_by_label_contains(text).rect().top() < before.top() - 10.0);
    assert_eq!(
        h.query_all_by_label("Wrong")
            .next()
            .expect("attempt")
            .rect(),
        recall
    );
}

#[test]
fn long_book_title_line_scrolls_without_moving_close() {
    let mut h = words();
    h.get_by_label("Merged").click();
    h.run();
    h.get_by_label("Details for gaunt").click();
    h.run_steps(30);
    h.set_size(egui::vec2(260.0, 680.0));
    h.run_steps(15);
    let title = "Middlemarch, A Field Guide to Ferns";
    let before = h.get_by_label(title).rect();
    let close = h.get_by_label("Close word details").rect();
    h.hover_at(egui::pos2(100.0, close.center().y));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(-220.0, 0.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(30);
    assert!(h.get_by_label(title).rect().left() < before.left() - 10.0);
    assert_eq!(h.get_by_label("Close word details").rect(), close);
}

#[test]
fn word_details_open_from_cards_without_breaking_row_actions() {
    let mut h = reader(&[
        ("books", "3"),
        ("words", "demo"),
        ("space", "words"),
        ("space_view", "cards"),
    ]);
    h.get_by_label("Details for arcane").click();
    h.run_steps(30);
    assert!(h.query_by_label("Close word details").is_some());
    h.get_by_label("Close word details").click();
    h.run_steps(30);
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Missed));
    press(&mut h, "Memorize ponder");
    h.run();
    assert_eq!(status(&h, "ponder"), Some(WordStatus::Memorized));
}

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
    assert!(lib.words().any(|w| w.lemma == "reverie"), "offscreen item stays");
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
        let entry = Saved { word: "gaunt".into(), pos: "adjective".into(), lang: "en".into(), meaning: "thin".into(), ..Saved::default() };
        dict.ask(Query::Remember { saved: vec![entry], replace: false });
        assert!(dict.poll().1.iter().all(|r| r.failed.is_none()));
        let path = dir.path().to_str().unwrap();
        let mut h = reader(&[("books", "3"), ("words", "demo"), ("space", "words"), ("dictionaries", path)]);
        h.get_by_label("Merged").click();
        h.run();
        right_click(&mut h, "Details for gaunt");
        h.get_by_label("Delete…").click();
        h.run();
        if opt_in {
            h.query_all_by_label("Also delete from AI dictionary").last().unwrap().click();
            h.run();
        }
        confirm_delete(&mut h);
        h.run_steps(20);
        dict.ask(Query::Remembered { words: vec!["gaunt".into()], lang: "en".into() });
        let found = dict.poll().1.into_iter().next().unwrap();
        assert_eq!(!found.saved.is_empty(), !opt_in);
    }
}
