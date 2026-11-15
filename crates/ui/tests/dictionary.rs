//! Look up, the Dictionary page and Settings over fixture packs.

use std::path::Path;

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_dict::{PACKS, State, build, db_file};
use mareader_settings::{Bar, DictionaryOpens};
use mareader_ui::MareaderApp;
use mareader_ui::command::Command;
use mareader_ui::dict::card::PURSUE;
use mareader_ui::dict::page::LABEL;
use mareader_ui::dict::window::{CLOSE, EXPAND, MINIMIZE, RESTORE};
use mareader_ui::park::PEEK;
use mareader_ui::route::Route;
use mareader_ui::settings_window::Tab;
use mareader_ui::settings_window::{ALL_LANGUAGES, MORE_LANGUAGES};

use crate::common::click_at;

type H = Harness<'static, MareaderApp>;

/// A folder holding the Burmese and French packs, converted.
fn shelf() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary folder");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    for (file, id) in [
        ("en-my.parquet", "mcfnlp-en-my"),
        ("en-fr.parquet", "wiktionary-en-fr"),
    ] {
        let pack = PACKS.iter().find(|p| p.id == id).expect("a pack on offer");
        build(&fixtures.join(file), &db_file(dir.path(), pack)).expect("a fixture converts");
    }
    dir
}

fn reader(dir: &Path) -> H {
    let mut app = MareaderApp::default();
    let path = dir.to_string_lossy().into_owned();
    for (k, v) in [
        ("workers", "0"),
        ("animations", "false"),
        ("dictionaries", path.as_str()),
        ("pdf", "sample:5"),
    ] {
        app.set_option(k, v).expect("a known option");
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1000.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    h
}

fn select(h: &mut H, text: &str) {
    let pane = h.state().active_pane();
    let text = text.to_owned();
    h.state_mut().queue(Command::SelectText {
        pane,
        page: 0,
        text,
    });
    h.run();
}

#[test]
fn look_up_answers_from_the_packs_in_the_default_language() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_none());
    let dict = h.state().dict();
    assert_eq!(dict.card_word(), Some("quick"));
    // No default yet: the first language other than English.
    assert_eq!(dict.card_answers(), Some(vec!["မြန်သော".to_owned()]));
    h.get_by_label("English → Burmese");
    // The card points at its words from beside them, never over them.
    let (card, at) = h.state().dict().card_place().expect("a drawn card");
    assert!(!card.intersects(at), "{card:?} covers {at:?}");
    assert!(card.top() >= at.bottom() || card.bottom() <= at.top());
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().dict().card_word(), None);

    let mut s = h.state().settings().clone();
    s.dictionary.default_lang = Some("fr".into());
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    select(&mut h, "quick brown");
    h.get_by_label("Look up").click();
    h.run();
    let dict = h.state().dict();
    assert_eq!(dict.card_word(), Some("quick brown"));
    assert_eq!(dict.card_answers(), Some(Vec::new()));
    h.get_by_label("No entry for “quick brown”.");
}

#[test]
fn without_packs_the_card_leads_to_settings() {
    let empty = tempfile::tempdir().expect("a temporary folder");
    let mut h = reader(empty.path());
    select(&mut h, "fox");
    h.get_by_label("Look up").click();
    h.run();
    h.get_by_label("No dictionary is downloaded.");
    h.get_by_label("Get dictionaries…").click();
    h.run();
    h.get_by_label("Language packs");
    h.get_by_label(ALL_LANGUAGES);
    h.get_by_label("Download English → Burmese");
}

#[test]
fn the_tab_lists_packs_and_removes_one() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().queue(Command::OpenSettings(Tab::Dictionary));
    h.run();
    // Packs on this device show; the rest wait in a fold.
    assert!(h.query_by_label("Download English → Japanese").is_none());
    h.get_by_label(MORE_LANGUAGES).click();
    h.run();
    h.get_by_label("Download English → Japanese");
    h.get_by_label("Download English → Korean");
    h.get_by_label("Remove English → French").click();
    h.run();
    let fr = PACKS
        .iter()
        .find(|p| p.id == "wiktionary-en-fr")
        .expect("French");
    assert_eq!(*h.state().dict().state(fr), State::Absent);
    h.get_by_label("Download English → French");
    h.get_by_label("Default language");
}

