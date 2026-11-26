//! Unified lookup and AI word card: tabs, swap, paging, and footer.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use egui::Pos2;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_dict::{PACKS, build, db_file};
use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_settings::CefrLevel;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::dict::card::PURSUE;
use mareader_ui_egui::ui_command::Sent;
use parquet::basic::{Compression, ZstdLevel};
use parquet::data_type::{ByteArray, ByteArrayType, FloatType};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;

use crate::common::{click_at, find, go_to, page};

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

fn reader_with(extra: &[(&str, &str)]) -> H {
    let mut app = MareaderApp::default();
    let defaults = [
        ("workers", "0"),
        ("animations", "false"),
        ("pdf", "sample:5"),
    ];
    for (k, v) in defaults.iter().chain(extra) {
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

fn cefr_shelf() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("cefr temp dir");
    let parquet = dir.path().join("words.parquet");
    let schema = Arc::new(
        parse_message_type(
            "message schema {
                REQUIRED BYTE_ARRAY word (UTF8);
                OPTIONAL BYTE_ARRAY pos_tag (UTF8);
                REQUIRED FLOAT level;
                OPTIONAL BYTE_ARRAY lemma (UTF8);
            }",
        )
        .expect("schema parses"),
    );
    let props = Arc::new(
        WriterProperties::builder()
            .set_compression(Compression::ZSTD(ZstdLevel::default()))
            .build(),
    );
    let mut w = SerializedFileWriter::new(File::create(&parquet).expect("create"), schema, props)
        .expect("writer");
    let mut rg = w.next_row_group().expect("row group");
    let words: Vec<ByteArray> = ["the", "quick", "brown", "fox"]
        .into_iter()
        .map(Into::into)
        .collect();
    let tags: Vec<ByteArray> = ["DT", "JJ", "JJ", "NN"]
        .into_iter()
        .map(Into::into)
        .collect();
    let def_some = [1i16; 4];
    let def_none = [0i16; 4];
    let levels = [1.0f32, 6.0, 1.0, 2.0];
    let mut c = rg.next_column().expect("c1").expect("col");
    c.typed::<ByteArrayType>()
        .write_batch(&words, None, None)
        .expect("words");
    c.close().expect("close");
    let mut c = rg.next_column().expect("c2").expect("col");
    c.typed::<ByteArrayType>()
        .write_batch(&tags, Some(&def_some), None)
        .expect("tags");
    c.close().expect("close");
    let mut c = rg.next_column().expect("c3").expect("col");
    c.typed::<FloatType>()
        .write_batch(&levels, None, None)
        .expect("levels");
    c.close().expect("close");
    let mut c = rg.next_column().expect("c4").expect("col");
    c.typed::<ByteArrayType>()
        .write_batch(&[], Some(&def_none), None)
        .expect("lemmas");
    c.close().expect("close");
    rg.close().expect("rg");
    w.close().expect("file");
    mareader_cefr::build(&parquet, &dir.path().join("cefr.db")).expect("build cefr");
    dir
}

#[test]
fn card_hides_tabs_when_one_side_lacks_data_and_offers_empty_actions() {
    let dir = shelf();
    let path = dir.path().to_string_lossy().into_owned();
    // Without AI, Dictionary card hides the tab switcher.
    let mut no_ai = reader_with(&[("dictionaries", path.as_str()), ("explain", "none")]);
    select(&mut no_ai, "quick");
    no_ai.get_by_label("Look up").click();
    no_ai.run();
    assert_eq!(no_ai.state().dict().card_word(), Some("quick"));
    assert!(no_ai.query_by_label("AI in context").is_none());

    // With AI and two packs, a missing entry hides tabs and offers CTAs.
    let mut h = reader_with(&[("dictionaries", path.as_str()), ("explain", "canned")]);
    select(&mut h, "quick brown");
    h.get_by_label("Look up").click();
    h.run();
    assert!(h.query_by_label("AI in context").is_none());
    h.get_by_label("No entry for “quick brown”.");
    h.get_by_label("Ask AI in context");
    h.get_by_label("Try French").click();
    h.run();
    h.get_by_label("English → French");
    h.get_by_label("Try Burmese");

    // Asking AI from the empty card switches to AI and hides the Dict tab.
    h.get_by_label("Ask AI in context").click();
    h.run();
    assert_eq!(h.state().study().card_word(), Some("quick brown"));
    let card_dict_tab = h
        .query_all_by_label("Dictionary")
        .any(|n| n.rect().top() > 50.0);
    assert!(
        !card_dict_tab,
        "AI card hides Dictionary tab when word has no dict entry"
    );
}

