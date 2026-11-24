//! End-to-end headless tests for `mareader-cli` and its six JSONL scripts.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::process::Command;

use mareader_engine::command_spec::SPECS;
use rusqlite::{Connection, params};
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_mareader-cli");

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn script(name: &str) -> PathBuf {
    manifest_dir().join("scripts").join(name)
}

fn raven_epub() -> PathBuf {
    manifest_dir().join("../../crates/epub/tests/books/the-raven-gutenberg-17192.epub")
}

/// Runs `mareader-cli` headless (no display env) and parses its JSON stdout.
fn run_cli(state_dir: Option<&Path>, args: &[&str], env: &[(&str, &Path)]) -> Value {
    let mut cmd = Command::new(BIN);
    cmd.env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("MAREADER_STATE_DIR");
    if let Some(dir) = state_dir {
        cmd.arg("--state-dir").arg(dir);
    }
    cmd.args(args);
    for (key, path) in env {
        cmd.env(key, path);
    }
    let out = cmd.output().expect("spawn mareader-cli");
    assert!(
        out.status.success(),
        "mareader-cli {:?} failed ({:?}):\nstdout: {}\nstderr: {}",
        args,
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    serde_json::from_slice(&out.stdout).expect("valid JSON stdout")
}

fn write_sample_pdf(path: &Path, pages: usize) {
    let bytes = mareader_pdf::sample::pdf(pages, "Sample PDF");
    std::fs::write(path, bytes).expect("write pdf");
}

fn write_sample_epub(path: &Path, chapters: usize) {
    let bytes = mareader_epub::sample::epub(chapters, "Sample EPUB");
    std::fs::write(path, bytes).expect("write epub");
}

fn write_vocab_db(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mkdir vocab");
    let db = Connection::open(dir.join("cefr.db")).expect("open cefr.db");
    let schema = "CREATE TABLE cefr (word TEXT NOT NULL, pos_tag TEXT, level REAL NOT NULL);";
    db.execute_batch(schema).expect("create table");
    let rows: &[(&str, &str, f64)] = &[
        ("the", "DT", 1.0),
        ("this", "DT", 1.0),
        ("page", "NN", 1.2),
        ("sphinx", "NN", 5.8),
        ("quartz", "NN", 5.6),
        ("vow", "NN", 5.4),
        ("helvetica", "NN", 5.2),
    ];
    let insert = "INSERT INTO cefr (word, pos_tag, level) VALUES (?1, ?2, ?3)";
    for (word, pos, level) in rows {
        db.execute(insert, params![word, pos, level]).unwrap();
    }
    let finish = "CREATE INDEX cefr_word ON cefr(word); ANALYZE; PRAGMA user_version = 1;";
    db.execute_batch(finish).expect("finalize cefr.db");
}

#[test]
fn commands_lists_every_registered_spec() {
    let out = run_cli(None, &["commands"], &[]);
    let items = out.as_array().expect("commands is an array");
    assert_eq!(items.len(), SPECS.len(), "every CommandSpec listed");
    for spec in SPECS {
        let row = items
            .iter()
            .find(|item| item["id"].as_str() == Some(spec.id))
            .unwrap_or_else(|| panic!("missing command {}", spec.id));
        assert!(!row["label"].as_str().unwrap_or_default().is_empty());
        assert!(row["params"].is_array());
        assert!(!row["routes"].as_array().unwrap().is_empty());
    }
}

#[test]
fn exec_and_inspect_persist_across_invocations() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let pdf = tmp.path().join("book.pdf");
    write_sample_pdf(&pdf, 6);

    let open_params = serde_json::json!({ "path": pdf }).to_string();
    let opened = run_cli(Some(&state), &["exec", "reader.open", &open_params], &[]);
    assert_eq!(opened["state"]["panes"][0]["page_count"], 6);

    run_cli(Some(&state), &["exec", "page.go", r#"{"page":2}"#], &[]);

    let inspected = run_cli(Some(&state), &["inspect"], &[]);
    assert_eq!(inspected["panes"][0]["page"], 2);
    assert_eq!(inspected["panes"][0]["page_count"], 6);
    let text = inspected["panes"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("Page 3 of 6"), "unexpected text: {text}");
}

#[test]
fn script_01_page_text_from_pdf_and_epub() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let pdf = tmp.path().join("sample.pdf");
    let epub = tmp.path().join("sample.epub");
    write_sample_pdf(&pdf, 3);
    write_sample_epub(&epub, 2);

    let path = script("01-page-text.jsonl");
    let out = run_cli(
        Some(&state),
        &["run", path.to_str().expect("utf8")],
        &[("PDF", &pdf), ("EPUB", &epub)],
    );
    assert_eq!(out["steps"], 4);
    let clips = out["clipboard"].as_array().expect("clipboard array");
    assert_eq!(clips.len(), 2, "copied PDF and EPUB pages: {clips:?}");
    let pdf_text = clips[0].as_str().expect("pdf text");
    let epub_text = clips[1].as_str().expect("epub text");
    assert!(pdf_text.contains("The quick brown fox"), "{pdf_text}");
    assert!(epub_text.contains("Chapter 1"), "{epub_text}");
    assert!(
        epub_text.contains("The river ran slow and brown"),
        "{epub_text}"
    );
}

#[test]
fn script_02_search_book() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let epub = raven_epub();

    let path = script("02-search-book.jsonl");
    let out = run_cli(
        Some(&state),
        &["run", path.to_str().expect("utf8")],
        &[("EPUB", &epub)],
    );
    assert_eq!(out["steps"], 2);
    let search = &out["state"]["panes"][0]["search"];
    assert_eq!(search["query"], "Nevermore");
    assert_eq!(search["done"].as_bool(), Some(true));
    let count = search["hit_count"].as_u64().unwrap_or(0);
    assert!(
        count >= 10,
        "expected >= 10 hits in The Raven, got {search}"
    );
    let snippet = search["hits"][0]["snippet"]
        .as_str()
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(snippet.contains("nevermore"), "snippet: {snippet}");
}