#[test]
fn the_panel_searches_every_pack_as_typed() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_page", "show")
        .expect("a known option");
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    // Focused first, as a click does, then typed into.
    h.get_by_label(LABEL).click();
    h.run();
    h.get_by_label(LABEL).type_text("ight");
    // The debounce passes, then the reply lands.
    h.run_steps(30);
    // Each answer shows in its row, and the chosen one in full.
    assert!(h.query_all_by_label_contains("lumière").count() >= 1);
    assert!(h.query_all_by_label_contains("အလင်း").count() >= 1);
}

/// Settings › Dictionary › Full page: the reader opens the page.
fn full_page(h: &mut H) {
    let mut s = h.state().settings().clone();
    s.dictionary.opens = DictionaryOpens::Page;
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.run();
}

#[test]
fn the_shortcut_opens_the_page_and_escape_steps_back() {
    let dir = shelf();
    let mut h = reader(dir.path());
    full_page(&mut h);
    let keys = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    h.key_press_modifiers(keys, egui::Key::D);
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    // Cmd+D's bookmark never rode along with the Shift.
    h.get_by_label("Back to reading");
    h.get_by_label(LABEL).type_text("quick");
    h.run_steps(30);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().dict().page_text(), "", "Escape clears first");
    assert_eq!(h.state().route(), Route::Dictionary);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().route(), Route::Reader);
}

#[test]
fn the_page_bar_opens_its_settings_and_keeps_a_pin() {
    let dir = shelf();
    let mut h = reader(dir.path());
    // The reader's button opens the floating window; this is the page.
    h.state_mut()
        .set_option("dict_page", "show")
        .expect("a known option");
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    h.get_by_label("Settings").click();
    h.run();
    let window = h.state().settings_window();
    assert!(window.is_open());
    assert_eq!(window.tab(), Tab::Dictionary);
    assert_eq!(h.query_all_by_label("Layout").count(), 0, "a reading tab");
    h.state_mut().queue(Command::CloseSettings);
    h.run();
    assert!(h.state().titlebar().pinned, "the page starts pinned");
    let field_top = |h: &H| h.state().dict().page_field().map(|r| r.top());
    let below_bar = field_top(&h).expect("the field shows");
    assert!(below_bar > 48.0, "pinned, the page starts under the bar");
    h.get_by_label("Dictionary options").click();
    h.run();
    h.get_by_label("Keep title bar shown").click();
    h.run();
    assert!(!h.state().titlebar().pinned);
    let pins = &h.state().settings().chrome;
    assert!(!pins.pinned(Bar::Dictionary) && pins.pinned(Bar::Library));
    // A press on the page shuts the menu; the pointer gone, the bar hides.
    let away = egui::pos2(500.0, 600.0);
    h.hover_at(away);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: away,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    h.run_steps(60);
    assert!(!h.state().titlebar().visible(), "unpinned, it hides");
    let risen = field_top(&h).expect("the field shows");
    assert!(risen < below_bar - 40.0, "the page takes the bar's room");
    // Over the field, the bar stays away; above it, the bar comes.
    h.hover_at(egui::pos2(500.0, risen + 10.0));
    h.run_steps(8);
    assert!(!h.state().titlebar().visible(), "the field keeps it away");
    h.hover_at(egui::pos2(500.0, 5.0));
    h.run_steps(8);
    assert!(
        h.state().titlebar().visible(),
        "the top edge brings it back"
    );
}

#[test]
fn from_the_library_the_page_returns_to_the_library() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    h.get_by_label("Dictionary").click();
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    h.get_by_label("Back to the library").click();
    h.run();
    assert_eq!(h.state().route(), Route::Library);
    // The page's Library button goes there from the reader's side too.
    h.state_mut().queue(Command::ShowLibrary);
    h.run();
    full_page(&mut h);
    h.state_mut().queue(Command::ShowDictionary);
    h.run();
    h.get_by_label("Library").click();
    h.run();
    assert_eq!(h.state().route(), Route::Library);
}

