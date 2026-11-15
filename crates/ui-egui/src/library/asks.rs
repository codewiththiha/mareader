//! The question sheet: what each kind asks, and the answers it offers.

use egui::{Button, RichText, Ui};
use mareader_library::conflict::next_name;
use mareader_library::gate::Clash;
use mareader_library::screen::{FileAsk, Why};
use mareader_library::{Entry, Level, Library, MergeAsk, MoveAsk, Placement};

use crate::modal::title;
use crate::theme::Tokens;
use mareader_engine::library::Action;
use mareader_engine::library::counted;
use mareader_engine::library::{Kind, Question};

/// The front question's sheet; Cancel leaves every waiting one.
pub fn show(ui: &mut Ui, model: &Library, asks: &[&Question], t: &Tokens) -> Option<Action> {
    let question = asks.first()?;
    let kind = question.kind();
    let alike = asks.iter().skip(1).filter(|q| q.kind() == kind).count();
    let words = words(model, question);
    title(ui, &words.heading, t);
    let mut subtitle = words.subtitle.clone();
    if alike > 0 {
        subtitle = format!("{subtitle} · {alike} more waiting");
    }
    ui.label(RichText::new(subtitle).size(12.5).color(t.muted));
    ui.add_space(4.0);
    ui.label(RichText::new(&words.question).color(t.ink));
    ui.add_space(6.0);
    let key = egui::Id::new("library-ask-all");
    let mut all = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let mut act = None;
    for (answer, label, note) in &words.choices {
        let choice = ui.add(
            Button::new(RichText::new(*label).color(t.ink))
                .min_size(egui::vec2(ui.available_width(), 30.0)),
        );
        ui.label(RichText::new(note).size(12.0).color(t.muted));
        ui.add_space(2.0);
        if choice.clicked() {
            act = Some(Action::Answer(*answer, all));
        }
    }
    if alike > 0 && matches!(kind, Kind::Held | Kind::Merge) {
        let label = format!("Apply to all {}", alike + 1);
        ui.checkbox(&mut all, label)
            .on_hover_text("Give every waiting question this same answer");
    }
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let tip = match kind {
            Kind::Clash => "Import nothing",
            _ => "Leave the shelf as it is",
        };
        if ui.button("Cancel").on_hover_text(tip).clicked() {
            act = Some(Action::CancelAsks);
        }
    });
    // A new question starts with "apply to all" off.
    match act {
        Some(_) => ui.data_mut(|d| {
            d.remove::<bool>(key);
        }),
        None => ui.data_mut(|d| {
            d.insert_temp(key, all);
        }),
    }
    act
}

