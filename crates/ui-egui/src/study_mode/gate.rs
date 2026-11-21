//! The veil over the pages past a pane's gate, and its way to the deck.
//!
//! The view stops at the end of the gate page (`pdf_view`), so locked pages
//! peek in under the veil. Once they fill enough of the pane the veil covers
//! all of it and holds the deck. With the lock on, the view also stops at
//! the start page, and a frost covers the pages before it.

use egui::{
    Align, Context, CornerRadius, Id, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2,
};
use mareader_engine::command::Command;
use mareader_engine::pane::ViewMode;
use mareader_engine::study_mode::Action;
use mareader_workspace::PaneId;

use crate::icons::Icon;
use crate::selection::Sheet;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use crate::z_order::{self, Tier};

/// The share of a strip the locked pages fill once the gate is reached.
const REACHED: f32 = 0.3;
/// The button that brings back cards left for the pages before.
pub const OPEN_CARDS: &str = "Open the cards";
/// What the frost over the pages before the start says.
pub const BEFORE: &str = "Locked while you study";
/// A veil this short shows its button alone.
const ROOMY: f32 = 120.0;
/// How much of the paper the veil lets through: none of the words.
const FROST: f32 = 0.97;

/// How far study mode lets a pane go: from `floor` up to `gate`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StudySpan {
    pub floor: usize,
    pub gate: Option<usize>,
}

impl StudySpan {
    /// `page`, kept within the hold; the gate wins if the two cross.
    pub fn clamp(self, page: usize) -> usize {
        let page = page.max(self.floor);
        self.gate.map_or(page, |gate| page.min(gate))
    }
}

/// Where locked pages show in a pane, and whether the gate is reached.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Veil {
    pub rect: Rect,
    pub reached: bool,
}

/// The veil over `sheets` from `gate` on; none while no locked page shows.
///
/// A paged mode reaches the gate as soon as a locked page shows.
pub fn veil(sheets: &[Sheet], pane: Rect, gate: usize, mode: ViewMode) -> Option<Veil> {
    let locked = sheets.iter().filter(|s| s.page >= gate);
    let first = locked.clone().min_by_key(|s| s.page)?;
    let rect = match mode {
        ViewMode::Vertical => Rect::from_min_max(pos2(pane.left(), first.rect.top()), pane.max),
        ViewMode::Horizontal => Rect::from_min_max(pos2(first.rect.left(), pane.top()), pane.max),
        ViewMode::Single | ViewMode::Spread => locked.fold(Rect::NOTHING, |r, s| r.union(s.shown)),
    };
    let rect = rect.intersect(pane);
    if !rect.is_positive() {
        return None;
    }
    let reached = match mode {
        ViewMode::Vertical => rect.height() >= pane.height() * REACHED,
        ViewMode::Horizontal => rect.width() >= pane.width() * REACHED,
        ViewMode::Single | ViewMode::Spread => true,
    };
    Some(Veil { rect, reached })
}

/// The pages before `floor` as they show in a pane, if any do.
pub fn frost(sheets: &[Sheet], pane: Rect, floor: usize, mode: ViewMode) -> Option<Rect> {
    let before = sheets.iter().filter(|s| s.page < floor);
    let end = before.clone().max_by_key(|s| s.page)?.rect.max;
    let rect = match mode {
        ViewMode::Vertical => Rect::from_min_max(pane.min, pos2(pane.right(), end.y)),
        ViewMode::Horizontal => Rect::from_min_max(pane.min, pos2(end.x, pane.bottom())),
        ViewMode::Single | ViewMode::Spread => before.fold(Rect::NOTHING, |r, s| r.union(s.shown)),
    };
    let rect = rect.intersect(pane);
    rect.is_positive().then_some(rect)
}

/// The frost over the locked pages before the start, which says why.
pub fn before(ctx: &Context, pane: PaneId, rect: Rect) {
    let id = Id::new(("study-before", pane));
    frosted(ctx, id, (rect, rect.bottom() - 0.5), |ui| {
        let t = Tokens::get(ui.ctx());
        ui.add_space((ui.max_rect().height() * 0.5 - 10.0).max(0.0));
        ui.label(RichText::new(BEFORE).size(t.text_small).color(t.muted));
    });
}

