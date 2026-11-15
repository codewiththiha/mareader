//! A script replayed against the engine alone: no window, no toolkit.
//!
//! Open the sample PDF, split, go to a page, highlight, export the notes:
//! every step runs through `Session::execute` and `Session::execute_id`
//! against real documents and real engine-owned study marks.

use std::sync::Arc;

use mareader_appearance::Rgb;
use mareader_engine::command::Command;
use mareader_engine::command_spec::SPECS;
use mareader_engine::documents::OpenRequest;
use mareader_engine::event::Event;
use mareader_engine::export::{Entry, Format, Sheet, file_name, render};
use mareader_engine::host::Fake;
use mareader_engine::notes_panel::page_of;
use mareader_engine::route::Route;
use mareader_engine::session::Session;
use mareader_library::{MarkKind, Pen};
use mareader_workspace::{PaneId, SplitAxis};

const PAGES: usize = 6;

fn script() -> Session {
    let mut session = Session::headless(Arc::new(Fake::default()));
    let sample = mareader_pdf::sample::pdf(PAGES, "Sample");
    let open = Command::Open(OpenRequest::bytes(sample, "Sample.pdf"));
    session.execute(open, 0.0);
    session.pump();
    session.drain();
    session
}

fn page_text(session: &Session, pane: PaneId) -> String {
    let Some(page) = session.documents().current(pane) else {
        return String::new();
    };
    session
        .documents()
        .layer(pane, page)
        .map(|l| l.text.text(0..l.text.len()))
        .unwrap_or_default()
}

fn exported_marks(session: &Session, pane: PaneId) -> Vec<Entry> {
    let Some(doc) = session.documents().document_of(pane) else {
        return Vec::new();
    };
    let book = session.book_in(pane);
    let name = session.documents().title(pane).unwrap_or_default();
    let (lib, id) = session.study().lib_of(session.library_model(), book, name);
    lib.marks(id)
        .into_iter()
        .filter(|m| m.kind == MarkKind::Highlight)
        .map(|m| Entry {
            quote: m.word.clone(),
            note: None,
            page: page_of(&doc, m.spot),
            chapter: None,
            pen: m.pen,
            color: m.color.map_or(Rgb(255, 232, 102), |[r, g, b]| Rgb(r, g, b)),
            added: m.added,
        })
        .collect()
}

#[test]
fn a_script_opens_splits_turns_marks_and_exports_with_no_window() {
    let out = tempfile::tempdir().expect("a scratch folder");
    let mut session = script();
    let first = session.active();
    assert_eq!(session.documents().page_count(first), Some(PAGES));

    session.execute(Command::Split(SplitAxis::Horizontal), 1.0);
    let panes = session.tree().panes();
    assert_eq!(panes.len(), 2, "a split makes a pane");
    let second = session.active();
    assert_ne!(first, second, "and focuses it");
    assert_eq!(session.recent().first(), Some(&second));

    session.execute(Command::FocusPane(first), 2.0);
    let go = Command::GoTo {
        pane: first,
        page: 3,
    };
    session.execute(go, 3.0);
    assert_eq!(session.documents().current(first), Some(3));

    let text = page_text(&session, first);
    let word = text.split_whitespace().find(|w| w.len() > 2);
    let word = word.expect("the sample page has words").to_owned();
    let select = Command::SelectText {
        pane: first,
        page: 3,
        text: word.clone(),
    };
    session.execute(select, 4.0);
    let mark = Command::Highlight {
        pane: first,
        pen: Pen::default(),
        color: None,
    };
    session.execute(mark, 5.0);
    let marks = exported_marks(&session, first);
    assert_eq!(marks.len(), 1, "the selection became a mark");
    assert_eq!(marks.first().and_then(|m| m.page), Some(3));

    session.drain();
    let sheet = Sheet {
        title: "Sample",
        entries: &marks,
        now_ms: 0,
    };
    let name = file_name("Sample", Format::Markdown);
    let export = Command::Export {
        name: name.clone(),
        format: Format::Markdown,
        contents: render(&sheet, Format::Markdown),
    };
    session.execute(export, 6.0);
    let events = session.drain();
    assert_eq!(events.len(), 1, "export leaves as one event");
    let Some(Event::Export {
        name: out_name,
        format: Format::Markdown,
        contents,
    }) = events.into_iter().next()
    else {
        panic!("expected export event");
    };
    std::fs::write(out.path().join(&out_name), &contents).expect("write export");
    let written = std::fs::read_to_string(out.path().join(name)).expect("the export");
    assert!(written.contains(&word), "the notes carry the quote");
}

#[test]
fn the_library_page_drops_what_acts_on_panes() {
    let mut session = script();
    let pane = session.active();
    session.execute(Command::ShowRoute(Route::Library), 0.0);
    session.execute(Command::GoTo { pane, page: 4 }, 0.0);
    assert_eq!(session.documents().current(pane), Some(0));
    session.execute(Command::Split(SplitAxis::Horizontal), 0.0);
    assert_eq!(session.tree().panes().len(), 1);
}

#[test]
fn a_refused_split_and_a_failure_leave_as_toasts() {
    let mut session = script();
    for _ in 0..8 {
        session.execute(Command::Split(SplitAxis::Horizontal), 0.0);
    }
    let said = session.drain();
    assert!(!said.is_empty(), "the workspace is full, and says so");
    assert!(said.iter().all(|e| matches!(e, Event::Toast(_))));
    assert!(session.tree().panes().len() <= mareader_workspace::MAX_PANES);

    session.execute(Command::AutoScroll, 0.0);
    let said = session.drain();
    assert!(matches!(said.as_slice(), [Event::Toast(t)] if t.message.contains("scrolling")));
}

#[test]
fn opening_shows_the_reader_and_a_dial_is_written_by_its_path() {
    let mut session = Session::headless(Arc::new(Fake::default()));
    session.execute(Command::ShowRoute(Route::Library), 0.0);
    assert_eq!(session.drain(), vec![Event::RouteChanged(Route::Library)]);
    let sample = mareader_pdf::sample::pdf(PAGES, "Sample");
    let open = Command::Open(OpenRequest::bytes(sample, "Sample.pdf"));
    session.execute(open, 0.0);
    assert_eq!(session.route(), Route::Reader);
    assert_eq!(session.drain(), vec![Event::RouteChanged(Route::Reader)]);
    let params = r#"{"path":"chrome.sidebar_overlay","value":true}"#;
    session
        .execute_id("settings.set", params, 0.0)
        .expect("settings.set succeeds");
    assert!(session.settings().chrome.sidebar_overlay);
    session.execute(Command::SetSidebarOverlay(false), 0.0);
    assert!(!session.settings().chrome.sidebar_overlay);
}

#[test]
fn the_registry_lists_every_intent_once() {
    let ids: Vec<&str> = SPECS.iter().map(|s| s.id).collect();
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    assert_eq!(ids.len(), unique.len());
    let go = Command::GoTo {
        pane: mareader_workspace::PaneId(1),
        page: 0,
    };
    assert!(ids.contains(&go.id()), "a script can name a jump");
}
