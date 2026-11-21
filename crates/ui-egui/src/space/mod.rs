//! Space: every studied word, across books, as one page beside the library.
//!
//! The engine reads the words into rows (`mareader_engine::space`); this
//! module keeps the page's filter and draws the rail down the left side,
//! then the header, the tabs, the stat cards, the toolbar and the rows.
//! What a row asks of its word goes back as a command.

mod look;
mod rail;
mod rows;
mod top;

use egui::{Align, Layout, Rect, Ui, UiBuilder, pos2, vec2};
use mareader_engine::space::{self, Filter, View};
use mareader_library::Library;

use crate::theme::Tokens;
use crate::ui_command::Commands;

/// The page's own label, which tests and the titlebar find it by.
pub const TITLE: &str = "Your words";
/// The Export CSV button's label.
pub const EXPORT: &str = "Export CSV";
/// The rail runs down the page's left side from this window width on.
const RAIL_FROM: f32 = 900.0;
const RAIL_W: f32 = 264.0;
const MARGIN: f32 = 32.0;
const MAX_W: f32 = 1120.0;

/// Which words show, and how; a window keeps it while the page is away.
#[derive(Default)]
pub struct SpaceUi {
    pub filter: Filter,
    /// Cards in a grid instead of the list.
    pub cards: bool,
    /// The row opened to its sentence, by its key.
    open: Option<u128>,
}

/// What the page asks of the window this frame.
pub enum Ask {
    /// Save these rows as CSV.
    Export(String),
}

impl SpaceUi {
    /// Draws the page below `top`, filling `ui`, from `lib`'s words.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        lib: &Library,
        top: f32,
        out: &mut Commands,
    ) -> Option<Ask> {
        let t = Tokens::get(ui.ctx());
        let full = ui.max_rect();
        ui.painter().rect_filled(full, 0.0, t.paper);
        let view = space::view(lib, &self.filter);
        let page = Rect::from_min_max(pos2(full.left(), top), full.max);
        let mut main = page;
        if full.width() >= RAIL_FROM {
            let rail = Rect::from_min_max(page.min, pos2(page.left() + RAIL_W, page.bottom()));
            ui.painter().rect_filled(rail, 0.0, t.card);
            main.min.x = rail.right();
            let filter = &mut self.filter;
            let inner = rail.shrink2(vec2(20.0, 24.0));
            let scroll = egui::ScrollArea::vertical().id_salt("space-rail");
            ui.scope_builder(down(inner), |ui| {
                scroll.show(ui, |ui| rail::show(ui, &view, filter, &t));
            });
        }
        let width = (main.width() - 2.0 * MARGIN).clamp(0.0, MAX_W);
        let left = main.center().x - width / 2.0;
        let bottom = full.bottom().max(top + 12.0);
        let col = Rect::from_min_max(pos2(left, top + 28.0), pos2(left + width, bottom));
        ui.scope_builder(down(col), |ui| self.column(ui, &view, &t, out))
            .inner
    }

    /// The header, the tabs and stats, the toolbar, then the rows, all
    /// scrolling together.
    fn column(&mut self, ui: &mut Ui, view: &View, t: &Tokens, out: &mut Commands) -> Option<Ask> {
        let scroll = egui::ScrollArea::vertical()
            .id_salt("space-page")
            .auto_shrink([false, false]);
        let scrolled = scroll.show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 14.0;
            let export = top::header(ui, &mut self.filter, t);
            top::tabs(ui, view, &mut self.filter, t);
            ui.add_space(6.0);
            top::stats(ui, view, t);
            ui.add_space(4.0);
            top::toolbar(ui, &mut self.filter, &mut self.cards, t);
            ui.add_space(2.0);
            let shown = rows::Shown {
                cards: self.cards,
                open: &mut self.open,
            };
            rows::show(ui, view, shown, t, out);
            ui.add_space(MARGIN);
            export
        });
        crate::widgets::scroll_fades(ui, &scrolled, t.paper);
        scrolled.inner.then(|| Ask::Export(space::csv(view)))
    }
}

/// A top-down area filling `rect`.
fn down(rect: Rect) -> UiBuilder {
    UiBuilder::new()
        .max_rect(rect)
        .layout(Layout::top_down(Align::Min))
}
