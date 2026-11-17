//! Shared shell for the unified Dictionary and AI word card.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Label, Layout, Margin, Rect,
    RichText, Sense, Stroke, TextFormat, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_cefr::Role;
use mareader_engine::command::Command;
use mareader_engine::dict::Action as DictAction;
use mareader_engine::study::words::Asked;
use mareader_engine::toast::Toast;
use mareader_workspace::PaneId;

use crate::icons::Icon;
use crate::shaped;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::{vocab, widgets};

pub const WIDTH: f32 = 320.0;
pub const PAD: i8 = 14;
pub const PAD_Y: i8 = 10;
pub const FOOT: f32 = 36.0;
pub const BODY_MIN: f32 = 96.0;
/// The button carrying the card's word into the Dictionary.
pub const PURSUE: &str = "Open in the Dictionary";

const WORD_ROWS: usize = 3;
const CLOSE_W: f32 = 28.0;

/// Which answer tab the unified word card shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Dictionary,
    Ai,
}

/// Shared word-card state handed across Dictionary and AI tab switches.
#[derive(Clone, Debug)]
pub struct TabSnapshot {
    pub pane: PaneId,
    pub word: String,
    pub context: String,
    pub asked: Option<Asked>,
    pub at: Rect,
    pub role: Option<Role>,
    pub rect: Option<Rect>,
}

/// Optional speak button state for the metadata sub-row.
#[derive(Clone, Copy)]
pub struct Say<'a> {
    pub saying: bool,
    pub voice: Option<&'a str>,
}

/// Everything the unified header draws above the divider.
pub struct Header<'a> {
    pub word: &'a str,
    pub class: &'a str,
    pub level: Option<&'a str>,
    pub sound: &'a [mareader_engine::sounds::Line],
    pub say: Option<Say<'a>>,
    pub mode: Mode,
    pub close: Command,
}

/// Shared bubble identity so switching tabs keeps position and caret.
pub fn bubble_id(word: &str) -> Id {
    Id::new(("word-card", word))
}

/// Fill colour for a CEFR level chip, reusing the page's band palette.
pub fn cefr_chip_fill(level: &str, dark: bool) -> Color32 {
    let band = vocab::cefr_band(level).unwrap_or(3);
    let base = theme::color(vocab::level_color(band));
    if dark {
        base.lerp_to_gamma(Color32::WHITE, 0.14)
    } else {
        base
    }
}

/// Renders a coloured CEFR level chip matching the page's level tint.
pub fn cefr_chip(ui: &mut Ui, level: &str, t: &Tokens) {
    let fill = cefr_chip_fill(level, t.dark);
    let ink = theme::ink_on(fill);
    let font = theme::medium(t.text_small * 0.92);
    let galley = ui.painter().layout_no_wrap(level.to_owned(), font, ink);
    let size = vec2(galley.size().x + 12.0, (galley.size().y + 4.0).max(18.0));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    let at = pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(at, galley, ink);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, level));
    widgets::tip(&resp, "This sense's CEFR level", None);
}

/// Draws the shared header: headword + POS + close, metadata row, and tabs.
pub fn header(ui: &mut Ui, info: &Header<'_>, t: &Tokens, out: &mut Commands) {
    let margin = Margin {
        left: PAD,
        right: 10,
        top: 10,
        bottom: 10,
    };
    Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        title_row(ui, info, t, out);
        if info.level.is_some() || !info.sound.is_empty() || info.say.is_some() {
            meta_row(ui, info, t, out);
        }
        let mut mode = info.mode;
        let tab_id = Id::new(("word-card-tab", info.word));
        let all = [Mode::Dictionary, Mode::Ai];
        let changed = widgets::tab_bar(ui, tab_id, &mut mode, &all, |m| match m {
            Mode::Dictionary => (Some(Icon::Dictionary), "Dictionary"),
            Mode::Ai => (Some(Icon::Ai), "AI in context"),
        });
        if changed {
            out.push(Sent::Ui(UiCommand::WordCardMode(mode)));
        }
    });
}

fn title_row(ui: &mut Ui, info: &Header<'_>, t: &Tokens, out: &mut Commands) {
    let class_w = if info.class.is_empty() {
        0.0
    } else {
        let font = FontId::proportional(t.text);
        ui.painter()
            .layout_no_wrap(info.class.to_owned(), font, t.muted)
            .size()
            .x
            + 8.0
    };
    let room = (ui.available_width() - CLOSE_W - class_w).max(60.0);
    let headword = Headword::new(ui, info.word, room, t);
    let align = if headword.rows() > 1 {
        Align::Min
    } else {
        Align::Center
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.with_layout(Layout::right_to_left(align), |ui| {
            if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                out.push(Sent::Engine(info.close.clone()));
            }
            ui.with_layout(Layout::left_to_right(align), |ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.scope(|ui| headword.show(ui, t));
                if !info.class.is_empty() {
                    ui.add_space(7.0);
                    let text = RichText::new(info.class)
                        .size(t.text)
                        .italics()
                        .color(t.muted);
                    ui.add(Label::new(text).selectable(false));
                }
            });
        });
    });
}

