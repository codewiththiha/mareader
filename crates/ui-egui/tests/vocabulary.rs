//! Shading words above the reader's level, and the Vocabulary tab.

use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use egui::accesskit::Role;
use egui::{Event, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;
use mareader_cefr::{Part, State};
use mareader_dict::{PACKS, build as build_pack, db_file};
use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_engine::settings::Tab;
use mareader_settings::CefrLevel;
use mareader_ui_egui::MareaderApp;
use mareader_ui_egui::ui_command::Sent;
use parquet::basic::{Compression, ZstdLevel};
use parquet::data_type::{ByteArray, ByteArrayType, FloatType};
use parquet::file::properties::WriterProperties;
use parquet::file::writer::SerializedFileWriter;
use parquet::schema::parser::parse_message_type;

type H = Harness<'static, MareaderApp>;

/// "quick" is C2; the rest of its line is easy.
const ROWS: &[(&str, &str, f32)] = &[
    ("the", "DT", 1.0),
    ("quick", "JJ", 6.0),
    ("brown", "JJ", 1.0),
    ("fox", "NN", 2.0),
];

/// A parquet shaped like the real dataset.
fn dataset(path: &Path) {
    let message = "message schema {
        REQUIRED BYTE_ARRAY word (UTF8);
        OPTIONAL BYTE_ARRAY pos_tag (UTF8);
        REQUIRED FLOAT level;
        OPTIONAL BYTE_ARRAY lemma (UTF8);
    }";
    let schema = Arc::new(parse_message_type(message).unwrap());
    let zstd = Compression::ZSTD(ZstdLevel::default());
    let props = Arc::new(WriterProperties::builder().set_compression(zstd).build());
    let mut writer = SerializedFileWriter::new(File::create(path).unwrap(), schema, props).unwrap();
    let mut group = writer.next_row_group().unwrap();
    let words: Vec<ByteArray> = ROWS.iter().map(|r| r.0.into()).collect();
    let tags: Vec<ByteArray> = ROWS.iter().map(|r| r.1.into()).collect();
    let tagged = vec![1i16; ROWS.len()];
    let levels: Vec<f32> = ROWS.iter().map(|r| r.2).collect();
    let none = vec![0i16; ROWS.len()];
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&words, None, None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&tags, Some(&tagged), None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<FloatType>();
    typed.write_batch(&levels, None, None).unwrap();
    col.close().unwrap();
    let mut col = group.next_column().unwrap().unwrap();
    let typed = col.typed::<ByteArrayType>();
    typed.write_batch(&[], Some(&none), None).unwrap();
    col.close().unwrap();
    group.close().unwrap();
    writer.close().unwrap();
}

/// A folder with the word table converted, and one with the Burmese pack.
fn shelves() -> (tempfile::TempDir, tempfile::TempDir) {
    let words = tempfile::tempdir().unwrap();
    let parquet = words.path().join("source.parquet");
    dataset(&parquet);
    mareader_cefr::build(&parquet, &words.path().join("cefr.db")).unwrap();
    std::fs::remove_file(parquet).unwrap();
    let packs = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    let pack = PACKS.iter().find(|p| p.id == "mcfnlp-en-my").unwrap();
    build_pack(
        &fixtures.join("en-my.parquet"),
        &db_file(packs.path(), pack),
    )
    .unwrap();
    (words, packs)
}

