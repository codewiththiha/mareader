//! The thumbnail grid: two columns of small pages that follow the reader.

mod picks;

use egui::emath::GuiRounding as _;
use egui::style::ScrollAnimation;
use egui::{
    Align, Align2, CornerRadius, FontId, Id, Rect, ScrollArea, Sense, Stroke, StrokeKind, Ui, Vec2,
    pos2, vec2,
};
use mareader_session::{DocId, PageKey, Part, Scale};
use mareader_workspace::PaneId;

use mareader_appearance::Pattern;

use crate::documents::Reading;
use crate::gpu::{Anchor, PagesCallback};
use crate::motion;
use crate::pages::Lookup;
use crate::pdf_view::{Sinks, Urgency, paint_page};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use mareader_engine::command::Command;
pub use picks::Offer;
use picks::Picks;

/// A cell's page width; two fit the rail with room either side.
pub const CELL_W: f32 = 120.0;
const COLUMNS: usize = 2;
const GAP: f32 = 12.0;
const PAD: f32 = 12.0;
/// The page number band under each page.
const BAND: f32 = 22.0;
const ROW_GAP: f32 = 8.0;
/// Rows drawn ahead each way, so a scroll finds them ready.
const ROW_BUFFER: usize = 2;
/// After the reader scrolls or clicks the grid, following waits this long.
pub const GRACE: f64 = 1.2;

/// What the grid remembers between frames.
#[derive(Debug, Default, Clone)]
pub struct ThumbState {
    /// The pane, document and page last brought into view.
    followed: Option<(PaneId, DocId, usize)>,
    /// When the reader last drove the grid by hand.
    driven_at: Option<f64>,
    /// Frames left to repeat a snap; a new area clamps its first one.
    snaps: u8,
    /// Pages picked to act on together.
    picks: Picks,
    /// What the page AI can do with them.
    offer: Offer,
}

impl ThumbState {
    /// The pages picked, in reading order.
    pub fn picked(&self) -> Vec<usize> {
        self.picks.pages()
    }

    /// Lets go of the picked pages; false if none was picked.
    pub fn clear_picks(&mut self) -> bool {
        self.picks.clear()
    }

    pub fn set_offer(&mut self, offer: Offer) {
        self.offer = offer;
    }

    /// No book to show: picks and the book they held let go.
    pub fn forget_book(&mut self) {
        self.picks = Picks::default();
    }
}

/// Where each page's cell sits, from the first page's shape.
struct Grid {
    left: f32,
    /// The box every page is fitted into.
    cell: Vec2,
    pitch: f32,
    rows: usize,
}

impl Grid {
    fn new(width: f32, first: Option<(f32, f32)>, count: usize) -> Self {
        // Mixed sizes share the first page's box; 3:4 if it has none.
        let aspect = first
            .map_or(4.0 / 3.0, |(w, h)| h / w.max(1.0))
            .clamp(0.3, 3.0);
        let cell = vec2(CELL_W, (CELL_W * aspect).round());
        let span = COLUMNS as f32 * CELL_W + (COLUMNS - 1) as f32 * GAP;
        Self {
            left: ((width - span) / 2.0).max(PAD),
            cell,
            pitch: cell.y + BAND + ROW_GAP,
            rows: count.div_ceil(COLUMNS),
        }
    }

    fn height(&self) -> f32 {
        2.0 * PAD + self.rows as f32 * self.pitch - ROW_GAP
    }

    /// The cell of `page`, page box and band, from the content's top left.
    fn cell(&self, page: usize) -> Rect {
        let (row, col) = (page / COLUMNS, page % COLUMNS);
        let min = pos2(
            self.left + col as f32 * (CELL_W + GAP),
            PAD + row as f32 * self.pitch,
        );
        Rect::from_min_size(min, self.cell + vec2(0.0, BAND))
    }

    /// Rows meeting the band from `top` to `bottom`.
    fn rows(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let first = ((top - PAD) / self.pitch).floor().max(0.0) as usize;
        let last = ((bottom - PAD) / self.pitch).ceil().max(0.0) as usize + 1;
        first.min(self.rows)..last.min(self.rows)
    }
}

/// The page's own shape, fitted and centred in the shared box.
fn fitted(boxed: Rect, size: (f32, f32)) -> Rect {
    let (w, h) = (size.0.max(1.0), size.1.max(1.0));
    let scale = (boxed.width() / w).min(boxed.height() / h);
    Rect::from_center_size(boxed.center(), vec2(w * scale, h * scale))
}

