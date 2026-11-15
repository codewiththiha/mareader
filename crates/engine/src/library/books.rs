//! Book work from menus, the selection bar and drops: removal,
//! twins and imports.

use mareader_library::{LinkId, Move};

use super::Out;
use super::{Pick, Sheet, State, wall_ms};
use crate::toast::Toast;

pub use super::BookAction;

impl State {
    pub(crate) fn book_act(&mut self, action: BookAction, out: &mut Vec<Out>) {
        let now = wall_ms();
        match action {
            BookAction::AskRemove {
                books,
                shelves,
                links,
            } => {
                let asked = Sheet::Removal {
                    books,
                    shelves,
                    links,
                };
                self.sheet = Some(asked);
            }
            BookAction::AskRemovePicked => {
                let books = self.picks.books();
                if books.is_empty() {
                    let note = "Pick the books to take out first.";
                    out.push(Out::Toast(Toast::info(note)));
                    return;
                }
                let (shelves, links) = (Vec::new(), Vec::new());
                let asked = Sheet::Removal {
                    books,
                    shelves,
                    links,
                };
                self.sheet = Some(asked);
            }
            BookAction::Remove {
                books,
                shelves,
                links,
                cascade,
                forget,
            } => {
                self.remove_links(&links, books.is_empty() && shelves.is_empty(), out);
                self.remove_entries((&books, &shelves), cascade, forget, out);
            }
            BookAction::Twin(id) => {
                let Some(twin) = self.model.twin(id, now) else {
                    return;
                };
                let title = |id| self.model.book(id).map(|b| b.title.value.clone());
                let (made, from) = (title(twin), title(id));
                let note = format!(
                    "“{}” reads separately from “{}”.",
                    made.unwrap_or_default(),
                    from.unwrap_or_default()
                );
                out.push(Out::Toast(Toast::info(note)));
            }
            BookAction::Import { paths, to } => self.import_files(paths, to, out),
            BookAction::ShelveTogether(picks) => {
                let picks = self.unheld_picks(picks, out);
                if !picks.is_empty() {
                    self.shelve_together(&picks, out);
                }
            }
            BookAction::Duplicate(picks) => self.duplicate(picks, out),
            // File manager calls go through the host.
            BookAction::Reveal(_) => {}
        }
    }

    /// Links go at once: what they point at stays. `alone` tells of it.
    fn remove_links(&mut self, links: &[LinkId], alone: bool, out: &mut Vec<Out>) {
        let first = links.first().and_then(|l| self.model.link(*l));
        let first = first.map(|l| l.name.value.clone()).unwrap_or_default();
        let gone = self.model.remove_links(links, wall_ms());
        if !alone || gone == 0 {
            return;
        }
        self.sheet = None;
        self.picks.apply(super::PickAction::Clear);
        let note = match gone {
            1 => format!("Removed the link “{first}”."),
            n => format!("Removed {n} links."),
        };
        out.push(Out::Toast(Toast::info(note)));
    }

    /// A new shelf where the reader is, the picks added to it.
    fn shelve_together(&mut self, picks: &[Pick], out: &mut Vec<Out>) {
        let name = super::NEW_SHELF;
        let Ok(id) = self.model.create_shelf(name, self.at, wall_ms()) else {
            return;
        };
        let mv = Move {
            from: self.at,
            to: Some(id),
            keep: true,
            ..Move::of(picks)
        };
        self.carry(mv, out);
    }
}