fn reader(words: &Path, packs: Option<&Path>) -> H {
    let mut app = MareaderApp::default();
    let words = words.to_string_lossy().into_owned();
    let packs = packs.map(|p| p.to_string_lossy().into_owned());
    let mut options = vec![
        ("workers", "0"),
        ("explain", "canned"),
        ("animations", "false"),
        ("cefr", words.as_str()),
        ("pdf", "sample:5"),
    ];
    if let Some(packs) = &packs {
        options.push(("dictionaries", packs.as_str()));
    }
    for (k, v) in options {
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

/// The middle of the first `needle` on page 0, on screen.
fn spot(h: &H, needle: &str) -> Pos2 {
    let pane = h.state().active_pane();
    let docs = h.state().documents();
    let layer = docs.reading(pane).unwrap().document.text_layer(0).unwrap();
    let sheet = *docs.sheets(pane).iter().find(|s| s.page == 0).unwrap();
    let chars = layer.text.chars();
    let text: String = chars.iter().map(|c| c.ch).collect();
    let at = text.find(needle).unwrap();
    let i = text.get(..at).unwrap().chars().count() + needle.chars().count() / 2;
    sheet.to_screen(chars.get(i).unwrap().bounds).center()
}

/// A click on the middle of the first `needle` on page 0.
fn click(h: &mut H, needle: &str) {
    let pos = spot(h, needle);
    h.hover_at(pos);
    h.step();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
}

#[test]
fn a_click_on_a_shaded_word_explains_it_and_marks_it() {
    let (words, _) = shelves();
    let mut h = reader(words.path(), None);
    assert_eq!(h.state().vocab().state(Part::Dataset), &State::Ready);
    // An easy word is not shaded: a click there opens nothing.
    click(&mut h, "brown");
    assert_eq!(h.state().study().card_word(), None);
    click(&mut h, "quick");
    assert_eq!(h.state().study().card_word(), Some("quick"));
    let pane = h.state().active_pane();
    assert_eq!(h.state().marks(pane).len(), 1);
    // Off: the same word is plain text again.
    h.key_press(egui::Key::Escape);
    h.run();
    let mut s = h.state().settings().clone();
    s.study.cefr.click_explain = false;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    h.run();
    click(&mut h, "quick");
    assert_eq!(h.state().study().card_word(), None);
}

#[test]
fn resting_on_a_shaded_word_opens_its_dictionary_card() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    h.hover_at(spot(&h, "quick"));
    h.run_steps(20);
    let dict = h.state().dict();
    assert_eq!(dict.card_word(), Some("quick"));
    assert!(dict.card_rested());
    assert_eq!(dict.card_answers(), Some(vec!["မြန်သော".to_owned()]));
    // Leaving the word and its card closes it.
    h.hover_at(Pos2::new(5.0, 790.0));
    h.run_steps(3);
    assert_eq!(h.state().dict().card_word(), None);
    // With resting off, nothing opens.
    let mut s = h.state().settings().clone();
    s.dictionary.hover = false;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    h.run();
    h.hover_at(spot(&h, "quick"));
    h.run_steps(20);
    assert_eq!(h.state().dict().card_word(), None);
}

#[test]
fn a_dictionary_card_names_the_class_beside_its_word_as_the_ai_card_does() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    h.hover_at(spot(&h, "quick"));
    h.run_steps(20);
    assert_eq!(h.state().dict().card_word(), Some("quick"));
    let role = mareader_cefr::Role {
        pos: "JJ".into(),
        kind: "adjective",
        level: Some(3.0),
    };
    let level = role.cefr().unwrap();
    // A role for another word is not this card's.
    h.state_mut().dict_mut().set_role("brown", role.clone());
    assert!(h.state().dict().card_role().is_none());
    h.state_mut().dict_mut().set_role("quick", role);
    h.run_steps(3);
    let word = h.get_by_label("quick").rect();
    let class = h.get_by_label("adjective").rect();
    assert!(class.left() > word.right(), "after the word: {class:?}");
    assert!(
        (class.center().y - word.center().y).abs() < 8.0,
        "on its line"
    );
    assert!(h.query_by_label(level).is_some());
}

#[test]
fn the_tab_sets_the_level_and_removes_the_dataset() {
    use egui::accesskit::Role;
    let (words, _) = shelves();
    let mut h = reader(words.path(), None);
    h.set_size(egui::vec2(1000.0, 1200.0));
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Vocabulary)));
    h.run();
    h.get_by_label("Reading level");
    h.get_by_label("4 words");
    assert_eq!(
        h.state().settings().study.cefr.mark,
        mareader_settings::CefrMark::Wash
    );
    h.get_by_role_and_label(Role::Button, "Underline").click();
    h.run();
    assert_eq!(
        h.state().settings().study.cefr.mark,
        mareader_settings::CefrMark::Underline
    );
    h.get_by_role_and_label(Role::Button, "Custom").click();
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Shade words above my level")
        .click();
    // A pen's own colour is kept from shading.
    h.get_by_label("Shading #00b2f4").click();
    h.run();
    let study = &h.state().settings().study;
    assert!(!study.cefr.enabled);
    assert_eq!(study.cefr.level, CefrLevel::default());
    assert_eq!(study.cefr.mark, mareader_settings::CefrMark::Custom);
    assert_eq!(study.highlights.cefr.color.to_hex(), "#00b2f4");
    h.get_by_label("Remove word data").click();
    h.run();
    assert_eq!(h.state().vocab().state(Part::Dataset), &State::Absent);
    h.get_by_label("Download word data");
    // Without the data its options go, until it is back.
    assert!(h.query_by_label("Reading level").is_none());
    assert!(h.query_by_label("Shade words above my level").is_none());
}

