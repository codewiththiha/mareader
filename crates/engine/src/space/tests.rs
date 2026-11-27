//! Space's rows: scopes, filters, tabs, groups, stats and the CSV.

use std::path::PathBuf;

use mareader_library::{ContentId, DeviceId, Library, Measure, NewWord, Opened};

use super::*;

/// 2026-10-17 and 2026-11-16, UTC.
const OCT: u64 = 1_792_225_800_000;
const NOV: u64 = OCT + 30 * 86_400_000;

fn book(lib: &mut Library, title: &str, content: u128) -> BookId {
    let opened = Opened {
        path: PathBuf::from(format!("/{title}.epub")),
        measure: Measure {
            content: ContentId(content),
            stamp: Default::default(),
        },
        format: Format::Epub,
        title: title.to_owned(),
        author: None,
    };
    lib.record_open(opened, 1_000)
}

fn noun(lemma: &str, band: u8) -> NewWord {
    NewWord {
        lemma: lemma.into(),
        pos: Some("noun".into()),
        band,
        form: lemma.into(),
        context: format!("The {lemma} stood by the door."),
        meaning: format!("a kind of {lemma}"),
        ..NewWord::default()
    }
}

/// Dune and Emma: five sightings of four words, one memorized.
fn shelf() -> Library {
    let mut lib = Library::new(DeviceId(1));
    let (dune, emma) = (book(&mut lib, "Dune", 1), book(&mut lib, "Emma", 2));
    lib.record_word(dune, noun("lintel", 5), OCT);
    lib.record_word(emma, noun("lintel", 5), NOV);
    let looked_up = NewWord {
        meaning_by: Some(MeaningBy::Dictionary),
        ..noun("sconce", 4)
    };
    let sconce = lib.record_word(dune, looked_up, NOV);
    let asked = |lemma, band, tries, first_try| NewWord {
        meaning_by: Some(MeaningBy::Ai),
        by: LearnedBy::Reactive,
        status: WordStatus::Missed,
        tries,
        first_try,
        ..noun(lemma, band)
    };
    lib.record_word(emma, asked("arcane", 6, 2, false), OCT);
    lib.record_word(dune, asked("gaunt", 3, 1, true), OCT);
    let memorized: Vec<WordId> = sconce.into_iter().collect();
    lib.set_word_status(&memorized, WordStatus::Memorized, NOV);
    lib
}

fn lemmas(view: &View) -> Vec<&str> {
    view.rows().map(|r| r.lemma.as_str()).collect()
}

fn labels(view: &View) -> Vec<&str> {
    view.sections.iter().map(|s| s.label.as_str()).collect()
}

fn az(scope: Scope) -> Filter {
    Filter {
        scope,
        group: Group::None,
        sort: Sort::Az,
        ..Filter::default()
    }
}

#[test]
fn with_books_a_row_is_a_sighting_and_merged_a_word() {
    let lib = shelf();
    let books = view(&lib, &az(Scope::Books));
    assert_eq!(
        lemmas(&books),
        ["arcane", "gaunt", "lintel", "lintel", "sconce"]
    );
    assert_eq!((books.total, books.counts.all), (5, 5));
    let merged = view(&lib, &az(Scope::Merged));
    assert_eq!(lemmas(&merged), ["arcane", "gaunt", "lintel", "sconce"]);
    let lintel = merged.rows().find(|r| r.lemma == "lintel");
    let lintel = lintel.expect("lintel is listed");
    assert_eq!(lintel.books, ["Emma", "Dune"], "the latest book first");
    let emma = merged
        .books
        .iter()
        .find(|b| b.title == "Emma")
        .map(|b| b.id);
    assert_eq!(lintel.book_ids.first().copied(), emma, "ids follow titles");
    let at = lintel
        .met_at
        .as_ref()
        .expect("its sentence is met somewhere");
    assert_eq!(
        (Some(at.book), at.quote.as_str()),
        (emma, "The lintel stood by the door.")
    );
    assert_eq!(lintel.seen, NOV);
    let titles: Vec<&str> = merged.books.iter().map(|b| b.title.as_str()).collect();
    assert_eq!(titles, ["Dune", "Emma"]);
    assert_eq!(merged.books[0].count, 3);
}

#[test]
fn merged_sentence_keeps_the_form_of_its_own_sighting() {
    let mut lib = Library::new(DeviceId(1));
    let older = book(&mut lib, "Older", 11);
    let newer = book(&mut lib, "Newer", 12);
    let earlier = NewWord {
        form: "ran".into(),
        context: "She ran home.".into(),
        ..noun("run", 2)
    };
    let later = NewWord {
        form: "run".into(),
        context: String::new(),
        ..noun("run", 2)
    };
    lib.record_word(older, earlier, OCT);
    lib.record_word(newer, later, NOV);
    let shown = view(&lib, &az(Scope::Merged));
    let row = shown.rows().next().expect("a merged word");
    assert_eq!((&*row.context, &*row.form), ("She ran home.", "ran"));
    assert_eq!(row.met_at.as_ref().map(|at| at.book), Some(older));
}

#[test]
fn the_tabs_count_every_filter_but_their_own() {
    let lib = shelf();
    let filter = Filter {
        tab: Tab::Memorized,
        ..az(Scope::Books)
    };
    let shown = view(&lib, &filter);
    assert_eq!(lemmas(&shown), ["sconce"]);
    let counts = shown.counts;
    assert_eq!((counts.all, counts.memorized), (5, 1));
    assert_eq!((counts.learning, counts.missed), (2, 2));
    assert_eq!(counts.of(Tab::Missed), 2);
}

