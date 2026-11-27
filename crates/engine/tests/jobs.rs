//! Headless engine jobs: Waker notifications, `Session::tick`, and `Event`s.
//!
//! Exercises the engine's threaded controllers without a window or toolkit:
//! search, page copy, dictionaries, word lists, vocabulary, pronunciation
//! sounds, file watch, and read-aloud.
#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use mareader_cefr::{Part, State as VocabState};
use mareader_dict::{PACKS, Query, Reply, Ticket, build as build_pack, db_file};
use mareader_engine::command::Command;
use mareader_engine::documents::OpenRequest;
use mareader_engine::event::Event;
use mareader_engine::host::{Fake, Shared};
use mareader_engine::page_ai::Action as PageAiAction;
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::read_aloud::player::Act as ReadAct;
use mareader_engine::session::{Session, Waker};
use mareader_engine::watch::Watch;
use mareader_engine::word_list::{Ask, Levels, Meanings, WordLists};
use mareader_settings::WordList;
use mareader_speech::PRONUNCIATION;
use mareader_study::Pos;

const PAGES: usize = 6;
const WAIT: Duration = Duration::from_secs(5);
const STEP: Duration = Duration::from_millis(10);

fn counter_waker() -> (Arc<AtomicUsize>, Waker) {
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    let waker: Waker = Arc::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    });
    (wakes, waker)
}

fn open_sample(session: &mut Session) {
    let sample = mareader_pdf::sample::pdf(PAGES, "Sample");
    let open = Command::Open(OpenRequest::bytes(sample, "Sample.pdf"));
    session.execute(open, 0.0);
    let pane = session.active();
    let opened = wait_until(WAIT, || {
        session.pump();
        session.documents().page_count(pane).is_some()
    });
    assert!(opened, "sample PDF should finish opening");
    session.drain();
}

fn first_word(session: &Session, page: usize) -> String {
    let pane = session.active();
    let docs = session.documents();
    let layer = docs
        .layer(pane, page)
        .expect("sample page has a text layer");
    let text = layer.text.text(0..layer.text.len());
    text.split_whitespace()
        .find(|w| w.len() > 2 && w.chars().all(char::is_alphabetic))
        .expect("sample page has a word")
        .to_owned()
}

fn wait_until(limit: Duration, mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if check() {
            return true;
        }
        std::thread::sleep(STEP);
    }
    check()
}

struct AllHard;

impl Levels for AllHard {
    fn coming(&self) -> bool {
        false
    }

    fn ready(&self) -> bool {
        true
    }

    fn bands(&mut self, keys: &[String]) -> Vec<Option<u8>> {
        vec![Some(6); keys.len()]
    }
}

struct NoMeanings;

impl Meanings for NoMeanings {
    fn languages(&self) -> Vec<String> {
        Vec::new()
    }

    fn ask_words(&mut self, _words: Vec<String>, _to: Vec<String>, _near: bool) -> Ticket {
        Ticket(1)
    }

    fn take_words(&mut self, _ticket: Ticket) -> Option<Reply> {
        None
    }
}

#[test]
fn search_wakes_and_finds_matches_through_tick() {
    let (wakes, waker) = counter_waker();
    let mut session = Session::new(Arc::new(Fake::default()), waker);
    open_sample(&mut session);
    let pane = session.active();
    let doc = session.documents().place(pane).expect("open pane").doc;
    let document = session.documents().document_of(pane).expect("document");
    let query = first_word(&session, 0);

    wakes.store(0, Ordering::Relaxed);
    session.execute(
        Command::Search {
            pane,
            query: query.clone(),
            case_sensitive: false,
        },
        1.0,
    );
    let found = wait_until(WAIT, || {
        session.tick(1.0);
        session
            .search()
            .found(doc, &document)
            .is_some_and(|f| f.query == query && !f.hits.is_empty())
    });
    assert!(found, "search should find hits for {query:?}");
    assert!(wakes.load(Ordering::Relaxed) > 0, "search should wake");

    session.execute(
        Command::SelectText {
            pane,
            page: 0,
            text: query,
        },
        2.0,
    );
    assert!(session.documents().selection(pane).is_some());
    assert!(session.execute(Command::ClearSelection(pane), 2.0).dirty);
    assert!(!session.execute(Command::ClearSelection(pane), 2.0).dirty);
}

