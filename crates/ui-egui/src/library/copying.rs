//! The copy ask: books read in place that leave their folder become copies.

use egui::{RichText, Ui};
use mareader_library::{BookId, Departure, Library, Move, Origin, ShelfId, free_name};

use crate::modal::{buttons, primary, title};
use crate::theme::{self, Tokens};
use mareader_engine::library::counted;
use mareader_engine::library::{Action, ShelfAction};
use mareader_engine::library::{CopyAnswer, CopyWork};

const UNTOUCHED: &str = "The folder on disk is untouched.";

/// What the sheet says: the gesture, what it's about, and its cost.
struct Ask {
    action: String,
    subject: String,
    lines: Vec<String>,
    copy: &'static str,
    /// The answer that copies nothing, and what it does.
    without: Option<(String, &'static str)>,
}

/// The copy ask's sheet; Cancel leaves everything as it is.
pub fn show(ui: &mut Ui, model: &Library, work: &CopyWork, t: &Tokens) -> Option<Action> {
    let Some(ask) = words(model, work) else {
        return Some(Action::CloseSheet);
    };
    title(ui, "Copying books read in place", t);
    ui.label(
        RichText::new(&ask.action)
            .font(theme::medium(14.0))
            .color(t.ink),
    );
    ui.label(RichText::new(&ask.subject).size(12.5).color(t.muted));
    for line in &ask.lines {
        ui.label(RichText::new(line).color(t.ink));
    }
    ui.add_space(8.0);
    let answer = |a| Some(Action::Shelf(ShelfAction::Copy(a)));
    let mut act = None;
    buttons(ui, |ui| {
        let copy = primary(ui, ask.copy, t.accent, t)
            .on_hover_text("Copy the books read in place into the library, then finish");
        if copy.clicked() {
            act = answer(CopyAnswer::Copy);
        }
        if let Some((label, tip)) = &ask.without
            && ui.button(label).on_hover_text(*tip).clicked()
        {
            act = answer(CopyAnswer::Without);
        }
        let cancel = ui
            .button("Cancel")
            .on_hover_text("Leave everything as it is");
        if cancel.clicked() {
            act = Some(Action::CloseSheet);
        }
    });
    act
}

fn words(model: &Library, work: &CopyWork) -> Option<Ask> {
    match work {
        CopyWork::Move { mv, owed } if !owed.shelves.is_empty() => shelf_words(model, mv, owed),
        CopyWork::Move { owed, .. } => Some(book_words(model, &owed.books)),
        CopyWork::Apart(id) => apart_words(model, *id),
        CopyWork::Removal {
            shelves, copies, ..
        } => Some(removal_words(model, shelves, copies)),
    }
}

fn book_words(model: &Library, books: &[BookId]) -> Ask {
    let n = books.len();
    let lines = match n {
        1 => "It is read in place; moving it out stores a copy.",
        _ => "They are read in place; moving them out stores copies.",
    };
    Ask {
        action: "Move books".into(),
        subject: format!(
            "{} from “{}”",
            counted(n, "book", "books"),
            ground(model, books)
        ),
        lines: vec![lines.into(), UNTOUCHED.into()],
        copy: "Copy and move",
        without: None,
    }
}

/// Rungs off their seats: each lands under a copy's name.
fn shelf_words(model: &Library, mv: &Move, owed: &Departure) -> Option<Ask> {
    let mut promised: Vec<String> = (model.children(mv.to).iter())
        .map(|s| s.name.value.clone())
        .collect();
    let mut names = Vec::new();
    let mut copies = Vec::new();
    for shelf in owed.shelves.iter().filter_map(|s| model.shelf(*s)) {
        let own = shelf.name.value.clone();
        let taken = promised.iter().map(String::as_str).chain([own.as_str()]);
        let copy = free_name(&own, taken);
        promised.push(copy.clone());
        copies.push(format!("“{copy}”"));
        names.push(own);
    }
    let first = names.first()?;
    let one = names.len() == 1;
    let count = owed.carried.len();
    let cost = counted(count, "book", "books");
    let folder = rung_folder(model, owed.shelves.first().copied());
    let subject = match (one, count) {
        (true, 0) => format!("“{first}” — nothing read in place"),
        (true, _) => format!("“{first}” — {cost} from “{folder}”"),
        (false, 0) => counted(names.len(), "shelf", "shelves"),
        (false, _) => format!(
            "{} — {cost} read in place",
            counted(names.len(), "shelf", "shelves")
        ),
    };
    let cost_line = match (one, count) {
        (true, 0) => "Nothing on it is read in place, so no file is copied.".to_owned(),
        (true, _) => format!(
            "Moving it out stores its {cost}; the copy lands as {}.",
            copies.join(", ")
        ),
        (false, 0) => "Nothing on them is read in place, so no file is copied.".to_owned(),
        (false, _) => format!(
            "Moving them out stores their {cost}; the copies land as {}.",
            copies.join(", ")
        ),
    };
    let back = match owed.homeward.len() {
        0 => None,
        1 => Some("Put it back in its place"),
        _ => Some("Put them back in their places"),
    };
    let tip = "Return each shelf to the place its folder names; nothing is copied";
    Some(Ask {
        action: if one { "Move shelf" } else { "Move shelves" }.into(),
        subject,
        lines: vec![cost_line, UNTOUCHED.into()],
        copy: "Copy and move",
        without: back.map(|b| (b.to_owned(), tip)),
    })
}

fn apart_words(model: &Library, id: ShelfId) -> Option<Ask> {
    let rung = model.shelf(id)?;
    let books = model.rung_books(id, false);
    let home = match model.seat(id).flatten().and_then(|s| model.shelf(s)) {
        Some(seat) => format!("“{}”", seat.name.value),
        None => "the library's top level".into(),
    };
    let line = match books.len() {
        1 => format!("The book read in place here becomes a copy and comes up to {home}."),
        n => format!("The {n} books read in place here become copies and come up to {home}."),
    };
    Some(Ask {
        action: "Take shelf apart".into(),
        subject: format!(
            "“{}” — {} from “{}”",
            rung.name.value,
            counted(books.len(), "book", "books"),
            ground(model, &books)
        ),
        lines: vec![line, UNTOUCHED.into()],
        copy: "Copy and take apart",
        without: None,
    })
}

fn removal_words(model: &Library, shelves: &[ShelfId], copies: &[BookId]) -> Ask {
    let one = shelves.len() == 1;
    let folder = ground(model, copies);
    let n = copies.len();
    let kept = match (one, n) {
        (true, 1) => "Read in place, so removing it stores a copy.",
        (true, _) => "Read in place, so removing it stores copies.",
        (false, 1) => "Read in place, so removing them stores a copy.",
        (false, _) => "Read in place, so removing them stores copies.",
    };
    let (again, label) = match one {
        true => (
            format!("Or let “{folder}” make it again."),
            format!("Let “{folder}” make it again"),
        ),
        false => (
            "Or let the folders make them again.".to_owned(),
            "Let the folders make them again".to_owned(),
        ),
    };
    let tip = "Take the shelf off the list; its books read in place where they are";
    Ask {
        action: if one {
            "Remove shelf"
        } else {
            "Remove shelves"
        }
        .into(),
        subject: counted(shelves.len(), "shelf", "shelves"),
        lines: vec![kept.into(), again],
        copy: "Copy and remove",
        without: Some((label, tip)),
    }
}

/// The folder the first of these books is read in place from.
fn ground(model: &Library, books: &[BookId]) -> String {
    let folder = books
        .iter()
        .find_map(|b| match model.book(*b)?.home()?.origin {
            Origin::Folder(f) => model.folder(f),
            Origin::Store | Origin::Opened => None,
        });
    folder.map_or_else(|| "its folder".into(), |f| f.name())
}

/// The folder a rung was cut from.
fn rung_folder(model: &Library, shelf: Option<ShelfId>) -> String {
    let folder = shelf
        .and_then(|s| model.shelf(s))
        .and_then(|s| s.kind.value.rung())
        .and_then(|(f, _)| model.folder(f));
    folder.map_or_else(|| "its folder".into(), |f| f.name())
}