#[test]
fn arrows_pick_a_result_and_enter_keeps_the_search() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_page", "ight")
        .expect("a known option");
    h.run_steps(30);
    h.key_press(egui::Key::ArrowDown);
    h.key_press(egui::Key::Enter);
    h.run();
    let recent = &h.state().settings().dictionary.recent;
    assert_eq!(recent, &["ight".to_owned()]);
    // Cleared, the page offers it again; a click searches it.
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_label("Recent");
    h.get_by_label("ight").click();
    h.run();
    assert_eq!(h.state().dict().page_text(), "ight");
}

#[test]
fn an_answer_can_be_looked_up_in_turn() {
    let dir = shelf();
    let mut h = reader(dir.path());
    let mut s = h.state().settings().clone();
    s.dictionary.langs = vec!["wiktionary-en-fr".into()];
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.state_mut()
        .set_option("dict_page", "light")
        .expect("a known option");
    h.run_steps(30);
    h.get_by_label_contains("Look up “").click();
    h.run();
    let text = h.state().dict().page_text().to_owned();
    assert!(!text.is_empty() && text != "light", "{text}");
}

fn window_open(h: &H) -> (bool, bool) {
    let w = &h.state().settings().dictionary.window;
    (w.open, w.minimized)
}

#[test]
fn in_the_reader_the_button_floats_a_window_that_folds_and_closes() {
    let dir = shelf();
    let mut h = reader(dir.path());
    // As the bar's Dictionary button sends.
    h.state_mut().queue(Command::ShowDictionary);
    h.run();
    assert_eq!(h.state().route(), Route::Reader, "the book stays");
    assert_eq!(window_open(&h), (true, false));
    h.get_by_label(LABEL).type_text("ight");
    h.run_steps(30);
    assert!(h.query_all_by_label_contains("lumière").count() >= 1);
    h.get_by_label(MINIMIZE).click();
    h.run();
    assert_eq!(window_open(&h), (true, true));
    assert!(h.query_by_label(LABEL).is_none(), "folded to a pill");
    // The pill names the word last searched.
    h.get_by_label("ight");
    h.get_by_label(RESTORE).click();
    h.run();
    assert_eq!(window_open(&h), (true, false));
    h.get_by_label(CLOSE).click();
    h.run();
    assert_eq!(window_open(&h), (false, false));
    // The shortcut brings it back, its field ready for typing.
    let keys = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    h.key_press_modifiers(keys, egui::Key::D);
    h.run();
    assert_eq!(window_open(&h), (true, false));
    assert!(h.state().dict().typing(), "the field has the keys");
}

/// Presses at `from`, moves by `by` in steps, and lets go.
fn drag(h: &mut H, from: egui::Pos2, by: egui::Vec2) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.hover_at(from);
    h.run_steps(2);
    h.event(button(from, true));
    h.run_steps(2);
    for i in 1..=6 {
        h.hover_at(from + by * (i as f32 / 6.0));
        h.run_steps(1);
    }
    h.event(button(from + by, false));
    h.run_steps(3);
}

#[test]
fn the_window_moves_and_resizes_and_remembers_where() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("a known option");
    h.run();
    let at = h.state().dict().window_rect().expect("a drawn window");
    drag(
        &mut h,
        at.left_top() + egui::vec2(60.0, 18.0),
        egui::vec2(-200.0, 40.0),
    );
    let moved = h.state().dict().window_rect().expect("still drawn");
    assert!(
        (moved.left() - (at.left() - 200.0)).abs() < 2.0,
        "{moved:?}"
    );
    assert!((moved.top() - (at.top() + 40.0)).abs() < 2.0, "{moved:?}");
    let saved = h.state().settings().dictionary.window.rect;
    assert_eq!(saved.map(|r| r[0].round()), Some(moved.left().round()));
    drag(
        &mut h,
        moved.right_bottom() - egui::vec2(3.0, 3.0),
        egui::vec2(60.0, -100.0),
    );
    let sized = h.state().dict().window_rect().expect("still drawn");
    assert!(
        (sized.width() - (moved.width() + 60.0)).abs() < 2.0,
        "{sized:?}"
    );
    assert!(
        (sized.height() - (moved.height() - 100.0)).abs() < 2.0,
        "{sized:?}"
    );
    assert_eq!(sized.left_top(), moved.left_top(), "the far corner held");
}