pub fn show(
    ui: &mut Ui,
    reading: &Reading,
    io: Sinks<'_>,
    state: &mut ThumbState,
    recentre: bool,
    out: &mut Commands,
) {
    let Sinks {
        pages,
        demand,
        pass,
        ..
    } = io;
    let ctx = ui.ctx().clone();
    let now = ctx.input(|i| i.time);
    let panel = ui.max_rect();
    let sizes = reading.document.page_sizes();
    let count = sizes.len();
    let size = |page: usize| {
        sizes
            .get(page)
            .map_or((612.0, 792.0), |s| (s.width, s.height))
    };
    let grid = Grid::new(
        panel.width(),
        sizes.first().map(|s| (s.width, s.height)),
        count,
    );
    let ppp = ctx.pixels_per_point();
    let key = |page: usize| {
        let (w, h) = size(page);
        let shown = fitted(Rect::from_min_size(pos2(0.0, 0.0), grid.cell), (w, h));
        PageKey {
            doc: reading.doc,
            page,
            scale: Scale::covering(shown.width() / w * ppp),
            part: Part::Whole,
        }
    };
    if ui.rect_contains_pointer(panel) {
        let (wheel, down) = ctx.input(|i| (i.smooth_scroll_delta, i.pointer.any_down()));
        if wheel != Vec2::ZERO || down {
            state.driven_at = Some(now);
        }
    }
    let goal = follow(state, reading, recentre, now, &ctx);
    let mut t = Tokens::get(&ctx);
    if reading.document.is_reflowable() {
        t.page = t.book;
    }
    // As in mareader: thumbnails take the theme but not the texture.
    t.page.pattern = Pattern::NONE;
    state.picks.follow(reading);
    if state.picks.active() {
        picks::bar(ui, &mut state.picks, count, &t);
    }
    ScrollArea::vertical()
        .id_salt(("thumbs", reading.pane))
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            ui.set_min_size(vec2(panel.width(), grid.height()));
            let origin = ui.max_rect().min.to_vec2();
            if let Some(animation) = goal {
                let target = grid.cell(reading.current).translate(origin);
                ui.scroll_to_rect_animation(target, Some(Align::Center), animation);
            }
            let shown = grid.rows(viewport.top(), viewport.bottom());
            let mut quads = Vec::new();
            let mut picked = Vec::new();
            for page in (shown.start * COLUMNS..shown.end * COLUMNS).take_while(|&p| p < count) {
                let cell = grid.cell(page).translate(origin);
                let boxed = Rect::from_min_size(cell.min, grid.cell);
                let rect = fitted(boxed, size(page)).round_to_pixels(ppp);
                let look = pages.lookup(key(page));
                if !matches!(look, Lookup::Sharp(_) | Lookup::Failed) {
                    demand.push((Urgency::Thumbnail, key(page).into()));
                }
                let anchor = Anchor::page(rect, size(page).0);
                let square = (ui.clip_rect(), 0.0);
                paint_page(
                    ui,
                    ((rect, ui.clip_rect()), anchor, square),
                    look,
                    (&t, true),
                    &mut quads,
                );
                let on = page == reading.current;
                let r = ui.interact(cell, Id::new(("thumb", reading.pane, page)), Sense::click());
                let label = format!("Page {}", page + 1);
                r.widget_info(|| {
                    egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label)
                });
                let held = picks::long_press(ui, &r, page, &mut state.picks);
                if r.clicked() && !held && !picks::click(ui, page, &mut state.picks) {
                    out.push(Sent::Engine(Command::GoTo {
                        pane: reading.pane,
                        page,
                    }));
                }
                let pane = reading.pane;
                let picked_here = state.picks.contains(page);
                let menu_w = if picked_here {
                    crate::widgets::GROUPED_MENU_W
                } else {
                    crate::widgets::MENU_W
                };
                crate::widgets::context_menu_sized(&r, menu_w, |ui| match picked_here {
                    true => picks::picks_menu(
                        ui,
                        (pane, &state.offer),
                        &mut state.picks,
                        count,
                        &t,
                        out,
                    ),
                    false => picks::page_menu(ui, pane, page, &mut state.picks, out),
                });
                let chosen = state.picks.contains(page);
                if chosen {
                    picked.push(rect);
                }
                decorate(ui, (rect, cell), page, (on, chosen), r.hovered(), &t);
            }
            if let (Some(pass), false) = (pass, quads.is_empty()) {
                ui.painter()
                    .add(PagesCallback::shape(pass.clone(), ui.clip_rect(), quads));
            }
            // Over the pages, which draw last: a wash and a tick.
            for rect in picked {
                tick(ui, rect, &t);
            }
            // Rows just off screen render too, after what's on screen.
            let ahead = shown.end..(shown.end + ROW_BUFFER).min(grid.rows);
            let behind = shown.start.saturating_sub(ROW_BUFFER)..shown.start;
            for row in behind.chain(ahead) {
                for page in (row * COLUMNS..(row + 1) * COLUMNS).take_while(|&p| p < count) {
                    if !pages.touch(key(page)) {
                        demand.push((Urgency::Prefetch, key(page).into()));
                    }
                }
            }
        });
    state.picks.settle(reading);
}