#[test]
fn page_copy_wakes_writes_host_clipboard_and_emits_toast() {
    let (wakes, waker) = counter_waker();
    let fake = Arc::new(Fake::default());
    let host: Shared = fake.clone();
    let mut session = Session::new(host, waker);
    open_sample(&mut session);
    let pane = session.active();

    wakes.store(0, Ordering::Relaxed);
    let pages = vec![0, 1, 2, 3, 4];
    session.execute(Command::CopyPages { pane, pages }, 1.0);
    let mut toast = false;
    let done = wait_until(WAIT, || {
        session.tick(1.0);
        for event in session.drain() {
            if matches!(event, Event::Toast(_)) {
                toast = true;
            }
        }
        toast
    });
    assert!(done, "page copy should emit a toast event");
    assert!(wakes.load(Ordering::Relaxed) > 0, "page copy should wake");
    let copied = fake.copied();
    assert_eq!(copied.len(), 1, "host clipboard receives copied pages");
    assert!(copied.first().is_some_and(|s| !s.trim().is_empty()));
}

/// A session with the en-my fixture pack, whose cells are short: "v".
fn with_en_my(dir: &Path) -> Session {
    let dict_dir = dir.join("dictionaries");
    std::fs::create_dir_all(&dict_dir).expect("create dictionaries dir");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    let pack = PACKS.iter().find(|p| p.id == "mcfnlp-en-my");
    let db = db_file(&dict_dir, pack.expect("en-my pack"));
    build_pack(&fixtures.join("en-my.parquet"), &db).expect("build pack");
    let host: Shared = Arc::new(Fake::under(dir));
    Session::new(host, Arc::new(|| {}))
}

#[test]
fn a_word_card_hears_its_class_from_short_dictionary_cells() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut session = with_en_my(dir.path());
    // No tagger is here, so the dictionary is the card's only source.
    session.ask_role("run", "They run home.");
    let mut heard = None;
    let done = wait_until(WAIT, || {
        session.tick(1.0);
        for event in session.drain() {
            if let Event::Class { word, pos } = event {
                heard = Some((word, pos));
            }
        }
        heard.is_some()
    });
    assert!(done, "the card's class should come through tick");
    assert_eq!(heard, Some(("run".to_owned(), Pos::Verb)), "\"v\" reads");
}

#[test]
fn dict_lookup_wakes_and_emits_dict_replies_through_tick() {
    let dir = tempfile::tempdir().expect("temp dir");
    let dict_dir = dir.path().join("dictionaries");
    std::fs::create_dir_all(&dict_dir).expect("create dictionaries dir");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dict/tests/fixtures");
    let pack = PACKS
        .iter()
        .find(|p| p.id == "mcfnlp-en-my")
        .expect("en-my pack");
    let src = fixtures.join("en-my.parquet");
    let db = db_file(&dict_dir, pack);
    build_pack(&src, &db).expect("build pack");

    let (wakes, waker) = counter_waker();
    let fake = Arc::new(Fake::under(dir.path()));
    let host: Shared = fake.clone();
    let mut session = Session::new(host, waker);
    assert!(session.dict().attached());

    wakes.store(0, Ordering::Relaxed);
    let ticket = session.dict_ask(Query::LookUp {
        word: "quick".into(),
        to: None,
    });
    let mut answer = None;
    let done = wait_until(WAIT, || {
        session.tick(1.0);
        for event in session.drain() {
            if let Event::DictReplies { replies, .. } = event {
                for reply in replies {
                    if reply.ticket == ticket {
                        answer = Some(reply);
                    }
                }
            }
        }
        answer.is_some()
    });
    assert!(done, "dict lookup should deliver Event::DictReplies");
    assert!(wakes.load(Ordering::Relaxed) > 0, "dict lookup should wake");
    let reply = answer.expect("reply received");
    assert!(
        reply.entries.iter().any(|e| e.answer() == "မြန်သော"),
        "expected Burmese translation in {:?}",
        reply.entries
    );
}

#[test]
fn word_list_background_job_wakes_and_builds_markdown() {
    let (wakes, waker) = counter_waker();
    let mut session = Session::new(Arc::new(Fake::default()), Arc::clone(&waker));
    open_sample(&mut session);
    let pane = session.active();
    let document = session.documents().document_of(pane).expect("document");

    wakes.store(0, Ordering::Relaxed);
    let mut lists = WordLists::default();
    lists.start(
        waker,
        Ask {
            document,
            title: "Sample".into(),
            pages: vec![0, 1, 2, 3, 4],
            level: 2,
            list: WordList::default(),
        },
    );
    let mut levels = AllHard;
    let mut meanings = NoMeanings;
    let mut made = None;
    let done = wait_until(WAIT, || {
        if let Some(result) = lists.poll(&mut levels, &mut meanings) {
            made = Some(result);
            return true;
        }
        false
    });
    assert!(done, "word list should finish");
    assert!(wakes.load(Ordering::Relaxed) > 0, "word list should wake");
    let (built, _) = made.expect("result").expect("word list succeeded");
    assert!(built.words > 0);
    assert!(!built.markdown.is_empty());

    // Without a vocabulary dataset, Command::WordList reports via Event::Toast.
    session.execute(
        Command::WordList {
            pane,
            pages: vec![0],
        },
        1.0,
    );
    let mut toasted = false;
    let notified = wait_until(WAIT, || {
        session.tick(1.0);
        toasted |= session
            .drain()
            .into_iter()
            .any(|e| matches!(e, Event::Toast(_)));
        toasted
    });
    assert!(notified, "missing vocab dataset emits a toast via tick");
}

