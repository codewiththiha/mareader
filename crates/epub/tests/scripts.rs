//! Scripted chapters: found, run by the script engine, and laid again.
use std::collections::BTreeMap;
use std::sync::Arc;

use mareader_epub::{Book, Destination, Inserts, Scripted};
use mareader_reflow::Typography;
use mareader_script::{Ask, Change, Engine, Page};

use crate::common::text;

const BOOK: &[u8] = include_bytes!("books/scripts.epub");

fn open() -> Book {
    Book::open(Arc::new(BOOK.to_vec()), &Typography::default()).unwrap()
}

fn page(s: &Scripted) -> Page {
    Page {
        chapter: s.chapter,
        xhtml: s.xhtml.clone(),
        scripts: s.code.clone(),
        styles: s.styles.clone(),
    }
}

/// The book and its scripts, as a reader opening it sees them.
struct Reading {
    book: Book,
    engines: Vec<Engine>,
    pages: BTreeMap<usize, String>,
}

impl Reading {
    fn new() -> Self {
        let book = open();
        let mut reading = Self {
            book,
            engines: Vec::new(),
            pages: BTreeMap::new(),
        };
        for s in reading.book.scripts() {
            let (engine, change) = Engine::new(&page(&s));
            assert!(change.errors.is_empty(), "{:?}", change.errors);
            reading.take(change);
            reading.engines.push(engine);
        }
        reading
    }

    fn take(&mut self, change: Change) {
        if let Some(p) = change.page {
            self.pages.insert(change.chapter, p);
        }
    }

    /// The book laid with the scripts' pages.
    fn laid(&self) -> Book {
        let list = self.book.relaid(&BTreeMap::new(), &self.pages);
        let inserts = Arc::new(Inserts {
            key: 1,
            list: list.into_iter().map(Arc::new).collect(),
        });
        self.book
            .reset(&Typography::default(), Some(&inserts))
            .unwrap()
    }

    /// Script links on the laid pages: chapter, node and the text they cover.
    fn links(&self) -> Vec<(usize, u32, String)> {
        let laid = self.laid();
        let mut out = Vec::new();
        for p in 0..laid.page_count() {
            let lines = laid.text(p);
            for link in laid.links(p) {
                let Destination::Script { chapter, node } = link.to else {
                    continue;
                };
                let [x, y, w, h] = link.rect;
                let inside = |[cx, cy, cw, ch]: [f32; 4]| {
                    let (cx, cy) = (cx + cw / 2.0, cy + ch / 2.0);
                    cx >= x && cx <= x + w && cy >= y && cy <= y + h
                };
                let chars = lines.iter().flat_map(|l| l.chars.iter());
                let covered: String = chars.filter(|c| inside(c.rect)).map(|c| c.ch).collect();
                out.push((chapter, node, covered.trim().to_owned()));
            }
        }
        out
    }

    fn click(&mut self, label: &str) {
        let links = self.links();
        let found = links.iter().find(|l| l.2.contains(label));
        let &(chapter, node, _) =
            found.unwrap_or_else(|| panic!("no link on {label:?}: {links:?}"));
        self.ask(chapter, Ask::Click(node));
    }

    fn ask(&mut self, chapter: usize, ask: Ask) {
        let engine = self.engines.iter_mut().find(|e| e.chapter() == chapter);
        let change = engine.expect("the chapter has scripts").ask(&ask);
        assert!(change.errors.is_empty(), "{:?}", change.errors);
        self.take(change);
    }
}

#[test]
fn scripted_chapters_are_found_with_their_code_and_sheets() {
    let book = open();
    let scripts = book.scripts();
    assert_eq!(scripts.len(), 2, "the plain chapter has none");
    assert_eq!(scripts[0].path, "/OEBPS/text/c1.xhtml");
    assert!(scripts[0].code[0].contains("DOMContentLoaded"));
    assert!(scripts[0].styles[0].contains(".answer { display: none"));
    assert!(scripts[1].code[0].contains("setTimeout"));
    // Before scripts run, the book reads as written: the answer is hidden.
    let plain = text(&book);
    assert!(plain.contains("Scripts are off"), "{plain}");
}

#[test]
fn a_quiz_answers_clicks_and_lays_its_pages_again() {
    let mut r = Reading::new();
    let shown = text(&r.laid());
    // Scripts run, so `noscript` goes; controls are drawn as text.
    assert!(!shown.contains("Scripts are off"), "{shown}");
    assert!(shown.contains('\u{25a1}'), "{shown}");
    assert!(shown.contains("type here"), "{shown}");

    // A list that handles its items' clicks: each item is a link.
    r.click("Lyon");
    assert!(text(&r.laid()).contains("Not quite, try again."));
    r.click("Paris");
    assert!(text(&r.laid()).contains("Correct!"));

    // The hidden answer shows, and hides again.
    let before = text(&r.laid());
    r.click("Show answer");
    let after = text(&r.laid());
    assert!(after.contains("Hide answer"), "{after}");
    assert!(
        after.lines().count() > before.lines().count(),
        "the answer takes room"
    );
    r.click("Hide answer");
    assert!(text(&r.laid()).contains("Show answer"));

    // A checkbox, through its label.
    r.click("I knew that");
    assert!(text(&r.laid()).contains('\u{25a0}'));

    // A text field takes what was typed; the check button reads it.
    let links = r.links();
    let field = links
        .iter()
        .find(|l| l.2.contains("type here"))
        .expect("the field");
    let (chapter, node) = (field.0, field.1);
    r.ask(chapter, Ask::Value(node, "Tokyo".into()));
    r.click("Check");
    let done = text(&r.laid());
    assert!(done.contains("Correct: Tokyo."), "{done}");
}

#[test]
fn widgets_count_and_say_things() {
    let mut r = Reading::new();
    r.click("Tap");
    r.click("Tap");
    let shown = text(&r.laid());
    assert!(shown.contains("Taps so far: 2"), "{shown}");
    let links = r.links();
    let reset = links.iter().find(|l| l.2 == "Reset").expect("reset");
    let engine = r
        .engines
        .iter_mut()
        .find(|e| e.chapter() == reset.0)
        .unwrap();
    let change = engine.ask(&Ask::Click(reset.1));
    assert_eq!(change.alerts, ["The counter is back to zero."]);
    r.take(change);
    assert!(text(&r.laid()).contains("Taps so far: 0"));
    // The plain chapter is laid as it was.
    assert!(text(&r.laid()).contains("A plain chapter with no scripts at all."));
}

/// Like a window: each new page is laid over the book the last one made.
#[test]
fn every_change_shows_when_laid_over_the_last() {
    let mut r = Reading::new();
    let mut twin = r.laid();
    for n in 1..=3u64 {
        r.click("Tap");
        let list = twin.relaid(&BTreeMap::new(), &r.pages);
        let inserts = Arc::new(Inserts {
            key: n + 1,
            list: list.into_iter().map(Arc::new).collect(),
        });
        twin = twin.reset(&Typography::default(), Some(&inserts)).unwrap();
        let shown = text(&twin);
        assert!(shown.contains(&format!("Taps so far: {n}")), "{shown}");
    }
}