#[test]
fn dragging_past_a_side_peeks_the_dictionary_itself() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("a known option");
    h.run_steps(30);
    let at = h.state().dict().window_rect().expect("a drawn window");
    let size = at.size();
    let by = egui::vec2(-at.left() - at.width() * 0.5 - 8.0, 0.0);
    drag(&mut h, at.left_top() + egui::vec2(60.0, 18.0), by);
    h.run_steps(30);

    let tucked = h.state().dict().window_rect().expect("still drawn");
    let exposed = h.state().dict().window_cover().expect("visible edge");
    assert_eq!(tucked.size(), size, "the original shape stays whole");
    assert!(
        tucked.left() < 0.0,
        "it rests beyond the left edge: {tucked:?}"
    );
    assert!((exposed.width() - PEEK).abs() < 1.0, "{exposed:?}");
    assert!(h.query_by_label("Show the dictionary").is_some());
    assert!(h.query_by_label(CLOSE).is_none(), "no controls at the edge");

    h.hover_at(exposed.center());
    h.run_steps(20);
    let lifted = h.state().dict().window_cover().expect("the lifted edge");
    assert!(lifted.width() > exposed.width() + 5.0, "{lifted:?}");
    h.hover_at(egui::pos2(450.0, 50.0));
    h.run_steps(20);
    let settled = h.state().dict().window_cover().expect("the settled edge");
    assert!((settled.width() - PEEK).abs() < 1.0, "{settled:?}");

    click_at(&mut h, settled.center());
    h.run_steps(30);
    let restored = h.state().dict().window_rect().expect("restored window");
    assert_eq!(restored.size(), size);
    assert!(restored.left() >= 0.0, "{restored:?}");
    assert!(h.query_by_label(MINIMIZE).is_some(), "controls return");
}

#[test]
fn a_dictionary_edge_tab_names_the_word_and_restores_the_window() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("set", "workspace.edge_parking:\"tab\"")
        .expect("tab parking setting");
    h.state_mut()
        .set_option("dict_window", "light")
        .expect("dictionary window");
    h.run_steps(30);
    let at = h.state().dict().window_rect().expect("window");
    let by = egui::vec2(-at.left() - at.width() * 0.5 - 8.0, 0.0);
    drag(&mut h, at.left_top() + egui::vec2(60.0, 18.0), by);
    h.run_steps(30);
    assert!(h.query_by_label("Restore dictionary: light").is_some());
    h.key_press(egui::Key::F6);
    h.run_steps(2);
    assert!(
        h.ctx
            .memory(|m| m.has_focus(egui::Id::new("dict-edge-tab-button")))
    );
    h.key_press(egui::Key::Enter);
    h.run_steps(30);
    assert!(h.query_by_label(MINIMIZE).is_some());
}

#[test]
fn disabling_edge_parking_keeps_the_dictionary_on_screen() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("set", "workspace.edge_parking:\"off\"")
        .expect("disabled parking setting");
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("dictionary window");
    h.run_steps(30);
    let at = h.state().dict().window_rect().expect("window");
    let by = egui::vec2(-at.left() - at.width() * 0.5 - 8.0, 0.0);
    drag(&mut h, at.left_top() + egui::vec2(60.0, 18.0), by);
    h.run_steps(30);
    let rect = h.state().dict().window_rect().expect("window");
    assert!(rect.left() >= 0.0, "{rect:?}");
    assert!(h.query_by_label(MINIMIZE).is_some());
}

#[test]
fn open_as_a_page_carries_the_search_over() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_window", "ight")
        .expect("a known option");
    h.run_steps(30);
    h.get_by_label(EXPAND).click();
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    assert_eq!(h.state().dict().page_text(), "ight");
    assert_eq!(window_open(&h), (false, false));
}