#[test]
fn vocab_and_sounds_wire_from_host_and_poll_through_tick() {
    let dir = tempfile::tempdir().expect("temp dir");
    let voices = dir.path().join("voice-models");
    for f in PRONUNCIATION.files {
        let path = voices.join("kokoro").join(f.path);
        let parent = path.parent().expect("parent");
        std::fs::create_dir_all(parent).expect("mkdir");
        let file = std::fs::File::create(&path).expect("create");
        file.set_len(f.bytes).expect("set_len");
    }

    let (wakes, waker) = counter_waker();
    let fake = Arc::new(Fake::under(dir.path()));
    let host: Shared = fake.clone();
    let mut session = Session::new(host, waker);
    assert!(session.vocab().attached());
    assert_eq!(session.vocab().state(Part::Dataset), &VocabState::Absent);
    assert!(session.sounds().attached());

    let adopted = wait_until(WAIT, || {
        session.tick(1.0);
        session.sounds().ready()
    });
    assert!(adopted, "pronunciation pack should adopt Kokoro files");
    assert!(
        wakes.load(Ordering::Relaxed) > 0,
        "sounds adopt should wake"
    );
    assert!(session.sounds().shared_with().contains(&"Kokoro"));

    let other = tempfile::tempdir().expect("other vocab dir");
    session.execute(Command::AttachVocab(other.path().to_path_buf()), 2.0);
    session.ask_role("quick", "The quick brown fox.");
    session.tick(2.0);
    assert_eq!(session.vocab().state(Part::Dataset), &VocabState::Absent);
}

#[test]
fn watch_wakes_and_reports_modified_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("book.txt");
    std::fs::write(&file, b"v1").expect("seed file");

    let wakes = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&wakes);
    let mut watch = Watch::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    })
    .expect("file watch thread");
    watch.follow(vec![file.clone()]);
    std::thread::sleep(Duration::from_millis(150));

    std::fs::write(&file, b"version two with more bytes").expect("rewrite file");
    let mut seen = Vec::new();
    let found = wait_until(Duration::from_secs(6), || {
        seen.extend(watch.changed());
        seen.contains(&file)
    });
    assert!(found, "watch should report modified file");
    assert!(wakes.load(Ordering::Relaxed) > 0, "watch should wake");
}

#[test]
fn read_aloud_and_stop_pages_execute_through_session() {
    let (wakes, waker) = counter_waker();
    let mut session = Session::new(Arc::new(Fake::default()), waker);
    let set_engine = r#"{"path":"read_aloud.engine","value":"\"silent\""}"#;
    session
        .execute_id("settings.set", set_engine, 0.0)
        .expect("select silent read-aloud engine");
    assert_eq!(session.settings().read_aloud.engine, "silent");
    open_sample(&mut session);
    let pane = session.active();

    wakes.store(0, Ordering::Relaxed);
    session.execute(
        Command::ReadAloud {
            pane,
            from: ReadFrom::default(),
        },
        1.0,
    );
    let events = session.drain();
    let started = events
        .iter()
        .any(|e| matches!(e, Event::ReadAloudStarted { fresh: true }));
    assert!(started, "starting read-aloud emits ReadAloudStarted");
    assert!(session.read_aloud().active());

    let woke = wait_until(WAIT, || {
        session.tick(1.0);
        wakes.load(Ordering::Relaxed) > 0
    });
    assert!(woke, "read-aloud background thread should wake session");

    session.execute(Command::ReadAloudAct(ReadAct::Stop), 2.0);
    session.tick(2.0);
    assert!(!session.read_aloud().active());

    session.execute(Command::PageAi(PageAiAction::StopPages(pane)), 2.0);
    session
        .execute_id("mark.clear", "{}", 2.0)
        .expect("mark.clear");
    session
        .execute_id("read.act", r#"{"action":"stop"}"#, 2.0)
        .expect("read.act");
    session
        .execute_id("page.search", r#"{"query":"Sample"}"#, 2.0)
        .expect("page.search");
}