#[test]
fn filters_narrow_by_book_level_class_learner_and_words() {
    let lib = shelf();
    let dune = view(&lib, &az(Scope::Books)).books[0].id;
    let only = |filter: Filter| {
        let shown = view(&lib, &filter);
        lemmas(&shown).join(" ")
    };
    let book = Filter {
        book: Some(dune),
        ..az(Scope::Books)
    };
    assert_eq!(only(book), "gaunt lintel sconce");
    let hard = Filter {
        levels: vec![6, 3],
        ..az(Scope::Books)
    };
    assert_eq!(only(hard), "arcane gaunt");
    let recalled = Filter {
        by: Some(LearnedBy::Reactive),
        ..az(Scope::Merged)
    };
    assert_eq!(only(recalled), "arcane gaunt");
    let verbs = Filter {
        pos: Some("verb".into()),
        ..az(Scope::Merged)
    };
    assert_eq!(only(verbs), "");
    let typed = Filter {
        text: "  KIND OF SCON ".into(),
        ..az(Scope::Merged)
    };
    assert_eq!(only(typed), "sconce");
    assert_eq!(view(&lib, &az(Scope::Books)).poses, ["noun"]);
}

#[test]
fn who_wrote_the_meaning_narrows_and_older_rows_show_only_for_any() {
    let lib = shelf();
    let only = |by: Option<MeaningBy>| {
        let filter = Filter {
            meaning_by: by,
            ..az(Scope::Merged)
        };
        lemmas(&view(&lib, &filter)).join(" ")
    };
    assert_eq!(only(Some(MeaningBy::Ai)), "arcane gaunt");
    assert_eq!(only(Some(MeaningBy::Dictionary)), "sconce");
    assert_eq!(only(None), "arcane gaunt lintel sconce");
}

#[test]
fn groups_follow_the_first_row_and_levels_go_hardest_first() {
    let lib = shelf();
    let level = Filter {
        group: Group::Level,
        ..az(Scope::Merged)
    };
    let bands = [6, 5, 4, 3].map(mareader_cefr::label);
    assert_eq!(labels(&view(&lib, &level)), bands);
    let days = Filter {
        group: Group::Day,
        sort: Sort::Recent,
        ..az(Scope::Books)
    };
    assert_eq!(labels(&view(&lib, &days)), ["Nov 16, 2026", "Oct 17, 2026"]);
    let month = Filter {
        group: Group::Month,
        sort: Sort::Recent,
        ..az(Scope::Books)
    };
    assert_eq!(
        labels(&view(&lib, &month)),
        ["November 2026", "October 2026"]
    );
    let book = Filter {
        group: Group::Book,
        ..az(Scope::Merged)
    };
    assert_eq!(
        labels(&view(&lib, &book)),
        [""],
        "merged rows have many books"
    );
    let tries = Filter {
        sort: Sort::Tries,
        ..az(Scope::Merged)
    };
    assert_eq!(lemmas(&view(&lib, &tries))[..2], ["arcane", "gaunt"]);
}

#[test]
fn stats_count_first_try_answers_and_memorized_levels() {
    let stats = view(&shelf(), &az(Scope::Books)).stats;
    assert_eq!((stats.first_try, stats.answered), (1, 2));
    assert_eq!(stats.first_try_percent(), 50);
    assert_eq!(stats.by_level, [(3, 0), (4, 1), (5, 0), (6, 0)]);
    assert_eq!(Stats::default().first_try_percent(), 0);
}

#[test]
fn the_csv_quotes_every_cell_and_names_each_book() {
    let lib = shelf();
    let filter = Filter {
        text: "lintel".into(),
        ..az(Scope::Merged)
    };
    let text = csv(&view(&lib, &filter));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "lemma,pos,level,meaning,context,book,status,date");
    let level = mareader_cefr::label(5);
    let row = format!(
        "\"lintel\",\"noun\",\"{level}\",\"a kind of lintel\",\
         \"The lintel stood by the door.\",\"Emma; Dune\",\"learning\",\"2026-11-16\""
    );
    assert_eq!(lines[1], row);
    assert_eq!(lines.len(), 2);
    assert_eq!(csv::quote("say \"hi\""), "\"say \"\"hi\"\"\"");
    assert_eq!(day(NOV), "Nov 16");
}

#[test]
fn the_seed_fills_every_tab_and_meets_two_words_twice() {
    let mut lib = Library::new(DeviceId(1));
    assert_eq!(seed(&mut lib, NOV), 0, "no book, nowhere to keep them");
    book(&mut lib, "Dune", 1);
    book(&mut lib, "Emma", 2);
    assert_eq!(seed(&mut lib, NOV), 10);
    let shown = view(&lib, &az(Scope::Merged));
    let counts = shown.counts;
    assert_eq!(counts.all, 8);
    assert!(Tab::ALL.iter().all(|&tab| counts.of(tab) > 0));
    assert!(shown.stats.answered > 0);
}

#[test]
fn a_sentence_is_found_by_the_words_around_its_word() {
    let long = "One two three four five six seven eight nine ten eleven.";
    assert_eq!(
        query::words_around(long, "Eight"),
        "five six seven eight nine ten eleven."
    );
    assert_eq!(
        query::words_around(long, "two"),
        "One two three four five six seven"
    );
    assert_eq!(
        query::words_around("Short, it said.", "absent"),
        "Short, it said."
    );
}
