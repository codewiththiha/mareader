//! Space: the hub of study tools beside the library, and each tool's page.
//!
//! The engine reads the words and the marks into rows
//! (`mareader_engine::space`) and keeps which room shows; this module keeps
//! each page's filter and draws the hub's cards, or a tool's rail down the
//! left side, its header and its rows. What a row asks goes back as a
//! command.

mod detail;
mod hub;
mod locateglow;
mod look;
mod notes;
mod rail;
mod returning;
mod rows;
mod selection;
mod sentence;
mod top;

use egui::{Align, Layout, Rect, Ui, UiBuilder, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::space::{self, Filter, NotesFilter, Room, View};
use mareader_library::{BookId, Library};
use mareader_settings::HighlightSettings;

use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};

pub use hub::TITLE as HUB_TITLE;
pub use notes::TITLE as NOTES_TITLE;
pub use rows::{open_book, open_sentence};

/// The words page's own label, which tests and the titlebar find it by.
pub const TITLE: &str = "Vocabs";
/// The Export CSV button's label.
pub const EXPORT: &str = "Export CSV";
/// The rail runs down the page's left side from this window width on.
const RAIL_FROM: f32 = 900.0;
const RAIL_W: f32 = 264.0;
const MARGIN: f32 = 32.0;
const MAX_W: f32 = 1120.0;
/// The hub's cards keep to this width.
const HUB_W: f32 = 980.0;

/// Which words and notes show, and how; a window keeps it while the page
/// is away.
#[derive(Default)]
pub struct SpaceUi {
    pub filter: Filter,
    pub notes: NotesFilter,
    /// Cards in a grid instead of the list.
    pub cards: bool,
    card_columns: Option<u8>,
    /// The row opened to its sentence, by its key.
    open: Option<u128>,
    detail: Option<detail::Detail>,
    lookup: Option<String>,
    selection: selection::Selection,
    selection_filter: Option<(Filter, bool)>,
    return_to: Option<returning::Return>,
    reveal: Option<(u128, bool, f64)>,
    delete_ai: Option<Vec<(String, Option<String>)>>,
    delete_ticket: Option<mareader_dict::Ticket>,
    delete_error: Option<String>,
}

/// What the page asks of the window this frame.
pub enum Ask {
    /// Save these rows as CSV.
    Export(String),
}

/// What a page draws from: the library, the room, the reader's colours.
pub struct Source<'a> {
    pub lib: &'a Library,
    pub room: Room,
    pub look: &'a HighlightSettings,
}