#[test]
fn hover_card_switched_to_ai_tracks_scroll_and_aligns_meta_row() {
    let packs = shelf();
    let words = cefr_shelf();
    let packs_s = packs.path().to_string_lossy().into_owned();
    let words_s = words.path().to_string_lossy().into_owned();
    let mut h = reader_with(&[
        ("dictionaries", packs_s.as_str()),
        ("cefr", words_s.as_str()),
        ("explain", "canned"),
    ]);
    let mut s = h.state().settings().clone();
    s.study.cefr.level = CefrLevel::B1;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    h.run();

    let (layer, sheet) = page(&h, 0).expect("page 0");
    let at = find(&layer, "quick").expect("quick");
    let word_pos = sheet.to_screen(layer.text.chars()[at + 2].bounds).center();
    h.hover_at(word_pos);
    h.run_steps(20);
    assert_eq!(h.state().dict().card_word(), Some("quick"));
    assert!(h.state().dict().card_rested());
    let role = mareader_cefr::Role {
        pos: "JJ".into(),
        kind: "adjective",
        level: Some(6.0),
    };
    h.state_mut().dict_mut().set_role("quick", role);
    h.run_steps(2);

    // Switching hover card to AI exits hover mode and anchors as a mark.
    h.get_by_label("AI in context").click();
    h.run();
    assert_eq!(h.state().dict().card_word(), None);
    assert_eq!(h.state().study().card_word(), Some("quick"));

    // Metadata row centers CEFR chip and Say aloud on the same centerline.
    let chip = h.get_by_label("C2").rect();
    let say = h.get_by_label("Say aloud").rect();
    assert!(
        (chip.center().y - say.center().y).abs() <= 2.0,
        "chip {chip:?} and say {say:?} should share vertical center"
    );

    // Moving pointer away does not dismiss (exited hover mode).
    h.hover_at(Pos2::new(5.0, 790.0));
    h.run_steps(3);
    assert_eq!(h.state().study().card_word(), Some("quick"));
    // Scrolling away to another page dismisses the anchored AI card.
    go_to(&mut h, 4);
    h.run_steps(3);
    assert_eq!(h.state().study().card_word(), None);
}

#[test]
fn selected_dictionary_tracks_scroll_and_closes_offscreen() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick brown");
    h.get_by_label("Look up").click();
    h.run();
    let (card, anchor) = h.state().dict().card_place().expect("lookup shown");
    assert!(
        card.width() >= 380.0,
        "card should use wider shell: {card:?}"
    );
    crate::common::scroll(&mut h, 45.0);
    let (_, moved) = h
        .state()
        .dict()
        .card_place()
        .expect("still shown after scroll");
    assert!(
        (moved.top() - anchor.top()).abs() > 5.0,
        "anchor should move: {anchor:?} -> {moved:?}"
    );
    go_to(&mut h, 4);
    h.run_steps(3);
    assert_eq!(h.state().dict().card_word(), None);
}

