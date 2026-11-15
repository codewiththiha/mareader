#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use mareader_dict::WordMatch;

fn page(page: usize, chapter: Option<&str>, text: &str) -> Page {
    Page {
        page,
        chapter: chapter.map(String::from),
        text: text.into(),
    }
}

fn bands(of: &[(&str, u8)]) -> HashMap<String, u8> {
    of.iter().map(|(k, b)| ((*k).to_owned(), *b)).collect()
}

fn entry(word: &str, pos: &str, definition: &str) -> Entry {
    Entry {
        word: word.into(),
        pos: Some(pos.into()),
        definition: definition.into(),
        romanization: Some(format!("{definition}-rom")),
        sense: Some("a sense".into()),
        pack: "test",
        via: None,
        fit: WordMatch::Exact,
        flipped: false,
    }
}

fn found(word: &str, to: &str, own: Vec<Entry>, near: Vec<Entry>) -> ((String, String), Found) {
    let f = Found {
        word: word.into(),
        to: to.into(),
        own,
        near,
    };
    ((word.into(), to.into()), f)
}

/// Two chapters' pages with three hard words, one twice.
fn book() -> (Vec<Page>, HashMap<String, u8>) {
    let pages = vec![
        page(2, Some("One"), "The Ephemeral cat was obdurate."),
        page(3, Some("Two"), "An ephemeral zephyr | and a cat."),
    ];
    let known = bands(&[
        ("the", 1),
        ("ephemeral", 5),
        ("cat", 1),
        ("was", 1),
        ("obdurate", 6),
        ("an", 1),
        ("zephyr", 5),
        ("and", 1),
        ("a", 1),
    ]);
    (pages, known)
}

fn sheet<'a>(
    words: &'a [Word],
    found: &'a HashMap<(String, String), Found>,
    langs: &'a [String],
) -> Sheet<'a> {
    Sheet {
        title: "A Book",
        level: 4,
        words,
        found,
        languages: langs,
        pages: (2, 3),
    }
}

#[test]
fn keys_are_asked_once_each() {
    let (pages, _) = book();
    let keys = keys(&pages);
    assert_eq!(keys.iter().filter(|k| *k == "ephemeral").count(), 1);
    assert!(keys.contains(&"zephyr".to_owned()));
}

#[test]
fn hard_words_show_once_on_their_first_page() {
    let (pages, known) = book();
    let words = hard(&pages, &known, 4);
    let got: Vec<_> = words
        .iter()
        .map(|w| (w.key.as_str(), w.page, w.band))
        .collect();
    assert_eq!(
        got,
        [("ephemeral", 2, 5), ("obdurate", 2, 6), ("zephyr", 3, 5)]
    );
    assert_eq!(words[2].chapter.as_deref(), Some("Two"));
    assert!(hard(&pages, &known, 6).is_empty(), "nothing is above C2");
}

#[test]
fn a_table_has_a_column_per_language_in_order() {
    let (pages, known) = book();
    let words = hard(&pages, &known, 4);
    let found: HashMap<_, _> = [
        found(
            "ephemeral",
            "my",
            vec![entry("ephemeral", "adj", "yaayi")],
            vec![],
        ),
        found(
            "ephemeral",
            "jp",
            vec![entry("ephemeral", "adj", "ichiji")],
            vec![],
        ),
        found(
            "zephyr",
            "my",
            vec![entry("zephyr", "n", "le | pyay")],
            vec![],
        ),
    ]
    .into_iter()
    .collect();
    let langs = ["jp".to_owned(), "my".to_owned()];
    let list = WordList {
        group: WordGroup::None,
        ..WordList::default()
    };
    let md = render(&sheet(&words, &found, &langs), &list);
    assert!(
        md.starts_with("# A Book · words\n\n*3 words above B2 · pages 3–4*\n"),
        "{md}"
    );
    assert!(
        md.contains("| Word | Level | Part of speech | Japanese | Burmese | Page |"),
        "{md}"
    );
    assert!(
        md.contains("| **ephemeral** | C1 | adjective | ichiji | yaayi | 3 |"),
        "{md}"
    );
    assert!(md.contains("| **obdurate** | C2 |  | — | — | 3 |"), "{md}");
    assert!(md.contains("le \\| pyay"), "a bar stays text: {md}");
    assert!(!md.contains("## "), "no groups");
}

