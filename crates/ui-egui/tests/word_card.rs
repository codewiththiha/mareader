//! Unified lookup and AI word card: tabs, swap, paging, and footer.

use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_dict::{PACKS, build, db_file};
use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::dict::card::PURSUE;
use mareader_ui_egui::ui_command::Sent;

use crate::common::click_at;

type H = Harness<'static, MareaderApp>;

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
        ("explain", "canned"),
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
    h.state_mut().queue(Sent::Engine(Command::SelectText {
        pane,
        page: 0,
        text: text.to_owned(),
    }));
    h.run();
}

#[test]
fn card_switches_tabs_in_place_swaps_languages_and_opens_word_list() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(h.state().dict().card_word(), Some("quick"));
    assert_eq!(h.state().study().card_word(), None);
    let before = h.state().dict().card_place().expect("drawn lookup card").0;

    // Switch to AI in context without closing and re-selecting.
    h.get_by_label("AI in context").click();
    h.run();
    assert_eq!(h.state().dict().card_word(), None);
    assert_eq!(h.state().study().card_word(), Some("quick"));
    h.get_by_label("Sample answers");
    h.get_by_label("Synonyms");

    // Switch back to Dictionary: restores previous answer in place.
    let tab = h
        .query_all_by_label("Dictionary")
        .find(|n| n.rect().top() > 50.0)
        .expect("Dictionary tab inside card")
        .rect()
        .center();
    click_at(&mut h, tab);
    h.run();
    assert_eq!(h.state().study().card_word(), None);
    assert_eq!(h.state().dict().card_word(), Some("quick"));
    assert_eq!(
        h.state().dict().card_answers(),
        Some(vec!["မြန်သော".to_owned()])
    );
    let after = h.state().dict().card_place().expect("restored card").0;
    assert!((after.left() - before.left()).abs() < 2.0);

    // Swap EN -> MY to MY -> EN via the compact swap pill.
    h.get_by_label("English → Burmese").click();
    h.run();
    h.get_by_label("Burmese → English");

    // Open the page's word list from the footer.
    h.get_by_label("Word list").click();
    h.run_steps(4);
    assert_eq!(h.state().dict().card_word(), None);
    assert!(h.query_by_label("Word list").is_some());
}

#[test]
fn multi_entry_card_pages_with_arrows() {
    let dir = shelf();
    let mut h = reader(dir.path());
    let md = OpenRequest::bytes(b"Run fast in the light.".to_vec(), "note.md");
    h.state_mut().queue(Sent::Engine(Command::Open(md)));
    h.run_steps(4);
    h.run();
    select(&mut h, "Run");
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(h.state().dict().card_word(), Some("Run"));
    h.get_by_label("Entry 1 of 2");
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.get_by_label("Entry 2 of 2");
    h.key_press(egui::Key::ArrowLeft);
    h.run();
    h.get_by_label("Entry 1 of 2");
}

#[test]
fn ai_card_switches_to_dictionary_and_pursues() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("AI Dictionary").click();
    h.run();
    assert_eq!(h.state().study().card_word(), Some("quick"));
    h.get_by_label(PURSUE).click();
    h.run();
    assert_eq!(h.state().study().card_word(), None);
    assert_eq!(h.state().dict().page_text(), "quick");
}