impl SpaceUi {
    /// Draws `from`'s room below `top`, filling `ui`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        from: Source<'_>,
        top: f32,
        out: &mut Commands,
    ) -> Option<Ask> {
        let t = Tokens::get(ui.ctx());
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t.paper);
        let page = Rect::from_min_max(pos2(full.left(), top), full.max);
        let asked = match from.room {
            Room::Hub => {
                let col = column(page, page, HUB_W);
                let scroll = egui::ScrollArea::vertical().id_salt("space-hub");
                ui.scope_builder(down(col.translate(vec2(0.0, 12.0))), |ui| {
                    scroll.show(ui, |ui| hub::show(ui, from.lib, &t, out));
                });
                None
            }
            Room::Words => self.words(ui, from.lib, page, &t, out),
            Room::Notes => {
                self.notes(ui, &from, page, &t, out);
                None
            }
        };
        if from.room != Room::Words {
            self.detail = None;
            self.lookup = None;
        }
        if let Some(detail) = &mut self.detail
            && !detail.show(ui.ctx(), &t)
        {
            self.detail = None;
        }
        if let Some(words) = self.selection.dialog(ui.ctx(), from.lib, &t, out) {
            self.delete_ai = Some(words);
        }
        asked
    }

    pub fn take_delete_ai(&mut self) -> Option<Vec<(String, Option<String>)>> {
        self.delete_ai.take()
    }

    pub fn set_delete_ticket(&mut self, ticket: mareader_dict::Ticket) {
        self.delete_ticket = Some(ticket);
    }

    pub fn take_delete_error(&mut self) -> Option<String> {
        self.delete_error.take()
    }

    pub fn remember_return(&mut self, book: BookId, row: u128, now: f64) {
        self.return_to = Some(returning::Return::new(
            book,
            row,
            self.filter.clone(),
            self.cards,
            now,
        ));
    }

    pub fn show_return(
        &mut self,
        ctx: &egui::Context,
        book: Option<BookId>,
        rect: Rect,
        bar: bool,
    ) -> bool {
        let Some(back) = &mut self.return_to else {
            return false;
        };
        if Some(back.book) != book || !returning::show(ctx, back, rect, bar) {
            return false;
        }
        if let Some(back) = self.return_to.take() {
            self.filter = back.filter;
            self.cards = back.cards;
            self.reveal = Some((back.row, true, ctx.input(|i| i.time) + 5.0));
            self.selection = selection::Selection::default();
            return true;
        }
        false
    }

    pub fn escape(&mut self, ctx: &egui::Context) -> bool {
        if let Some(detail) = &mut self.detail {
            detail.close(ctx);
            true
        } else {
            false
        }
    }

    pub fn take_lookup(&mut self) -> Option<String> {
        self.lookup.take()
    }

    pub fn set_lookup_ticket(&mut self, ticket: mareader_dict::Ticket) {
        if let Some(detail) = &mut self.detail {
            detail.ticket(ticket);
        }
    }

    pub fn receive(&mut self, replies: &[mareader_dict::Reply]) -> bool {
        let mut changed = false;
        if let Some(reply) = replies
            .iter()
            .find(|r| Some(r.ticket) == self.delete_ticket)
        {
            self.delete_ticket = None;
            self.delete_error = reply.failed.clone();
            changed = reply.failed.is_none();
        }
        if let Some(detail) = &mut self.detail {
            for reply in replies {
                detail.receive(reply);
            }
        }
        changed
    }

    /// Shows only `book` in `room`, every chip and search let go.
    pub fn focus(&mut self, room: Room, book: BookId) {
        let book = Some(book);
        if room == Room::Notes {
            self.notes = NotesFilter {
                book,
                ..NotesFilter::default()
            };
            return;
        }
        // How the rows are laid out stays; what they're narrowed to goes.
        let old = std::mem::take(&mut self.filter);
        self.filter = Filter {
            book,
            group: old.group,
            sort: old.sort,
            scope: old.scope,
            ..Filter::default()
        };
    }

    /// The words page: its rail, then its column.
    fn words(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        page: Rect,
        t: &Tokens,
        out: &mut Commands,
    ) -> Option<Ask> {
        let key = (self.filter.clone(), self.cards);
        if self
            .selection_filter
            .as_ref()
            .is_some_and(|old| old != &key)
        {
            self.selection = selection::Selection::default();
        }
        self.selection_filter = Some(key);
        if self
            .reveal
            .as_ref()
            .is_some_and(|r| ui.ctx().input(|i| i.time) > r.2)
        {
            self.reveal = None;
        }
        let view = space::view(lib, &self.filter);
        let filter = &mut self.filter;
        let main = rail(ui, page, t, |ui| rail::show(ui, &view, filter, t));
        let col = column(page, main, MAX_W);
        ui.scope_builder(down(col), |ui| self.word_column(ui, lib, &view, t, out))
            .inner
    }

    /// The notes page: the books down the rail, then its column.
    fn notes(
        &mut self,
        ui: &mut Ui,
        from: &Source<'_>,
        page: Rect,
        t: &Tokens,
        out: &mut Commands,
    ) {
        let view = space::notes(from.lib, &self.notes);
        let book = &mut self.notes.book;
        let main = rail(ui, page, t, |ui| {
            ui.spacing_mut().item_spacing.y = 10.0;
            look::caption(ui, "Books", t);
            rail::books(ui, (&view.books, view.total), book, t);
        });
        let narrow = main == page;
        let col = column(page, main, MAX_W);
        let filter = &mut self.notes;
        ui.scope_builder(down(col), |ui| {
            let scroll = egui::ScrollArea::vertical()
                .id_salt("space-notes")
                .auto_shrink([false, false]);
            let scrolled = scroll.show(ui, |ui| {
                notes::column(ui, (&view, filter), (from.look, narrow), t, out);
                ui.add_space(MARGIN);
            });
            crate::widgets::scroll_fades(ui, &scrolled, t.paper);
        });
    }

    /// The header, the tabs and stats, the toolbar, then the rows, all
    /// scrolling together.
    fn word_column(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        view: &View,
        t: &Tokens,
        out: &mut Commands,
    ) -> Option<Ask> {
        let scroll = egui::ScrollArea::vertical()
            .id_salt("space-page")
            .auto_shrink([false, false]);
        let mut picked = None;
        let scrolled = scroll.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            if look::back(ui, t) {
                out.push(Sent::Engine(Command::ShowSpace(Room::Hub)));
            }
            let export = top::header(ui, &mut self.filter, t);
            top::tabs(ui, view, &mut self.filter, t);
            ui.add_space(6.0);
            top::stats(ui, view, t);
            ui.add_space(4.0);
            top::toolbar(ui, &mut self.filter, &mut self.cards, t);
            if self.cards {
                top::card_size(ui, &mut self.card_columns, t);
            }
            ui.add_space(2.0);
            self.selection.toolbar(ui, self.filter.scope, t);
            let shown = rows::Shown {
                cards: self.cards,
                columns: self.card_columns,
                open: &mut self.open,
                picked: &mut picked,
                selection: &mut self.selection,
                scope: self.filter.scope,
                reveal: &mut self.reveal,
            };
            rows::show(ui, view, shown, t, out);
            ui.add_space(MARGIN);
            export
        });
        crate::widgets::scroll_fades(ui, &scrolled, t.paper);
        if let Some((row, rect)) = picked {
            let now = ui.ctx().input(|i| i.time);
            let row = space::detail_row(lib, row);
            let history = space::history(lib, &row);
            let detail = detail::Detail::new(row, history, rect, now);
            self.lookup = Some(detail.word().to_owned());
            self.detail = Some(detail);
            ui.ctx().request_repaint();
        }
        scrolled.inner.then(|| Ask::Export(space::csv(view)))
    }
}

