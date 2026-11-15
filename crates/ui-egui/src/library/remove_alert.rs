//! Removal's alert: one line of meaning, then Remove.

use egui::{Context, Id, Key};
use mareader_library::{BookId, Library, LinkId, Receipt, Removing, ShelfId};

use crate::icons::Icon;
use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use mareader_engine::library::listed;
use mareader_engine::library::{Action, BookAction};

const KEY: &str = "library-remove";
/// Fixed, so the box never jumps as its words change.
const WIDTH: f32 = 440.0;

/// Both start off on each opening: erasing is never a habit.
#[derive(Clone, Copy, Default)]
struct Switches {
    cascade: bool,
    forget: bool,
}

type Asked<'a> = (&'a [BookId], &'a [ShelfId], &'a [LinkId]);

/// The alert; Enter removes, Escape or Cancel keeps everything.
pub(super) fn show(ctx: &Context, model: &Library, asked: Asked<'_>, t: &Tokens) -> Option<Action> {
    let key = Id::new(KEY);
    let mut on = ctx
        .data(|d| d.get_temp::<Switches>(key))
        .unwrap_or_default();
    let (books, shelves, links) = asked;
    let going = model.removing(books, shelves, on.cascade);
    let receipt = model.receipt(&going.books);
    let mut act = None;
    let modal = Dialog::new(key.with("modal")).show(ctx, |ui| {
        let width = WIDTH.min(ui.ctx().content_rect().width() - 64.0);
        ui.set_width(width.max(200.0));
        ui.spacing_mut().item_spacing.y = 12.0;
        let message = message(asked, &going, &receipt);
        let mark = Some((Icon::Remove, t.danger));
        modal::heading(ui, mark, &title(model, asked), message.as_deref(), t);
        if going.inside_books + going.inside_shelves > 0 {
            let note = inside(&going, on.cascade);
            modal::switch_card(
                ui,
                &mut on.cascade,
                "Remove everything inside",
                &note,
                None,
                t,
            );
        }
        let data = receipt.has_data();
        if data {
            let note = match (on.forget, going.books.len()) {
                (true, _) => "Erased for good. This can't be undone.",
                (false, 1) => "Kept for when the book comes back.",
                (false, _) => "Kept for when the books come back.",
            };
            let label = "Erase highlights, AI pages and reading position";
            modal::switch_card(ui, &mut on.forget, label, note, Some(t.danger), t);
        }
        let erase = data && on.forget;
        ui.horizontal(|ui| {
            modal::buttons(ui, |ui| {
                let label = if erase { "Erase & remove" } else { "Remove" };
                let free = ui.ctx().memory(|m| m.focused().is_none());
                let enter = free && ui.input(|i| i.key_pressed(Key::Enter));
                if modal::primary(ui, label, t.danger, t).clicked() || enter {
                    act = Some(Action::Book(BookAction::Remove {
                        books: books.to_vec(),
                        shelves: shelves.to_vec(),
                        links: links.to_vec(),
                        cascade: on.cascade,
                        forget: erase,
                    }));
                }
                if modal::quiet(ui, "Cancel", t).clicked() {
                    act = Some(Action::CloseSheet);
                }
            });
        });
    });
    if act.is_none() && modal.should_close() {
        act = Some(Action::CloseSheet);
    }
    match act {
        Some(_) => ctx.data_mut(|d| d.remove::<Switches>(key)),
        None => ctx.data_mut(|d| {
            d.insert_temp(key, on);
        }),
    }
    act
}

/// What was asked for, by name when it's one thing.
fn title(model: &Library, (books, shelves, links): Asked<'_>) -> String {
    let book = |id| model.book(id).map(|b| b.title.value.clone());
    let shelf = |id| model.shelf(id).map(|s| s.name.value.clone());
    let link = |id| model.link(id).map(|l| l.name.value.clone());
    let named = match (books, shelves, links) {
        ([one], [], []) => book(*one).map(|n| format!("Remove “{n}”?")),
        ([], [one], []) => shelf(*one).map(|n| format!("Remove the shelf “{n}”?")),
        ([], [], [one]) => link(*one).map(|n| format!("Remove the link “{n}”?")),
        _ => None,
    };
    named.unwrap_or_else(|| {
        let counts = [
            (books.len(), "book", "books"),
            (shelves.len(), "shelf", "shelves"),
            (links.len(), "link", "links"),
        ];
        format!("Remove {}?", listed(&counts))
    })
}

/// The one thing worth knowing first: what happens to the files.
fn message(asked: Asked<'_>, going: &Removing, receipt: &Receipt) -> Option<String> {
    let (books, shelves, links) = asked;
    let n = going.books.len();
    if n == 0 {
        return match (shelves.len(), links.len()) {
            (0, 1) => Some("The book it points to stays.".to_owned()),
            (0, _) => Some("The books they point to stay.".to_owned()),
            _ if going.inside_books + going.inside_shelves > 0 => None,
            (1, _) => Some("Nothing is inside it.".to_owned()),
            _ => Some("Nothing is inside them.".to_owned()),
        };
    }
    let one = n == 1 && books.len() == 1;
    let files = match (receipt.copies, one) {
        (0, true) => "The file stays on disk.",
        (0, false) => "The files stay on disk.",
        (c, true) if c >= n => "The library's copy is deleted.",
        (c, false) if c >= n => "The library's copies are deleted.",
        _ => "Library copies are deleted; files read in place stay.",
    };
    Some(match (receipt.hidden, one) {
        (0, _) => files.to_owned(),
        (_, true) => format!("{files} Its folder won't add it back."),
        (_, false) => format!("{files} Their folders won't add them back."),
    })
}

/// What the inside switch means, on or off.
fn inside(going: &Removing, cascade: bool) -> String {
    let (b, s) = (going.inside_books, going.inside_shelves);
    let what = listed(&[(b, "book", "books"), (s, "shelf", "shelves")]);
    let verb = if b + s == 1 { "goes" } else { "go" };
    match (cascade, b, s) {
        (true, ..) => format!("{what} {verb} too."),
        (false, _, 0) => "Off, its books stay in the library.".to_owned(),
        (false, 0, _) => "Off, the shelves inside move up.".to_owned(),
        (false, ..) => "Off, books stay and shelves inside move up.".to_owned(),
    }
}