#[test]
fn escape_in_the_window_clears_then_closes() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_window", "quick")
        .expect("a known option");
    h.run_steps(30);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().dict().page_text(), "");
    assert_eq!(window_open(&h), (true, false));
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(window_open(&h), (false, false));
}

#[test]
fn the_tab_chooses_how_the_reader_opens_it() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().queue(Command::OpenSettings(Tab::Dictionary));
    h.run();
    h.get_by_label("Full page").click();
    h.run();
    let opens = h.state().settings().dictionary.opens;
    assert_eq!(opens, DictionaryOpens::Page);
}

#[test]
fn a_look_up_fills_the_open_window_and_keeps_the_selection() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_window", "show")
        .expect("a known option");
    h.run();
    h.get_by_label(MINIMIZE).click();
    h.run();
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(window_open(&h), (true, false), "a folded window comes back");
    let dict = h.state().dict();
    assert_eq!(dict.page_text(), "quick");
    assert_eq!(dict.card_word(), None, "no card over the page");
    assert!(!dict.typing(), "the reader keeps the keys");
    let pane = h.state().active_pane();
    assert!(h.state().documents().selection(pane).is_some());
}

#[test]
fn the_card_carries_its_word_into_the_window_or_the_page() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    h.get_by_label(PURSUE).click();
    h.run();
    assert_eq!(window_open(&h), (true, false));
    assert_eq!(h.state().dict().card_word(), None);
    assert_eq!(h.state().dict().page_text(), "quick");
    assert!(h.state().dict().typing(), "ready to search on");

    h.get_by_label(CLOSE).click();
    let mut s = h.state().settings().clone();
    s.dictionary.opens = DictionaryOpens::Page;
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    select(&mut h, "brown");
    h.get_by_label("Look up").click();
    h.run();
    h.get_by_label(PURSUE).click();
    h.run();
    assert_eq!(h.state().route(), Route::Dictionary);
    assert_eq!(h.state().dict().page_text(), "brown");
}

#[test]
fn the_shortcut_gives_an_idle_window_the_keys_then_closes_it() {
    let dir = shelf();
    let mut h = reader(dir.path());
    let keys = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
    h.key_press_modifiers(keys, egui::Key::D);
    h.run();
    assert!(h.state().dict().typing());
    // Back to the book: the window stays, idle.
    h.ctx.memory_mut(|m| m.stop_text_input());
    h.run();
    assert!(!h.state().dict().typing());
    h.key_press_modifiers(keys, egui::Key::D);
    h.run();
    assert_eq!(window_open(&h), (true, false), "fetched, not closed");
    assert!(h.state().dict().typing());
    h.key_press_modifiers(keys, egui::Key::D);
    h.run();
    assert_eq!(window_open(&h), (false, false));
}

#[test]
fn the_pill_goes_anywhere_and_unfolds_onto_the_stage() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut()
        .set_option("dict_window", "ight")
        .expect("a known option");
    h.run_steps(30);
    h.get_by_label(MINIMIZE).click();
    h.run();
    let pill = h.state().dict().window_rect().expect("a pill");
    assert!(pill.height() < 60.0, "{pill:?}");
    // To the bottom right, which the window couldn't reach; above the bar.
    drag(
        &mut h,
        pill.center(),
        egui::pos2(960.0, 795.0) - pill.center(),
    );
    let pill = h.state().dict().window_rect().expect("a pill");
    assert!(pill.right() > 975.0 && pill.bottom() > 735.0, "{pill:?}");
    assert!(
        pill.bottom() <= 800.0 - 40.0,
        "clear of the page bar: {pill:?}"
    );
    h.get_by_label(RESTORE).click();
    h.run();
    let open = h.state().dict().window_rect().expect("a window");
    assert!(open.height() >= 240.0, "{open:?}");
    assert!(open.right() <= 1000.0 && open.bottom() <= 800.0, "{open:?}");
    // It grew up and left from the pill instead of hanging off.
    assert!(
        (open.right() - pill.right()).abs() < 1.0,
        "{open:?} {pill:?}"
    );
    assert!(
        (open.bottom() - pill.bottom()).abs() < 1.0,
        "{open:?} {pill:?}"
    );
}

