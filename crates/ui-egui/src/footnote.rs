//! A book's footnote in a card on its mark, so the reader keeps their place.

use egui::{Align, Context, Frame, Id, Layout, Margin, Rect, RichText, ScrollArea, Stroke, Ui};
use mareader_pdf::Bounds;

use crate::bubble::{Bubble, Side};
use crate::selection::Sheet;
use crate::theme::{self, Tokens};
use crate::widgets;

const WIDTH: f32 = 340.0;
const PAD: i8 = 16;
/// The header's and the footer's heights, which the note leaves room for.
const HEAD: f32 = 36.0;
const FOOT: f32 = 40.0;

/// An open note: its mark's page and box, what it says, and its page.
#[derive(Clone, Debug, PartialEq)]
pub struct Footnote {
    pub page: usize,
    pub at: Bounds,
    pub text: String,
    pub target: usize,
}

/// What the card's buttons asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Close,
    Go(usize),
}

/// The mark on screen, while its page shows.
pub fn anchor(note: &Footnote, sheets: &[Sheet]) -> Option<Rect> {
    let sheet = sheets.iter().find(|s| s.page == note.page)?;
    Some(sheet.to_screen(note.at))
}

/// Draws the card on `at`; returns where it is and what was pressed.
pub fn show(
    ctx: &Context,
    id: Id,
    (note, at, room): (&Footnote, Rect, Rect),
    t: &Tokens,
) -> (Rect, Option<Press>) {
    let bubble = Bubble {
        id,
        at,
        room,
        width: WIDTH,
        margin: Margin::ZERO,
        snug: false,
        prefer: Side::Below,
    };
    bubble.show(ctx, t, |ui, cap| card(ui, note, cap, t))
}

fn card(ui: &mut Ui, note: &Footnote, cap: f32, t: &Tokens) -> Option<Press> {
    let mut press = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    let head = Margin {
        left: PAD,
        right: 8,
        top: 6,
        bottom: 2,
    };
    Frame::new().inner_margin(head).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::small_icon_button(ui, crate::icons::Icon::Close, "Close note").clicked()
                {
                    press = Some(Press::Close);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let title = RichText::new("Note").font(theme::medium(12.0));
                    ui.label(title.color(t.muted));
                });
            });
        });
    });
    // As large as the page's words, not a label's.
    let body = Margin {
        left: PAD,
        right: PAD,
        top: 0,
        bottom: 10,
    };
    ScrollArea::vertical()
        .max_height((cap - HEAD - FOOT).max(60.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            Frame::new().inner_margin(body).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for para in note.text.split("\n\n") {
                    ui.label(RichText::new(para).size(t.text + 1.0).color(t.ink));
                }
            });
        });
    let line = ui.available_rect_before_wrap();
    let y = line.top();
    ui.painter()
        .hline(line.x_range(), y, Stroke::new(1.0, t.line));
    let foot = Margin::symmetric(8, 4);
    Frame::new().inner_margin(foot).show(ui, |ui| {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let go = format!("Go to page {}", note.target + 1);
            let button = egui::Button::new(RichText::new(go).color(t.accent));
            if ui.add(button.frame_when_inactive(false)).clicked() {
                press = Some(Press::Go(note.target));
            }
        });
    });
    press
}