/// The rail down `page`'s left, when wide enough, drawn by `draw`; what is
/// left of the page.
fn rail(ui: &mut Ui, page: Rect, t: &Tokens, draw: impl FnOnce(&mut Ui)) -> Rect {
    if ui.max_rect().width() < RAIL_FROM {
        return page;
    }
    let rail = Rect::from_min_max(page.min, pos2(page.left() + RAIL_W, page.bottom()));
    ui.painter().rect_filled(rail, 0.0, t.card);
    let inner = rail.shrink2(vec2(20.0, 24.0));
    let scroll = egui::ScrollArea::vertical().id_salt("space-rail");
    ui.scope_builder(down(inner), |ui| {
        scroll.show(ui, draw);
    });
    page.with_min_x(rail.right())
}

/// A column at most `max` wide, centred in `main`, from below `page`'s top.
fn column(page: Rect, main: Rect, max: f32) -> Rect {
    let width = (main.width() - 2.0 * MARGIN).clamp(0.0, max);
    let left = main.center().x - width / 2.0;
    let top = page.top();
    let bottom = page.bottom().max(top + 12.0);
    Rect::from_min_max(pos2(left, top + 28.0), pos2(left + width, bottom))
}

/// A top-down area filling `rect`.
fn down(rect: Rect) -> UiBuilder {
    UiBuilder::new()
        .max_rect(rect)
        .layout(Layout::top_down(Align::Min))
}