fn meta_row(ui: &mut Ui, info: &Header<'_>, t: &Tokens, out: &mut Commands) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
        ui.spacing_mut().interact_size.y = 20.0;
        if let Some(level) = info.level {
            cefr_chip(ui, level, t);
        }
        if !info.sound.is_empty() {
            crate::sounds::show_inline(ui, info.sound, t);
        }
        if let Some(say) = info.say {
            crate::read_aloud::say_button(ui, info.word, (say.saying, say.voice), out);
        }
    });
}

/// The word asked, laid out for the header.
enum Headword<'a> {
    Shaped(&'a str, f32),
    Laid(&'a str, std::sync::Arc<egui::Galley>),
}

impl<'a> Headword<'a> {
    fn new(ui: &Ui, word: &'a str, room: f32, t: &Tokens) -> Self {
        if shaped::needs_shaping(word) {
            return Self::Shaped(word, room);
        }
        let font = theme::medium(t.text * 1.4);
        let mut job = LayoutJob::single_section(word.to_owned(), TextFormat::simple(font, t.ink));
        job.wrap = TextWrapping {
            max_width: room,
            max_rows: WORD_ROWS,
            break_anywhere: false,
            overflow_character: Some('…'),
        };
        Self::Laid(word, ui.painter().layout_job(job))
    }

    fn rows(&self) -> usize {
        match self {
            Self::Shaped(..) => 1,
            Self::Laid(_, galley) => galley.rows.len(),
        }
    }

    fn show(self, ui: &mut Ui, t: &Tokens) {
        match self {
            Self::Shaped(word, room) => {
                ui.set_max_width(room);
                shaped::label(ui, word, t.text * 1.2, t.ink);
            }
            Self::Laid(word, galley) => {
                let cut = galley.rows.len() >= WORD_ROWS;
                let shown = ui.add(Label::new(galley).selectable(false));
                if cut {
                    shown.on_hover_text(word);
                }
            }
        }
    }
}

/// A hairline from edge to edge.
pub fn rule(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, t.line));
}

/// A padded block across the card.
pub fn section(ui: &mut Ui, pad_y: i8, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .inner_margin(Margin::symmetric(PAD, pad_y))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// The card's bottom footer bar container.
pub fn footer_frame(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Opens the current word in the Dictionary window or page.
pub fn pursue_button(ui: &mut Ui, word: &str, out: &mut Commands) {
    if widgets::small_icon_button(ui, Icon::Dictionary, PURSUE).clicked() {
        out.push(Sent::Engine(Command::Dict(DictAction::Pursue(
            word.to_owned(),
        ))));
    }
}

/// Copies `text` to the clipboard and raises a brief confirmation toast.
pub fn copy_button(ui: &mut Ui, text: &str, label: &str, out: &mut Commands) {
    if widgets::small_icon_button(ui, Icon::Copy, label).clicked() {
        ui.ctx().copy_text(text.to_owned());
        out.push(Sent::Ui(UiCommand::Notify(Toast::info("Copied"))));
    }
}

/// Opens this page's word list window from the card footer.
pub fn word_list_button(
    ui: &mut Ui,
    pane: PaneId,
    page: usize,
    close: Command,
    t: &Tokens,
    out: &mut Commands,
) {
    let font = FontId::proportional(t.text_small);
    let galley = ui
        .painter()
        .layout_no_wrap("Word list".to_owned(), font, t.ink);
    let size = vec2(galley.size().x + 26.0, 24.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        ui.painter().rect_filled(rect, t.radius, t.hover);
    }
    let ink = if resp.hovered() { t.ink } else { t.muted };
    let icon_at = pos2(rect.left() + 11.0, rect.center().y);
    let glyph = Rect::from_center_size(icon_at, Vec2::splat(12.0));
    Icon::Plus.paint(ui, glyph, ink);
    let text_pos = pos2(rect.left() + 21.0, rect.center().y - galley.size().y * 0.5);
    ui.painter().galley(text_pos, galley, ink);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Word list"));
    widgets::tip(&resp, "Open this page's word list", None);
    if resp.clicked() {
        out.push(Sent::Engine(close));
        out.push(Sent::Engine(Command::WordListWindow {
            pane,
            pages: vec![page],
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cefr_chip_matches_page_band_colours() {
        for (label, band) in [
            ("A1", 1),
            ("A2", 2),
            ("B1", 3),
            ("B2", 4),
            ("C1", 5),
            ("C2", 6),
        ] {
            assert_eq!(
                cefr_chip_fill(label, false),
                theme::color(vocab::level_color(band))
            );
        }
    }
}