#[test]
fn a_bridge_names_english_and_each_answer_its_word() {
    let dir = shelf();
    let mut h = reader(dir.path());
    let mut s = h.state().settings().dictionary.clone();
    (s.pair, s.from, s.to) = (true, Some("fr".into()), Some("my".into()));
    h.state_mut().queue(Command::SetDictionary(Box::new(s)));
    h.run();
    h.state_mut()
        .set_option("dict_page", "lumière")
        .expect("a known option");
    h.run_steps(30);
    h.get_by_label("via English");
    assert!(h.query_all_by_label("via light").count() >= 1);
}

#[test]
fn the_card_says_its_word_in_the_reading_voice() {
    use mareader_ui::read_aloud::SAY;
    let dir = shelf();
    let mut h = reader(dir.path());
    let mut s = h.state().settings().clone();
    s.read_aloud.engine = "silent".into();
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.run();
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(h.state().read_aloud().words_said(), 0);
    h.get_by_label(SAY).click();
    h.run_steps(3);
    assert_eq!(h.state().read_aloud().words_said(), 1);
    h.run();
    assert_eq!(h.state().read_aloud().saying(), None, "the word ends");
    assert_eq!(
        h.state().dict().card_word(),
        Some("quick"),
        "the card stays"
    );
}

#[test]
fn near_matches_can_be_left_off_the_card() {
    let dir = shelf();
    let mut h = reader(dir.path());
    // No entry is "quic"; "quick" is near it.
    select(&mut h, "quic");
    h.get_by_label("Look up").click();
    h.run();
    let near = h.state().dict().card_answers().unwrap_or_default();
    assert!(!near.is_empty(), "the card offers the nearest entries");
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().queue(Command::OpenSettings(Tab::Dictionary));
    h.run();
    let switch = h.get_by_role_and_label(Role::CheckBox, "Show near matches");
    switch.scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Show near matches")
        .click();
    h.run();
    assert!(!h.state().settings().dictionary.near);
    h.state_mut().queue(Command::CloseSettings);
    h.run();
    select(&mut h, "quic");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(h.state().dict().card_answers(), Some(Vec::new()));
}

/// The window, English → French, searching "light" in `voice`; `others`
/// lends other languages' voices.
fn light_in(dir: &Path, (engine, voice): (&str, Option<&str>), others: bool) -> H {
    let mut h = reader(dir);
    let mut s = h.state().settings().clone();
    s.read_aloud.engine = engine.into();
    if let Some(voice) = voice {
        s.read_aloud.voices.insert(engine.into(), voice.into());
    }
    (s.dictionary.pair, s.dictionary.to) = (true, Some("fr".into()));
    s.dictionary.other_voices = others;
    h.state_mut().queue(Command::SetSettings(Box::new(s)));
    h.run();
    h.state_mut()
        .set_option("dict_window", "light")
        .expect("a known option");
    h.run_steps(30);
    assert!(h.query_all_by_label_contains("lumière").count() >= 1);
    h
}

fn says_in(engine: &str, voice: Option<&str>) -> usize {
    let dir = shelf();
    let h = light_in(dir.path(), (engine, voice), false);
    h.query_all_by_label(mareader_ui::read_aloud::SAY).count()
}

#[test]
fn only_words_the_voice_speaks_can_be_said() {
    // The silent voice speaks English: the headword alone.
    assert_eq!(says_in("silent", None), 1);
    // A French voice: the two French answers, not the English headword.
    assert_eq!(says_in("kokoro", Some("ff_siwis")), 2);
}