#[test]
fn script_03_highlight_note_and_export_markdown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let pdf = tmp.path().join("sample.pdf");
    write_sample_pdf(&pdf, 3);

    let path = script("03-highlight-note-export.jsonl");
    let out = run_cli(
        Some(&state),
        &["run", path.to_str().expect("utf8")],
        &[("PDF", &pdf)],
    );
    assert_eq!(out["steps"], 5);
    let marks = out["state"]["panes"][0]["marks"].as_array().expect("marks");
    assert_eq!(marks.len(), 1, "expected 1 mark: {marks:?}");
    assert_eq!(marks[0]["word"], "The quick brown fox");
    assert_eq!(marks[0]["note"], "Classic English pangram.");

    let exports = out["exports"].as_array().expect("exports");
    assert_eq!(exports.len(), 1, "expected 1 markdown export: {exports:?}");
    assert_eq!(exports[0]["format"], "md");
    let md = exports[0]["contents"].as_str().expect("markdown text");
    assert!(md.contains("> The quick brown fox"), "{md}");
    assert!(md.contains("**Note:** Classic English pangram."), "{md}");
    assert!(md.contains("*Page 1*"), "{md}");
}

#[test]
fn script_04_build_word_list() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let pdf = tmp.path().join("sample.pdf");
    let vocab = tmp.path().join("vocab");
    write_sample_pdf(&pdf, 2);
    write_vocab_db(&vocab);

    let path = script("04-word-list.jsonl");
    let out = run_cli(
        Some(&state),
        &["run", path.to_str().expect("utf8")],
        &[("PDF", &pdf), ("VOCAB_DIR", &vocab)],
    );
    assert_eq!(out["steps"], 3);
    assert_eq!(out["state"]["vocab"]["ready"].as_bool(), Some(true));
    let lists = out["word_lists"].as_array().expect("word_lists");
    assert_eq!(lists.len(), 1, "expected 1 word list: {out}");
    let words = lists[0]["words"].as_u64().unwrap_or(0);
    assert!(words >= 2, "expected >= 2 hard words: {lists:?}");
    let md = lists[0]["markdown"]
        .as_str()
        .expect("markdown")
        .to_ascii_lowercase();
    assert!(md.contains("sphinx"), "{md}");
    assert!(md.contains("quartz"), "{md}");
}

/// Settings with a model for study mode's cards; a deck's time runs only
/// on screen, so it never runs out here.
fn write_study_settings(state: &Path) {
    std::fs::create_dir_all(state).expect("mkdir state");
    let settings = serde_json::json!({
        "version": 7,
        "ai": {
            "presets": [
                {"id": 1, "name": "Local", "provider": "ollama", "model": "", "at_once": 1}
            ],
            "tasks": {"explain": {"preset": 1}},
        },
        "study": {"mode": {"unit": "pages"}},
    });
    std::fs::write(state.join("settings.json"), settings.to_string()).expect("write settings");
}

fn toasts(out: &Value) -> Vec<String> {
    let events = out["events"].as_array().expect("events");
    events
        .iter()
        .filter(|e| e["kind"] == "toast")
        .filter_map(|e| e["message"].as_str())
        .map(str::to_owned)
        .collect()
}