#[test]
fn the_tab_offers_only_the_download_until_the_data_is_in() {
    let empty = tempfile::tempdir().unwrap();
    let mut h = reader(empty.path(), None);
    h.set_size(egui::vec2(1000.0, 1200.0));
    h.state_mut()
        .queue(Sent::Engine(Command::OpenSettings(Tab::Vocabulary)));
    h.run();
    h.get_by_label("Download word data");
    assert!(h.query_by_label("Word roles").is_none(), "one download");
    assert!(h.query_by_label("Reading level").is_none());
    assert!(h.query_by_label("Shading look").is_none());
}

/// What reaches the clipboard within `frames` frames.
fn copied(h: &mut H, frames: usize) -> Option<String> {
    for _ in 0..frames {
        h.step();
        let commands = &h.output().platform_output.commands;
        let text = commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyText(t) => Some(t.clone()),
            _ => None,
        });
        if text.is_some() {
            return text;
        }
    }
    None
}

fn word_list(h: &mut H, pages: Vec<usize>) -> Option<String> {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::WordList { pane, pages }));
    copied(h, 120)
}

#[test]
fn a_word_list_copies_the_hard_words_with_their_meanings() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    let md = word_list(&mut h, vec![0, 1, 2]).expect("a list on the clipboard");
    assert!(md.contains("*1 word above B2 · pages 1–3*"), "{md}");
    assert!(
        md.contains("| Word | Level | Part of speech | Burmese | Page |"),
        "{md}"
    );
    let row = md.lines().find(|l| l.starts_with("| **quick** |")).unwrap();
    assert!(row.contains("| C2 |") && row.contains("မြန်သော"), "{row}");
    assert!(row.ends_with("| 1 |"), "first seen on page 1: {row}");
    h.run();
    assert!(!h.state().making_word_list());
    let toast = h.query_by_label_contains("Copied a list of 1 word");
    assert!(toast.is_some());
}

#[test]
fn a_language_without_a_dictionary_is_left_out_and_named() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    let mut s = h.state().settings().clone();
    s.study.words.last.languages = vec!["jp".into(), "my".into()];
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    h.run();
    let md = word_list(&mut h, vec![0]).expect("a list on the clipboard");
    assert!(
        md.contains("| Burmese |") && !md.contains("Japanese"),
        "{md}"
    );
    h.run();
    let toast = h.query_by_label_contains("No Japanese dictionary yet");
    assert!(toast.is_some());
}

#[test]
fn a_word_list_says_when_no_word_is_hard() {
    let (words, _) = shelves();
    let mut h = reader(words.path(), None);
    let mut s = h.state().settings().clone();
    s.study.cefr.level = CefrLevel::C2;
    h.state_mut()
        .queue(Sent::Engine(Command::SetSettings(Box::new(s))));
    h.run();
    assert_eq!(word_list(&mut h, vec![0]), None);
    h.run();
    assert!(h.query_by_label_contains("No words above C2").is_some());
}

/// Opens the word list window on `pages` and waits for its list.
fn window(h: &mut H, pages: Vec<usize>) -> String {
    let pane = h.state().active_pane();
    h.state_mut()
        .queue(Sent::Engine(Command::WordListWindow { pane, pages }));
    for _ in 0..120 {
        h.step();
        if let Some(md) = h.state().word_window_markdown() {
            return md.to_owned();
        }
    }
    panic!("the list never came");
}