/// Snaps to the page on opening; glides as it moves, unless browsed.
fn follow(
    state: &mut ThumbState,
    reading: &Reading,
    recentre: bool,
    now: f64,
    ctx: &egui::Context,
) -> Option<ScrollAnimation> {
    let here = (reading.pane, reading.doc, reading.current);
    let moved_away = state
        .followed
        .is_none_or(|(pane, doc, _)| (pane, doc) != (here.0, here.1));
    if recentre || moved_away {
        state.snaps = 2;
    }
    if state.snaps > 0 {
        state.snaps -= 1;
        state.followed = Some(here);
        state.driven_at = None;
        ctx.request_repaint();
        return Some(ScrollAnimation::none());
    }
    if state.followed == Some(here) {
        return None;
    }
    // A reader browsing the grid keeps it where they left it.
    if let Some(at) = state.driven_at {
        let left = GRACE - (now - at);
        if left > 0.0 {
            ctx.request_repaint_after_secs(left as f32);
            return None;
        }
    }
    state.followed = Some(here);
    Some(motion::jump(ctx))
}

/// The number band, and the accent ring of the reading or a picked page.
fn decorate(
    ui: &Ui,
    (page, cell): (Rect, Rect),
    index: usize,
    (on, picked): (bool, bool),
    hovered: bool,
    t: &Tokens,
) {
    let painter = ui.painter();
    if on || picked {
        let ring = Stroke::new(2.0, t.accent);
        painter.rect_stroke(
            page.expand(3.0),
            CornerRadius::same(4),
            ring,
            StrokeKind::Middle,
        );
    } else if hovered {
        let ring = Stroke::new(1.0, t.muted.gamma_multiply(0.6));
        painter.rect_stroke(
            page.expand(2.0),
            CornerRadius::same(3),
            ring,
            StrokeKind::Middle,
        );
    }
    let centre = pos2(cell.center().x, cell.bottom() - BAND / 2.0 + 2.0);
    let font = FontId::proportional(t.text_small);
    let number = (index + 1).to_string();
    // The ring marks the page; its number just takes the accent.
    if on {
        let font = crate::theme::medium(t.text_small);
        painter.text(centre, Align2::CENTER_CENTER, number, font, t.accent);
    } else {
        painter.text(centre, Align2::CENTER_CENTER, number, font, t.muted);
    }
}

/// A picked page's wash and the tick in its corner.
fn tick(ui: &Ui, page: Rect, t: &Tokens) {
    let painter = ui.painter();
    painter.rect_filled(page, CornerRadius::ZERO, t.accent.gamma_multiply(0.12));
    let centre = pos2(page.right() - 10.0, page.top() + 10.0);
    painter.circle_filled(centre, 8.0, t.accent);
    let mark = [
        centre + vec2(-3.5, 0.2),
        centre + vec2(-1.0, 2.8),
        centre + vec2(3.8, -2.6),
    ];
    let ink = Stroke::new(1.6, egui::Color32::WHITE);
    painter.line(mark.to_vec(), ink);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_columns_fit_the_rail_and_rows_follow_the_first_page() {
        let span = COLUMNS as f32 * CELL_W + GAP;
        assert!(span + 2.0 * PAD <= crate::sidebar::WIDTH);
        let grid = Grid::new(crate::sidebar::WIDTH, Some((612.0, 792.0)), 5);
        assert_eq!(grid.rows, 3);
        assert_eq!(grid.cell.y, (CELL_W * 792.0 / 612.0).round());
        // Page 3 starts the second row, in the first column.
        let (p0, p2, p3) = (grid.cell(0), grid.cell(2), grid.cell(3));
        assert_eq!(p2.left(), p0.left());
        assert_eq!(p2.top() - p0.top(), grid.pitch);
        assert_eq!(p3.left() - p0.left(), CELL_W + GAP);
    }

    #[test]
    fn a_window_of_rows_covers_the_band_and_stops_at_the_end() {
        let grid = Grid::new(288.0, Some((100.0, 100.0)), 10);
        assert_eq!(grid.rows(0.0, grid.pitch * 0.5), 0..2);
        assert_eq!(
            grid.rows(grid.pitch * 2.0 + PAD, grid.pitch * 2.5 + PAD),
            2..4
        );
        assert_eq!(grid.rows(0.0, 1e6), 0..5);
        assert_eq!(grid.rows(1e6, 2e6), 5..5);
    }

    #[test]
    fn a_landscape_page_fits_inside_a_portrait_box() {
        let boxed = Rect::from_min_size(pos2(0.0, 0.0), vec2(120.0, 155.0));
        let r = fitted(boxed, (792.0, 612.0));
        assert!((r.width() - 120.0).abs() < 0.01);
        assert!(r.height() < 155.0);
        assert_eq!(r.center(), boxed.center());
    }
}