#[test]
fn dictionary_anchor_survives_round_trip_through_ai_tab() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    h.get_by_label("AI in context").click();
    h.run();
    let tab = h
        .query_all_by_label("Dictionary")
        .find(|n| n.rect().top() > 50.0)
        .expect("dictionary tab")
        .rect()
        .center();
    click_at(&mut h, tab);
    h.run();
    let before = h.state().dict().card_place().expect("restored").1;
    crate::common::scroll(&mut h, 45.0);
    let after = h.state().dict().card_place().expect("still open").1;
    assert!(
        (after.top() - before.top()).abs() > 5.0,
        "{before:?} -> {after:?}"
    );
    go_to(&mut h, 4);
    h.run_steps(3);
    assert_eq!(h.state().dict().card_word(), None);
}

#[test]
fn ai_origin_dictionary_tracks_its_mark() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("AI Dictionary").click();
    h.run();
    let tab = h
        .query_all_by_label("Dictionary")
        .find(|n| n.rect().top() > 50.0)
        .expect("dictionary tab")
        .rect()
        .center();
    click_at(&mut h, tab);
    h.run();
    let before = h
        .state()
        .dict()
        .card_place()
        .expect("dictionary tab open")
        .1;
    crate::common::scroll(&mut h, 45.0);
    let after = h.state().dict().card_place().expect("still open").1;
    assert!(
        (after.top() - before.top()).abs() > 5.0,
        "{before:?} -> {after:?}"
    );
    go_to(&mut h, 4);
    h.run_steps(3);
    assert_eq!(h.state().dict().card_word(), None);
}

#[test]
fn selected_book_word_tracks_reflow() {
    let dir = shelf();
    let path = dir.path().to_string_lossy().into_owned();
    let mut h = reader_with(&[("dictionaries", &path), ("epub", "sample:2")]);
    let pane = h.state().active_pane();
    let page = (0..8)
        .find(|&p| {
            h.state_mut().queue(Sent::Engine(Command::SelectText {
                pane,
                page: p,
                text: "customs".into(),
            }));
            h.run();
            h.state().documents().selection(pane).is_some()
        })
        .expect("sample contains customs");
    go_to(&mut h, page);
    h.run();
    h.get_by_label("Look up").click();
    h.run();
    assert_eq!(h.state().dict().card_word(), Some("customs"));
    let before = h.state().dict().card_place().expect("book card").1;
    h.state_mut()
        .set_option("text_size", "30")
        .expect("change text size");
    h.run_steps(10);
    h.run();
    let after = h
        .state()
        .dict()
        .card_place()
        .expect("book card follows reflow")
        .1;
    assert_ne!(before, after, "book anchor should change after reflow");
}

#[test]
fn word_card_fits_a_narrow_pane_in_both_tabs() {
    let dir = shelf();
    let mut h = reader(dir.path());
    h.set_size(egui::vec2(320.0, 700.0));
    h.run_steps(5);
    let md = OpenRequest::bytes(b"A quick fox jumps.".to_vec(), "narrow.md");
    h.state_mut().queue(Sent::Engine(Command::Open(md)));
    h.run_steps(5);
    h.run();
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    let card = h.state().dict().card_place().expect("narrow card").0;
    assert!(
        card.width() < 380.0 && card.left() >= 0.0 && card.right() <= 320.0,
        "{card:?}"
    );
    h.get_by_label("AI in context").click();
    h.run();
    assert_eq!(h.state().study().card_word(), Some("quick"));
    let close = h.get_by_label("Close").rect();
    assert!(close.left() >= 0.0 && close.right() <= 320.0, "{close:?}");
}

#[test]
fn replacing_a_document_does_not_attach_the_old_lookup() {
    let dir = shelf();
    let mut h = reader(dir.path());
    select(&mut h, "quick");
    h.get_by_label("Look up").click();
    h.run();
    let md = OpenRequest::bytes(b"Another quick sentence.".to_vec(), "new.md");
    h.state_mut().queue(Sent::Engine(Command::Open(md)));
    h.run_steps(6);
    h.run();
    assert_eq!(h.state().dict().card_word(), None);
}