/// A converted dictionary pack holding the deck's hard words; a headless
/// run has no AI writer, so its cards come from here.
fn write_dict_pack(state: &Path) {
    let dir = state.join("dictionaries");
    std::fs::create_dir_all(&dir).expect("mkdir dictionaries");
    let db = Connection::open(dir.join("mcfnlp-en-my.db")).expect("open pack");
    let schema = "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE entries (id INTEGER PRIMARY KEY, word TEXT NOT NULL,
            word_fold TEXT NOT NULL, pos TEXT, definition TEXT NOT NULL,
            def_fold TEXT NOT NULL, romanization TEXT, sense TEXT);";
    db.execute_batch(schema).expect("create pack");
    let rows = [
        ("sphinx", "a statue that asks riddles"),
        ("quartz", "a hard clear mineral"),
        ("vow", "a solemn promise"),
    ];
    let insert = "INSERT INTO entries (word, word_fold, pos, definition, def_fold) \
        VALUES (?1, ?1, 'n', ?2, ?2)";
    for (word, meaning) in rows {
        db.execute(insert, params![word, meaning]).unwrap();
    }
    let meta = "INSERT INTO meta (key, value) VALUES ('layout', '1'), ('rows', '3');";
    db.execute_batch(meta).expect("finish pack");
}

#[test]
fn script_06_study_mode_gates_the_first_batch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let pdf = tmp.path().join("sample.pdf");
    let vocab = tmp.path().join("vocab");
    write_sample_pdf(&pdf, 6);
    write_vocab_db(&vocab);
    write_study_settings(&state);
    write_dict_pack(&state);

    let path = script("06-study-mode.jsonl");
    let out = run_cli(
        Some(&state),
        &["run", path.to_str().expect("utf8")],
        &[("PDF", &pdf), ("VOCAB_DIR", &vocab)],
    );
    assert_eq!(out["steps"], 8);
    let said = toasts(&out);
    let said_one = |start: &str| said.iter().any(|t| t.starts_with(start));
    assert!(said_one("Proactive study mode on"), "{said:?}");
    assert!(said_one("Locked. Study the words for"), "{said:?}");

    let pane = &out["state"]["panes"][0];
    let study = &pane["study"];
    assert_eq!(study["mode"], "proactive", "{study}");
    assert_eq!(
        study["unlocked"], 3,
        "Start unlocks the first batch: {study}"
    );
    assert_eq!(study["gate"], 3, "{study}");
    assert_eq!(pane["page"], 0, "Start shows the batch from its first page");
    assert_eq!(
        study["deck"], "open",
        "the next batch's deck is ready: {study}"
    );
    let cards: Vec<&str> = study["cards"]
        .as_array()
        .expect("cards")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        cards.contains(&"vow|noun"),
        "kept as learning, its class from the pack's short \"n\": {cards:?}"
    );
    let has = |lemma: &str| cards.iter().any(|c| c.split('|').next() == Some(lemma));
    assert!(
        !has("helvetica"),
        "a capital mid-sentence is a name: {cards:?}"
    );
    assert!(
        cards.contains(&"sphinx|noun"),
        "a capital opening a sentence is a word: {cards:?}"
    );
    assert!(!has("quartz"), "memorized words are skipped: {cards:?}");
}

fn backdate(path: &Path) {
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open fixture")
        .set_modified(old)
        .expect("backdate fixture mtime");
}

#[test]
fn script_05_import_folder_and_list_library() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = tmp.path().join("state");
    let books_dir = tmp.path().join("books");
    std::fs::create_dir_all(&books_dir).expect("mkdir books");
    let pdf = books_dir.join("sample.pdf");
    let epub = books_dir.join("the-raven.epub");
    write_sample_pdf(&pdf, 3);
    std::fs::copy(raven_epub(), &epub).expect("copy epub");
    backdate(&pdf);
    backdate(&epub);

    let path = script("05-import-folder.jsonl");
    let out = run_cli(
        Some(&state),
        &["run", path.to_str().expect("utf8")],
        &[("IMPORT_DIR", &books_dir)],
    );
    assert_eq!(out["steps"], 2);
    assert_eq!(out["state"]["route"], "library");
    let books = out["state"]["library"]["books"].as_array().expect("books");
    assert_eq!(books.len(), 2, "expected 2 imported books: {books:?}");
    let formats: Vec<&str> = books.iter().filter_map(|b| b["format"].as_str()).collect();
    assert!(formats.contains(&"pdf"), "{formats:?}");
    assert!(formats.contains(&"epub"), "{formats:?}");
    let folders = out["state"]["library"]["folders"]
        .as_array()
        .expect("folders");
    assert_eq!(folders.len(), 1, "{folders:?}");

    let reloaded = run_cli(Some(&state), &["inspect"], &[]);
    let kept = reloaded["library"]["books"].as_array().expect("books");
    assert_eq!(kept.len(), 2, "persisted library lost books: {kept:?}");
}