/// Frosted paper over `rect`, taking every press on it, with `content` on top.
pub fn cover(ctx: &Context, pane: PaneId, rect: Rect, content: impl FnOnce(&mut Ui)) {
    let id = Id::new(("study-veil", pane));
    frosted(ctx, id, (rect, rect.top() + 0.5), content);
}

/// Frost over `rect` with a rule along `edge`, and `content` on top.
fn frosted(ctx: &Context, id: Id, (rect, edge): (Rect, f32), content: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ctx);
    z_order::keep_under(ctx, Tier::PageCard, id);
    z_order::area(ctx, Tier::PageCard, id)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, t.paper.gamma_multiply(FROST));
            ui.painter()
                .hline(rect.x_range(), edge, Stroke::new(1.0, t.line));
            let inner = UiBuilder::new()
                .max_rect(rect.shrink(16.0))
                .layout(Layout::top_down(Align::Center));
            ui.scope_builder(inner, content);
        });
}

/// What unlocks the veiled pages, and the way to their words; `away` once
/// the reader left the cards for the pages before.
pub fn prompt(ui: &mut Ui, pane: PaneId, (range, away): (&str, bool), out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    let (title, why, label) = match away {
        true => (
            format!("Your cards for {range} are waiting"),
            "Their time stopped while you read back.",
            OPEN_CARDS,
        ),
        false => (
            format!("{} unlock after their words", capital(range)),
            "Scrolling stops here until you've studied them.",
            "Study the words",
        ),
    };
    if ui.max_rect().height() >= ROOMY {
        let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), Sense::hover());
        Icon::Clock.paint(ui, mark, t.muted);
        ui.label(RichText::new(title).font(crate::theme::medium(15.0)));
        ui.label(RichText::new(why).size(t.text_small).color(t.muted));
        ui.add_space(6.0);
    }
    let button = widgets::primary_button(ui, None, label, t.accent, &t);
    if ui.add(button).clicked() {
        out.push(Sent::Engine(Command::StudyMode(Action::NextBatch(pane))));
    }
}

/// `text` with its first letter capitalized, as a title starts.
pub fn capital(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    fn sheet(page: usize, top: f32) -> Sheet {
        let rect = Rect::from_min_size(pos2(100.0, top), vec2(400.0, 600.0));
        Sheet {
            page,
            rect,
            shown: rect,
            scale: 1.0,
        }
    }

    #[test]
    fn a_strip_veils_from_the_first_locked_page_down() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0));
        let sheets = [sheet(3, -300.0), sheet(4, 310.0), sheet(5, 920.0)];
        let veil = veil(&sheets, pane, 4, ViewMode::Vertical);
        let expected = Rect::from_min_max(pos2(0.0, 310.0), pos2(600.0, 800.0));
        assert_eq!(veil.map(|v| v.rect), Some(expected));
        assert_eq!(veil.map(|v| v.reached), Some(true));
    }

    #[test]
    fn a_locked_page_just_peeking_in_is_not_yet_the_gate() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0));
        let sheets = [sheet(3, 100.0), sheet(4, 710.0)];
        let veil = veil(&sheets, pane, 4, ViewMode::Vertical);
        assert_eq!(veil.map(|v| v.reached), Some(false));
        assert!(super::veil(&sheets[..1], pane, 4, ViewMode::Vertical).is_none());
    }

    #[test]
    fn a_lock_frosts_the_pages_before_the_start_and_holds_jumps() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 800.0));
        let sheets = [sheet(1, -300.0), sheet(2, 310.0)];
        let frost = frost(&sheets, pane, 2, ViewMode::Vertical);
        let expected = Rect::from_min_max(pos2(0.0, 0.0), pos2(600.0, 300.0));
        assert_eq!(frost, Some(expected));
        let hold = StudySpan {
            floor: 2,
            gate: Some(6),
        };
        assert_eq!([0, 4, 9].map(|p| hold.clamp(p)), [2, 4, 6]);
    }

    #[test]
    fn titles_start_with_a_capital() {
        assert_eq!(capital("pages 4–6"), "Pages 4–6");
        assert_eq!(capital(""), "");
    }
}