/// What a question's sheet says.
struct Words {
    heading: String,
    subtitle: String,
    question: String,
    choices: Vec<(Placement, &'static str, String)>,
}

fn words(model: &Library, question: &Question) -> Words {
    match question {
        Question::Clash { clash, .. } => clash_words(model, clash),
        Question::File(ask) => file_words(model, ask),
        Question::Merge(ask) => merge_words(model, ask),
        Question::Move(ask) => move_words(model, ask),
    }
}

fn move_words(model: &Library, ask: &MoveAsk) -> Words {
    let at = where_line(model, ask.arrival.level);
    let existing = &ask.existing_name;
    let fresh = free_on(model, ask.arrival.level, &ask.arrival.name);
    let marks = model.marks(ask.existing).len();
    let replace = match marks {
        0 => format!(
            "“{existing}” leaves the library — this one takes its place on every shelf it was on"
        ),
        n => format!(
            "“{existing}” leaves the library, with its {} — this one takes its place on every \
             shelf it was on",
            counted(n, "highlight", "highlights")
        ),
    };
    let merge = format!(
        "One book — “{existing}” stays, and takes this one's shelves, its highlights, and the \
         further place in it"
    );
    let fresh = format!("Keeps both — this one becomes “{fresh}”");
    if ask.converted {
        return Words {
            heading: ask.arrival.name.clone(),
            subtitle: format!("Already {at}"),
            question: format!(
                "A book called “{existing}” is already {at}, and it is one of the library's own \
                 copies. Keep one book, reach the copy from here, or keep both under a new name."
            ),
            choices: vec![
                (Placement::Merge, "Merge", merge),
                (
                    Placement::LinkOnly,
                    "Make link",
                    format!(
                        "The book you dragged becomes a pointer here — “{existing}” stays, the \
                         file on disk stays, and nothing is destroyed"
                    ),
                ),
                (Placement::KeepBoth, "As new", fresh),
            ],
        };
    }
    Words {
        heading: ask.arrival.name.clone(),
        subtitle: format!("Already {at}"),
        question: format!(
            "A book called “{existing}” is already {at}. Keep one book, keep this one instead, \
             or keep both under a new name."
        ),
        choices: vec![
            (Placement::Merge, "Merge", merge),
            (Placement::Replace, "Replace", replace),
            (Placement::KeepBoth, "As new", fresh),
        ],
    }
}

fn clash_words(model: &Library, clash: &Clash) -> Words {
    let existing = shelf_name(model, Some(clash.existing));
    let subtitle = match clash.own {
        true => format!("Already in the library as “{existing}”"),
        false => format!("A shelf called “{existing}” is already here"),
    };
    if clash.in_place {
        return Words {
            heading: clash.name.clone(),
            subtitle,
            question: "A folder read in place cannot mint a second shelf of itself. Leave a \
                       pointer to the shelf that is here, or file this folder's books into it."
                .into(),
            choices: vec![
                (
                    Placement::LinkOnly,
                    "Make link",
                    "A pointer row, not a second shelf: nothing is imported, and tapping it \
                     lights the folder where it is"
                        .into(),
                ),
                (
                    Placement::Merge,
                    "Merge into it",
                    format!(
                        "The folder's books join “{existing}” — a name it already holds asks \
                         one by one"
                    ),
                ),
            ],
        };
    }
    let question = match clash.own {
        true => format!(
            "“{existing}” is the shelf this folder's last import made. Look at it, replace its \
             books with these copies, or give the copies a shelf of the next free name."
        ),
        false => "The arriving copies are the library's own, so all three answers are open: look \
                  at the shelf that is here, replace its books, or shelve the copies under the \
                  next free name."
            .into(),
    };
    let leaving = model.shelf_books(clash.existing).len();
    let replace = match leaving {
        0 => format!("Nothing to remove — the copies simply take “{existing}”"),
        1 => {
            format!("One book leaves, highlights and all — a copy takes its place on “{existing}”")
        }
        n => format!("{n} books leave, highlights and all — copies take “{existing}”"),
    };
    let tops = model.children(None);
    let fresh = next_name(tops.iter().map(|s| s.name.value.as_str()), &clash.name);
    let reads = (model.shelf(clash.existing))
        .and_then(|s| s.kind.value.rung())
        .and_then(|(f, _)| model.folder(f))
        .is_some_and(|f| f.mode().in_place());
    let fresh = match reads {
        true => format!(
            "Import as “{fresh}” — the library's own copies; the tree here keeps reading the folder"
        ),
        false => format!("Import as “{fresh}” — its own shelf, its own tree"),
    };
    Words {
        heading: clash.name.clone(),
        subtitle,
        question,
        choices: vec![
            (
                Placement::Open,
                "Show it",
                format!("Import nothing — go to “{existing}” and light it up where it stands"),
            ),
            (Placement::Replace, "Replace", replace),
            (Placement::KeepBoth, "Add as new", fresh),
        ],
    }
}

fn file_words(model: &Library, ask: &FileAsk) -> Words {
    let level = ask.arrival.level;
    let at = where_line(model, level);
    let name = &ask.arrival.name;
    let existing = entry_name(model, ask.existing);
    let fresh = free_on(model, level, name);
    match ask.why {
        Why::Name => Words {
            heading: name.clone(),
            subtitle: format!("Already {at}"),
            question: format!(
                "“{name}” is already {at}. Add a second book of its own, put a link here \
                 instead, or go to the one you have."
            ),
            choices: vec![
                (
                    Placement::Open,
                    "Already imported",
                    format!("Add nothing — go to “{existing}” where it already is"),
                ),
                (
                    Placement::KeepBoth,
                    "Add as new",
                    format!("Keeps both, under the next free name — “{fresh}”"),
                ),
                (
                    Placement::LinkOnly,
                    "Make link",
                    "A pointer row, not a copy: tapping it goes to the book where it lives".into(),
                ),
            ],
        },
        Why::AlreadyHave => Words {
            heading: file_name(ask),
            subtitle: "Already in your library".into(),
            question: format!(
                "The library already holds this book as “{existing}”. Import your own copy \
                 {at}, or go to the one you have."
            ),
            choices: held_choices(
                &at,
                format!("Add nothing — go to “{existing}” and light it up"),
            ),
        },
        Why::Covered(tree) => {
            let tree = model.folder(tree).map(|f| f.name()).unwrap_or_default();
            Words {
                heading: file_name(ask),
                subtitle: format!("Inside “{tree}”"),
                question: format!(
                    "“{name}” is inside “{tree}”, which the library reads in place — one book \
                     already stands for it. Import your own copy {at}, or go to the book the \
                     folder holds."
                ),
                choices: held_choices(
                    &at,
                    format!("Add nothing — go to “{existing}” inside “{tree}” and light it up"),
                ),
            }
        }
    }
}

fn held_choices(at: &str, open: String) -> Vec<(Placement, &'static str, String)> {
    vec![
        (
            Placement::KeepBoth,
            "Import a copy here",
            format!(
                "The library's own copy — its own book {at}, its own highlights, its own place in it"
            ),
        ),
        (Placement::Open, "Show the imported one", open),
    ]
}

fn merge_words(model: &Library, ask: &MergeAsk) -> Words {
    let level = ask.arrival.level;
    let name = &ask.arrival.name;
    let existing = entry_name(model, ask.existing);
    let same = match ask.existing {
        Entry::Book(id) => model.book(id).and_then(|b| b.path()) == Some(ask.found.path.as_path()),
        Entry::Link(_) => false,
    };
    let question = match same {
        true => format!(
            "“{name}” is arriving, and “{existing}” on this shelf reads this very file. Keep the \
             one that is here, or seat this file in its place."
        ),
        false => format!(
            "“{name}” is arriving, and “{existing}” is already on this shelf. Keep the one that \
             is here, seat this file in its place, or keep both under a name of its own."
        ),
    };
    let fresh = free_on(model, level, name);
    Words {
        heading: name.clone(),
        subtitle: format!("Into “{}”", shelf_name(model, level)),
        question,
        choices: vec![
            (
                Placement::Merge,
                "Merge",
                format!("One book — “{existing}” stays, and takes this file's measurement"),
            ),
            (
                Placement::Replace,
                "Replace",
                "The row on the shelf leaves the library; this file takes its slot".into(),
            ),
            (
                Placement::KeepBoth,
                "Add as new",
                format!("Keep both — this file becomes “{fresh}”"),
            ),
        ],
    }
}

/// "in your library", or "on “Poems”".
fn where_line(model: &Library, level: Level) -> String {
    match level {
        None => "in your library".into(),
        Some(_) => format!("on “{}”", shelf_name(model, level)),
    }
}

fn shelf_name(model: &Library, level: Level) -> String {
    let shelf = level.and_then(|s| model.shelf(s));
    shelf.map_or_else(|| "this shelf".into(), |s| s.name.value.clone())
}

fn entry_name(model: &Library, entry: Entry) -> String {
    let name = match entry {
        Entry::Book(id) => model.book(id).map(|b| b.title.value.clone()),
        Entry::Link(id) => model.link(id).map(|l| l.name.value.clone()),
    };
    name.unwrap_or_default()
}

fn free_on(model: &Library, level: Level, name: &str) -> String {
    let rows = model.level_rows(level);
    next_name(rows.iter().map(|(_, n)| n.as_str()), name)
}

fn file_name(ask: &FileAsk) -> String {
    let name = ask.found.path.file_name().map(|n| n.to_string_lossy());
    name.map_or_else(|| ask.arrival.name.clone(), |n| n.into_owned())
}
