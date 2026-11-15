//! The outline panel: the document's chapters, the current one marked.

use egui::style::ScrollAnimation;
use egui::{
    Align, Align2, CornerRadius, FontId, FontSelection, Id, Rect, RichText, ScrollArea, Sense,
    TextWrapMode, Ui, WidgetText, pos2, vec2,
};
use mareader_pdf::{MAX_DEPTH, OutlineEntry, active_entry};
use mareader_session::DocId;
use mareader_workspace::PaneId;

use crate::documents::Reading;
use crate::motion;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;

const ROW_H: f32 = 28.0;
/// Indent per level; levels past `MAX_DEPTH` share the last one.
const INDENT: f32 = 12.0;
const PAD_X: f32 = 12.0;
/// Room kept on the right for the page number.
const NUMBER_W: f32 = 36.0;

/// What the panel remembers between frames.
#[derive(Debug, Default, Clone)]
pub struct OutlineState {
    /// The pane, document and entry last brought into view.
    followed: Option<(PaneId, DocId, Option<usize>)>,
    /// The entry clicked, which stays marked while its page is read.
    clicked: Option<(PaneId, DocId, usize)>,
    /// Frames left to repeat a centring; a new area clamps its first one.
    snaps: u8,
}

impl OutlineState {
    /// The clicked entry while its page is read, else the page's.
    fn active(&self, reading: &Reading, entries: &[OutlineEntry]) -> Option<usize> {
        let clicked = self.clicked.and_then(|(pane, doc, i)| {
            let still = (pane, doc) == (reading.pane, reading.doc);
            let entry = entries.get(i)?;
            (still && entry.page == reading.current).then_some(i)
        });
        clicked.or_else(|| active_entry(entries, reading.current))
    }
}

/// Draws the panel for `reading`'s document.
pub fn show(
    ui: &mut Ui,
    reading: &Reading,
    state: &mut OutlineState,
    recentre: bool,
    out: &mut Commands,
) {
    let entries = reading.document.outline();
    let t = Tokens::get(ui.ctx());
    if entries.is_empty() {
        let hint = "This document has no table of contents.";
        widgets::panel_empty(ui, None, "No outline", hint);
        return;
    }
    let active = state.active(reading, entries);
    let goal = follow(state, reading, active, recentre, ui.ctx());
    let panel = ui.max_rect();
    ScrollArea::vertical()
        .id_salt(("outline", reading.pane))
        .auto_shrink(false)
        .show_viewport(ui, |ui, viewport| {
            let height = ROW_H * entries.len() as f32 + PAD_X;
            ui.set_min_size(vec2(panel.width(), height));
            let origin = ui.max_rect().min;
            let row = |i: usize| {
                Rect::from_min_size(
                    origin + vec2(0.0, PAD_X / 2.0 + i as f32 * ROW_H),
                    vec2(panel.width(), ROW_H),
                )
            };
            if let (Some((align, animation)), Some(i)) = (goal, active) {
                ui.scroll_to_rect_animation(row(i), align, animation);
            }
            let first = ((viewport.top() / ROW_H).floor().max(0.0)) as usize;
            let last = ((viewport.bottom() / ROW_H).ceil() as usize + 1).min(entries.len());
            for (i, entry) in entries.iter().enumerate().take(last).skip(first) {
                let on = active == Some(i);
                if entry_row(ui, row(i), entry, on, (reading.pane, i), &t) {
                    state.clicked = Some((reading.pane, reading.doc, i));
                    out.push(Sent::Engine(Command::GoTo {
                        pane: reading.pane,
                        page: entry.page,
                    }));
                }
            }
        });
}

/// One chapter row; true when clicked.
fn entry_row(
    ui: &Ui,
    rect: Rect,
    entry: &OutlineEntry,
    on: bool,
    key: (PaneId, usize),
    t: &Tokens,
) -> bool {
    let id = Id::new(("outline-row", key));
    let r = ui.interact(rect, id, Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &entry.title));
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect, CornerRadius::ZERO, t.hover);
        let bar = Rect::from_min_size(rect.min, vec2(2.0, rect.height()));
        painter.rect_filled(bar, CornerRadius::ZERO, t.accent);
    } else if r.hovered() {
        painter.rect_filled(rect, CornerRadius::ZERO, t.hover.gamma_multiply(0.6));
    }
    let indent = PAD_X + INDENT * f32::from(entry.depth.min(MAX_DEPTH));
    let room = (rect.width() - indent - NUMBER_W).max(24.0);
    // Chapters read darker and heavier than the sections under them.
    let chapter = entry.depth == 0;
    let font = if on || chapter {
        theme::medium(13.0)
    } else {
        FontId::proportional(13.0)
    };
    let colour = if on || chapter || r.hovered() {
        t.ink
    } else {
        t.muted
    };
    let elided = match crate::shaped::block(ui.ctx(), &entry.title, 13.0, room, 1) {
        Some(drawn) => {
            let at = pos2(rect.left() + indent, rect.center().y - drawn.size.y / 2.0);
            drawn.paint(painter, at, colour);
            drawn.cut
        }
        None => {
            let text = WidgetText::from(RichText::new(&entry.title).font(font).color(colour));
            let galley = text.into_galley(
                ui,
                Some(TextWrapMode::Truncate),
                room,
                FontSelection::Default,
            );
            let at = pos2(
                rect.left() + indent,
                rect.center().y - galley.size().y / 2.0,
            );
            let elided = galley.elided;
            painter.galley(at, galley, colour);
            elided
        }
    };
    let number = (entry.page + 1).to_string();
    let right = pos2(rect.right() - PAD_X, rect.center().y);
    let small = FontId::proportional(t.text_small);
    painter.text(right, Align2::RIGHT_CENTER, number, small, t.muted);
    let clicked = r.clicked();
    // A cut-off title shows whole on hover.
    if elided {
        r.on_hover_text(&entry.title);
    }
    clicked
}

/// Centres the entry on opening; else reveals it once off screen.
fn follow(
    state: &mut OutlineState,
    reading: &Reading,
    active: Option<usize>,
    recentre: bool,
    ctx: &egui::Context,
) -> Option<(Option<Align>, ScrollAnimation)> {
    let here = (reading.pane, reading.doc, active);
    let moved_away = state
        .followed
        .is_none_or(|(pane, doc, _)| (pane, doc) != (here.0, here.1));
    if recentre || moved_away {
        state.snaps = 2;
    }
    if state.snaps > 0 {
        state.snaps -= 1;
        state.followed = Some(here);
        ctx.request_repaint();
        return Some((Some(Align::Center), ScrollAnimation::none()));
    }
    if state.followed == Some(here) {
        return None;
    }
    state.followed = Some(here);
    Some((None, motion::jump(ctx)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(current: usize) -> Result<Reading, String> {
        crate::testing::reading(12, current)
    }

    #[test]
    fn a_click_keeps_its_entry_marked_until_the_page_changes() -> Result<(), String> {
        let mut r = reading(1)?;
        let mut entries = r.document.outline().to_vec();
        let mut state = OutlineState::default();
        // Page 2 belongs to "1.1 Scrolling", the second entry.
        assert_eq!(state.active(&r, &entries), Some(1));
        // A chapter clicked on a page it shares wins while it's read.
        if let Some(e) = entries.get_mut(0) {
            e.page = 1;
        }
        state.clicked = Some((r.pane, r.doc, 0));
        assert_eq!(state.active(&r, &entries), Some(0));
        r.current = 5;
        assert_eq!(state.active(&r, &entries), Some(3));
        Ok(())
    }
}