#[test]
fn the_window_previews_the_list_and_options_redraw_it() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    let md = window(&mut h, vec![0, 1]);
    assert!(md.contains("| **quick** | C2 |"), "{md}");
    h.run();
    assert!(h.query_by_label("Word list").is_some());
    // Below the fold of the options, so clicked as a screen reader would.
    h.get_by_role_and_label(Role::Button, "List")
        .click_accesskit();
    h.run();
    let md = h.state().word_window_markdown().unwrap();
    assert!(md.contains("**quick** · C2"), "a list now: {md}");
    let saved = &h.state().settings().study.words.last;
    assert_eq!(saved.layout, mareader_settings::WordLayout::List);
    // Removing the language leaves the words alone.
    h.get_by_label("Remove Burmese").click();
    h.run();
    let md = h.state().word_window_markdown().unwrap();
    assert!(!md.contains("Burmese") && md.contains("**quick**"), "{md}");
}

#[test]
fn a_preset_saves_the_options_and_can_drive_quick_export() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    window(&mut h, vec![0]);
    h.run();
    h.get_by_label("Save as preset…").click();
    h.run();
    // The name field takes the keys as it opens.
    h.event(Event::Text("Study".into()));
    h.run();
    h.get_by_label("Save").click();
    h.run();
    let words = &h.state().settings().study.words;
    assert_eq!(words.presets.len(), 1);
    assert_eq!(words.presets.first().unwrap().name, "Study");
    h.get_by_role_and_label(Role::CheckBox, "Use for quick export")
        .click();
    h.run();
    let words = &h.state().settings().study.words;
    assert_eq!(words.instant.as_deref(), Some("Study"));
}

#[test]
fn the_window_copies_its_list() {
    let (words, packs) = shelves();
    let mut h = reader(words.path(), Some(packs.path()));
    let md = window(&mut h, vec![0]);
    h.run();
    h.get_by_label("Copy").click();
    assert_eq!(copied(&mut h, 4).as_deref(), Some(md.as_str()));
}

/// Every clash egui warned of in the last frame.
fn clashes(h: &H) -> Vec<String> {
    fn walk(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(all) => all.iter().for_each(|s| walk(s, out)),
            egui::Shape::Text(t) if t.galley.job.text.contains(" ID ") => {
                out.push(t.galley.job.text.clone());
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    h.output()
        .shapes
        .iter()
        .for_each(|c| walk(&c.shape, &mut out));
    out
}

#[test]
fn one_book_in_two_panes_keeps_its_words_apart() {
    let (words, _packs) = shelves();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("Twice.pdf");
    std::fs::write(&path, mareader_pdf::sample::pdf(3, "Twice")).unwrap();
    let path = path.to_string_lossy().into_owned();
    let words = words.path().to_string_lossy().into_owned();
    let mut app = MareaderApp::default();
    let options = [
        ("workers", "0"),
        ("animations", "false"),
        ("cefr", words.as_str()),
        ("panes", "2"),
        ("focus", "1"),
        ("pdf", path.as_str()),
    ];
    for (k, v) in options {
        app.set_option(k, v).expect("a known option");
    }
    let mut h = Harness::builder()
        .with_size(egui::vec2(1400.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(200)
        .build_eframe(|_| app);
    h.run_steps(3);
    h.run();
    // Opened again once shown, as the player's reopen does, it's shared.
    let panes = h.state().panes();
    let second = *panes.get(1).unwrap();
    let request = OpenRequest {
        pane: Some(second),
        ..OpenRequest::path(path.into())
    };
    h.state_mut().queue(Sent::Engine(Command::Open(request)));
    h.run_steps(3);
    h.run();
    let docs = h.state().documents();
    let shown: Vec<_> = panes.iter().filter_map(|p| docs.reading(*p)).collect();
    assert_eq!(shown.len(), 2);
    assert_eq!(shown.first().map(|r| r.doc), shown.get(1).map(|r| r.doc));
    assert_eq!(h.state().vocab().state(Part::Dataset), &State::Ready);
    // Its shaded words, links and marks take Tab in each pane apart.
    for _ in 0..10 {
        h.step();
        let found = clashes(&h);
        assert!(found.is_empty(), "{found:#?}");
    }
}