/// A voice models folder with Kokoro's English voice, its files stand-ins.
fn kokoro_here() -> tempfile::TempDir {
    use mareader_speech::KOKORO;
    let dir = tempfile::tempdir().expect("a temporary folder");
    let build = KOKORO.builds.first().map(|b| b.files()).unwrap_or_default();
    let voice = KOKORO
        .voice("af_heart")
        .map(|v| v.files())
        .unwrap_or_default();
    for f in KOKORO.core().into_iter().chain(build).chain(voice) {
        let path = dir.path().join(KOKORO.id).join(f.path);
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("made");
        let file = std::fs::File::create(&path).expect("a stand-in file");
        file.set_len(f.bytes).expect("its size");
    }
    dir
}

/// Counts who can be said in a system-like engine, before any voice spoke.
fn sayable(others: bool) -> usize {
    use mareader_ui::read_aloud::SAY;
    let dir = shelf();
    let mut h = light_in(dir.path(), ("silent", None), others);
    let before = h.query_all_by_label(SAY).count();
    // Listed once, the voices stay known frames later.
    h.run_steps(5);
    assert_eq!(h.query_all_by_label(SAY).count(), before);
    before
}

#[test]
fn another_languages_voice_says_its_words() {
    // The silent engine's French voice says both French answers.
    assert_eq!(sayable(true), 3);
    assert_eq!(sayable(false), 1);
}

#[test]
fn a_missing_voice_offers_its_download_and_reveals_it() {
    use mareader_speech::KOKORO;
    use mareader_ui::settings_window::{fold_open, voice_fold};
    const OFFER: &str = "Get a French voice to hear these";
    let dir = shelf();
    let models = kokoro_here();
    let path = models.path().to_string_lossy().into_owned();
    let mut h = light_in(dir.path(), ("kokoro", Some("af_heart")), true);
    assert_eq!(h.query_all_by_label(OFFER).count(), 0, "not without Kokoro");
    h.state_mut()
        .set_option("voice_models", &path)
        .expect("a known option");
    h.run_steps(5);
    // The English headword is said; French waits for its download.
    assert_eq!(
        h.query_all_by_label(mareader_ui::read_aloud::SAY).count(),
        1
    );
    assert_eq!(h.query_all_by_label(OFFER).count(), 1);
    let fold = voice_fold(&KOKORO, "fr-FR");
    assert!(!fold_open(&h.ctx, fold));
    h.get_by_label(OFFER).click();
    h.run();
    h.run_steps(5);
    assert_eq!(h.state().settings_window().tab(), Tab::Model(&KOKORO));
    assert!(fold_open(&h.ctx, fold), "the French voices unfold");
}

#[test]
fn dictionary_window_idling_in_corner_moves_aside_for_toast_and_restores() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().set_option("dict_window", "show").unwrap();
    h.run();
    let at = h.state().dict().window_rect().expect("a drawn window");
    // Drag down to the bottom right where toasts sit.
    drag(
        &mut h,
        at.left_top() + egui::vec2(60.0, 18.0),
        egui::vec2(200.0, 200.0),
    );
    let corner = h.state().dict().window_rect().expect("moved to corner");
    // A toast arrives while the dictionary is idling.
    h.state_mut().set_option("toast", "Saved").unwrap();
    h.run_steps(20);
    let aside = h.state().dict().window_rect().expect("stepped aside");
    assert!(aside.left() < corner.left(), "moved aside for toast");
    // Clearing the toast eases the window back to its place.
    h.state_mut().queue(Command::ClearToasts);
    h.run_steps(30);
    let back = h.state().dict().window_rect().expect("restored");
    assert!(
        (back.left() - corner.left()).abs() < 2.0,
        "restored to corner"
    );
}

#[test]
fn user_dragging_dictionary_window_into_toast_clears_it() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.state_mut().set_option("toast", "Exported").unwrap();
    h.state_mut().set_option("dict_window", "show").unwrap();
    h.run_steps(4);
    assert_eq!(h.state().toasts().0.len(), 1, "toast is up");
    let at = h.state().dict().window_rect().expect("a drawn window");
    // User manually drags the window into the toast corner.
    drag(
        &mut h,
        at.left_top() + egui::vec2(60.0, 18.0),
        egui::vec2(250.0, 250.0),
    );
    h.run_steps(4);
    assert_eq!(
        h.state().toasts().0.len(),
        0,
        "manual conflict sends toast away"
    );
}