#[test]
fn fields_choose_what_a_row_says() {
    let (pages, known) = book();
    let words = hard(&pages, &known, 4);
    let found: HashMap<_, _> = [found(
        "ephemeral",
        "my",
        vec![entry("ephemeral", "adj", "yaayi")],
        vec![],
    )]
    .into();
    let langs = ["my".to_owned()];
    let mut list = WordList {
        group: WordGroup::None,
        pages: false,
        book: false,
        ..WordList::default()
    };
    list.fields = WordFields {
        level: false,
        part_of_speech: false,
        definition: true,
        romanization: true,
        sense: true,
    };
    let md = render(&sheet(&words, &found, &langs), &list);
    assert!(md.starts_with("# Word list\n"));
    assert!(md.contains("| Word | Burmese |"), "{md}");
    assert!(
        md.contains("| **ephemeral** | yaayi (yaayi-rom) — *a sense* |"),
        "{md}"
    );
}

#[test]
fn sorts_and_groups() {
    let (pages, known) = book();
    let words = hard(&pages, &known, 4);
    let found = HashMap::new();
    let order = |md: &str| {
        let at = |w: &str| md.find(&format!("**{w}**")).unwrap();
        let mut keys = ["ephemeral", "obdurate", "zephyr"];
        keys.sort_by_key(|w| at(w));
        keys
    };
    let with = |sort, group| {
        let list = WordList {
            sort,
            group,
            ..WordList::default()
        };
        render(&sheet(&words, &found, &[]), &list)
    };
    let md = with(WordSort::ZToA, WordGroup::None);
    assert_eq!(order(&md), ["zephyr", "obdurate", "ephemeral"]);
    let md = with(WordSort::Level, WordGroup::None);
    assert_eq!(order(&md), ["obdurate", "ephemeral", "zephyr"]);
    let md = with(WordSort::AToZ, WordGroup::Chapter);
    assert!(md.find("## One").unwrap() < md.find("## Two").unwrap());
    assert!(md.find("**zephyr**").unwrap() > md.find("## Two").unwrap());
    let md = with(WordSort::Reading, WordGroup::Page);
    assert!(
        md.contains("## Page 3\n") && md.contains("## Page 4\n"),
        "{md}"
    );
    assert!(
        md.contains("| Word | Level | Page |"),
        "no languages, no meanings: {md}"
    );
}

#[test]
fn chapters_without_an_outline_make_no_groups() {
    let pages = vec![page(0, None, "obdurate")];
    let words = hard(&pages, &bands(&[("obdurate", 6)]), 4);
    let md = render(&sheet(&words, &HashMap::new(), &[]), &WordList::default());
    assert!(!md.contains("## "), "{md}");
    assert!(md.contains("*1 word above B2"), "{md}");
}

#[test]
fn near_words_sit_under_the_word_that_found_them_once() {
    let (pages, known) = book();
    let words = hard(&pages, &known, 4);
    let near = vec![
        entry("ephemera", "n", "short-lived thing"),
        entry("zephyr", "n", "breeze"),
    ];
    let found: HashMap<_, _> = [
        found("ephemeral", "my", vec![], near.clone()),
        found(
            "obdurate",
            "my",
            vec![],
            vec![entry("ephemera", "n", "again")],
        ),
    ]
    .into();
    let langs = ["my".to_owned()];
    let list = WordList {
        similar: true,
        group: WordGroup::None,
        ..WordList::default()
    };
    let md = render(&sheet(&words, &found, &langs), &list);
    assert!(
        md.contains("| ↳ ephemera *(like ephemeral)* |  | noun | short-lived thing |  |"),
        "{md}"
    );
    assert_eq!(md.matches("↳ ephemera").count(), 1, "shown once: {md}");
    assert!(!md.contains("↳ zephyr"), "a book word is not near: {md}");
    assert!(md.ends_with("*↳ marks a dictionary word near the book word above it.*\n"));
    let list = WordList {
        layout: WordLayout::List,
        ..list
    };
    let md = render(&sheet(&words, &found, &langs), &list);
    assert!(md.contains("**ephemeral** · C1 · p. 3\n\n- *Not in the dictionary*\n- Near *ephemeral*:\n  - **ephemera** *(noun)* — Burmese: short-lived thing\n"), "{md}");
    assert!(!md.contains("↳"), "{md}");
}
