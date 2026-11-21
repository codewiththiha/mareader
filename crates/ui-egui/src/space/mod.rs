//! Space: every studied word, across books, as one page beside the library.
//!
//! The engine reads the words into rows (`mareader_engine::space`); this
//! module keeps the page's filter and draws the rail, the header, the tabs
//! and the rows. What a row asks of its word goes back as a command.

mod look;
mod rail;
mod rows;
mod top;

use egui::{Align, Layout, Rect, Ui, UiBuilder, pos2};
use mareader_engine::space::{self, Filter, View};
use mareader_library::Library;

use crate::theme::Tokens;
use crate::ui_command::Commands;

/// The page's own label, which tests and the titlebar find it by.
pub const TITLE: &str = "Your words";
/// The Export CSV button's label.
pub const EXPORT: &str = "Export CSV";
/// The rail shows beside the rows from this width on.
const RAIL_FROM: f32 = 760.0;
const RAIL_W: f32 = 210.0;
const MARGIN: f32 = 24.0;
const MAX_W: f32 = 1040.0;

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
        let width = (full.width() - 2.0 * MARGIN).clamp(0.0, MAX_W);
        let left = full.center().x - width / 2.0;
        let bottom = (full.bottom() - MARGIN).max(top + 12.0);
        let mut col = Rect::from_min_max(pos2(left, top + 16.0), pos2(left + width, bottom));
        if width >= RAIL_FROM {
            let rail = Rect::from_min_max(col.min, pos2(col.left() + RAIL_W, col.bottom()));
            col.min.x = rail.right() + MARGIN;
            let filter = &mut self.filter;
            ui.scope_builder(down(rail), |ui| rail::show(ui, &view, filter, &t));
        }
        ui.scope_builder(down(col), |ui| self.column(ui, &view, &t, out))
            .inner
    }

    /// The header, the tabs and stats, the toolbar, then the rows.
    fn column(&mut self, ui: &mut Ui, view: &View, t: &Tokens, out: &mut Commands) -> Option<Ask> {
        ui.spacing_mut().item_spacing.y = 10.0;
        let export = top::header(ui, &mut self.filter, t);
        top::tabs(ui, view, &mut self.filter, t);
        top::stats(ui, view, t);
        top::toolbar(ui, &mut self.filter, &mut self.cards);
        let shown = rows::Shown {
            cards: self.cards,
            open: &mut self.open,
        };
        rows::show(ui, view, shown, t, out);
        export.then(|| Ask::Export(space::csv(view)))
    }
}

/// A top-down area filling `rect`.
fn down(rect: Rect) -> UiBuilder {
    UiBuilder::new()
        .max_rect(rect)
        .layout(Layout::top_down(Align::Min))
}
