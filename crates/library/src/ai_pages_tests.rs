//! AI pages: kept per task and span, removed and brought back, traded.

use crate::disk::Folder;
use crate::test_support::{opened, unsent};
use crate::tests::save;
use crate::{AiTask, DeviceId, Format, Kind, Library, NewAiPage, PageSpan};

const A: DeviceId = DeviceId(1);
const B: DeviceId = DeviceId(2);

fn french(page: usize, result: &str) -> NewAiPage {
    NewAiPage {
        task: AiTask::Translate {
            language: "French".into(),
        },
        span: PageSpan::Page { page },
        source: "00ff".into(),
        result: result.into(),
        by: "Gemini · flash".into(),
    }
}

fn results(lib: &Library, book: crate::BookId) -> Vec<String> {
    lib.ai_pages(book)
        .iter()
        .map(|p| p.result.clone())
        .collect()
}

#[test]
fn a_redo_replaces_the_same_task_on_the_same_page() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.epub", 1, Format::Epub, None), 1_000);
    let first = lib.add_ai_page(book, french(3, "one"), 2_000).unwrap();
    let summary = NewAiPage {
        task: AiTask::Summary,
        ..french(3, "gist")
    };
    lib.add_ai_page(book, summary, 2_100).unwrap();
    lib.add_ai_page(book, french(4, "next page"), 2_200)
        .unwrap();
    let again = lib.add_ai_page(book, french(3, "two"), 2_300).unwrap();
    assert_ne!(again, first);
    assert_eq!(results(&lib, book), ["gist", "next page", "two"]);
    assert!(lib.ai_page(first).is_none());
    assert_eq!(
        lib.add_ai_page(crate::BookId(9), french(1, "x"), 2_400),
        None
    );
}

#[test]
fn removing_one_is_a_tombstone_and_undo_brings_it_back() {
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.epub", 1, Format::Epub, None), 1_000);
    let id = lib.add_ai_page(book, french(0, "bonjour"), 2_000).unwrap();
    lib.take_dirty();
    assert_eq!(lib.set_ai_pages_alive(&[id], false, 3_000), 1);
    assert!(lib.ai_pages(book).is_empty());
    let kinds: Vec<Kind> = unsent(&mut lib).iter().map(|r| r.kind).collect();
    assert_eq!(kinds, [Kind::AiPage]);
    assert_eq!(lib.set_ai_pages_alive(&[id], false, 3_100), 0);
    assert_eq!(lib.set_ai_pages_alive(&[id], true, 3_200), 1);
    assert_eq!(results(&lib, book), ["bonjour"]);
}

#[test]
fn results_are_saved_read_back_and_travel() {
    let dir = tempfile::tempdir().unwrap();
    let folder = Folder::new(dir.path());
    let mut lib = folder.load(A).library;
    let book = lib.record_open(opened("/a.epub", 1, Format::Epub, None), 1_000);
    let span = PageSpan::Blocks {
        from: [3, 0],
        to: [5, 120],
    };
    let new = NewAiPage {
        span: span.clone(),
        task: AiTask::Points,
        ..french(0, "{\"outs\":[]}")
    };
    let id = lib.add_ai_page(book, new, 2_000).unwrap();
    save(&folder, &mut lib);
    let mut back = folder.load(A).library;
    let page = back.ai_page(id).unwrap();
    assert_eq!((&page.span, &page.task), (&span, &AiTask::Points));
    let mut other = Library::new(B);
    other.record_open(opened("/elsewhere/a.epub", 1, Format::Epub, None), 1_100);
    back.mark_all();
    other.absorb(unsent(&mut back), 2_500);
    assert_eq!(results(&other, book), ["{\"outs\":[]}"]);
    other.set_ai_pages_alive(&[id], false, 3_000);
    back.absorb(unsent(&mut other), 3_500);
    assert!(back.ai_pages(book).is_empty(), "the removal came across");
}

#[test]
fn a_group_of_pages_is_kept_in_its_own_shape() {
    let span = PageSpan::Pages { from: 2, to: 6 };
    let json = serde_json::to_string(&span).unwrap();
    assert_eq!(json, r#"{"pages":{"from":2,"to":6}}"#);
    let mut lib = Library::new(A);
    let book = lib.record_open(opened("/a.pdf", 1, Format::Epub, None), 1_000);
    let new = NewAiPage {
        span: span.clone(),
        ..french(0, "{}")
    };
    let id = lib.add_ai_page(book, new, 2_000).unwrap();
    assert_eq!(lib.ai_page(id).map(|p| &p.span), Some(&span));
}
