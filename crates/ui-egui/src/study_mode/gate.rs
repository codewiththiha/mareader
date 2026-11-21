//! The veil over the pages past a pane's gate, and its way to the deck.
//!
//! The view stops at the end of the gate page (`pdf_view`), so locked pages
//! peek in under the veil. Once they fill enough of the pane the veil covers
//! all of it and holds the deck.

use egui::{
    Align, Context, CornerRadius, Id, LayerId, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder,
    pos2,
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
/// A veil this short shows its button alone.
const ROOMY: f32 = 120.0;
/// How much of the paper the veil lets through: none of the words.
const FROST: f32 = 0.97;

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

/// Frosted paper over `rect`, taking every press on it, with `content` on top.
pub fn cover(ctx: &Context, pane: PaneId, rect: Rect, content: impl FnOnce(&mut Ui)) {
    let t = Tokens::get(ctx);
    let id = Id::new(("study-veil", pane));
    let veil = z_order::layer(Tier::PageCard, id);
    let fresh = !ctx.memory(|m| m.areas().is_visible(&veil));
    z_order::area(ctx, Tier::PageCard, id)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, t.paper.gamma_multiply(FROST));
            ui.painter()
                .hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, t.line));
            let inner = UiBuilder::new()
                .max_rect(rect.shrink(16.0))
                .layout(Layout::top_down(Align::Center));
            ui.scope_builder(inner, content);
        });
    if fresh {
        under_cards(ctx, veil);
    }
}

/// Keeps the page cards already open, such as the page menu, over `veil`.
fn under_cards(ctx: &Context, veil: LayerId) {
    let shown: Vec<LayerId> = ctx.memory(|m| {
        let up = |l: &LayerId| *l != veil && m.areas().is_visible(l);
        m.layer_ids().filter(up).collect()
    });
    // Asked outside the memory's lock, which the tier check takes too.
    let card = |l: &LayerId| z_order::tier_of(ctx, *l) == Some(Tier::PageCard);
    for layer in shown.into_iter().filter(card) {
        ctx.move_to_top(layer);
    }
}

/// What unlocks the veiled pages, and the way to their words.
pub fn prompt(ui: &mut Ui, pane: PaneId, range: &str, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(12.0);
    if ui.max_rect().height() >= ROOMY {
        let (mark, _) = ui.allocate_exact_size(egui::Vec2::splat(22.0), Sense::hover());
        Icon::Clock.paint(ui, mark, t.muted);
        let title = format!("{} unlock after their words", capital(range));
        ui.label(RichText::new(title).font(crate::theme::medium(15.0)));
        let why = "Scrolling stops here until you've studied them.";
        ui.label(RichText::new(why).size(t.text_small).color(t.muted));
        ui.add_space(6.0);
    }
    let button = widgets::primary_button(ui, None, "Study the words", t.accent, &t);
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
    fn titles_start_with_a_capital() {
        assert_eq!(capital("pages 4–6"), "Pages 4–6");
        assert_eq!(capital(""), "");
    }
}
