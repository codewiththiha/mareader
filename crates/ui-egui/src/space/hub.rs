//! Space's hub: a card for each tool, its name, what it is for, and what
//! it holds so far; a click opens the tool.

use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Shadow, Stroke, Ui, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::space::Room;
use mareader_library::{Library, MarkKind, WordStatus};

use super::look;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};

/// The hub's own label, which tests and the titlebar find it by.
pub const TITLE: &str = "Space";
const LINE: &str = "Your study tools, gathered from every book you read.";
/// The cards' least width, their height and the room between them.
const CARD_W: f32 = 280.0;
const CARD_H: f32 = 196.0;
const GAP: f32 = 18.0;
const PAD: f32 = 22.0;
/// The icon's tile.
const TILE: f32 = 46.0;

/// One tool's card: where it goes, its mark and tint, and its words.
struct Tool {
    room: Option<Room>,
    icon: Icon,
    tint: Color32,
    name: &'static str,
    about: &'static str,
    held: String,
}

/// The title and its line, then the cards, as many to a row as fit.
pub(super) fn show(ui: &mut Ui, lib: &Library, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 4.0;
    let title = egui::RichText::new(TITLE).font(theme::semibold(30.0));
    ui.add(egui::Label::new(title.color(t.ink)).selectable(false));
    look::quiet(ui, LINE, t.text + 1.0, t);
    ui.add_space(26.0);
    let tools = tools(lib, t);
    let wide = ui.available_width();
    let across = (((wide + GAP) / (CARD_W + GAP)).floor() as usize).clamp(1, tools.len());
    let card_w = (wide - GAP * (across - 1) as f32) / across as f32;
    let rows = tools.len().div_ceil(across);
    let tall = rows as f32 * CARD_H + (rows - 1) as f32 * GAP;
    let (block, _) = ui.allocate_exact_size(vec2(wide, tall), Sense::hover());
    for (i, tool) in tools.iter().enumerate() {
        let (col, row) = ((i % across) as f32, (i / across) as f32);
        let min = block.min + vec2(col * (card_w + GAP), row * (CARD_H + GAP));
        let rect = Rect::from_min_size(min, vec2(card_w, CARD_H));
        if card(ui, rect, tool, t)
            && let Some(room) = tool.room
        {
            out.push(Sent::Engine(Command::ShowSpace(room)));
        }
    }
}

/// Vocabs and Notes, with what each holds, then a card for what comes.
fn tools(lib: &Library, t: &Tokens) -> Vec<Tool> {
    let words: Vec<WordStatus> = lib.words().map(|w| w.status.value).collect();
    let kept = words
        .iter()
        .filter(|s| **s == WordStatus::Memorized)
        .count();
    let marks = lib.every_mark().filter(|m| m.kind != MarkKind::Explain);
    let (mut lit, mut kept_at) = (0, 0);
    for m in marks {
        match m.kind {
            MarkKind::Bookmark => kept_at += 1,
            _ => lit += 1,
        }
    }
    vec![
        Tool {
            room: Some(Room::Words),
            icon: Icon::Levels,
            tint: t.accent,
            name: Room::Words.title(),
            about: "Every word you studied, by book, level and status. Memorize, review, export.",
            held: format!("{} · {kept} memorized", count(words.len(), "word")),
        },
        Tool {
            room: Some(Room::Notes),
            icon: Icon::Note,
            tint: theme::color(t.toast.warning.mark),
            name: Room::Notes.title(),
            about: "Your highlights, notes and bookmarks from every book, ready to revisit.",
            held: format!(
                "{} · {}",
                count(lit, "highlight"),
                count(kept_at, "bookmark")
            ),
        },
        Tool {
            room: None,
            icon: Icon::Ai,
            tint: t.muted,
            name: "More tools",
            about: "AI study helpers will live here: quizzes, reviews and summaries of your notes.",
            held: "Coming soon".to_owned(),
        },
    ]
}

/// `n` and its noun, plural past one.
fn count(n: usize, noun: &str) -> String {
    match n {
        1 => format!("1 {noun}"),
        _ => format!("{n} {noun}s"),
    }
}

/// One card at `rect`: it lifts under the pointer; true when clicked.
fn card(ui: &mut Ui, rect: Rect, tool: &Tool, t: &Tokens) -> bool {
    let id = ui.id().with(("space-tool", tool.name));
    let sense = match tool.room {
        Some(_) => Sense::click(),
        None => Sense::hover(),
    };
    let r = ui.interact(rect, id, sense);
    let open = tool.room.is_some();
    let lift = ui
        .ctx()
        .animate_bool_with_time(id, open && r.hovered(), 0.14);
    let rect = rect.translate(vec2(0.0, -3.0 * lift));
    let painter = ui.painter();
    if open {
        let shadow = Shadow {
            offset: [0, (2.0 + 6.0 * lift) as i8],
            blur: (8.0 + 18.0 * lift) as u8,
            spread: 0,
            color: t.shadow.gamma_multiply(0.35 + 0.65 * lift),
        };
        painter.add(shadow.as_shape(rect, look::ROUND));
    }
    let fill = if open {
        t.card
    } else {
        t.card.gamma_multiply(0.6)
    };
    painter.rect_filled(rect, look::ROUND, fill);
    let edge = match open {
        true => t.line.lerp_to_gamma(tool.tint, 0.35 * lift),
        false => t.line,
    };
    painter.rect_stroke(
        rect,
        look::ROUND,
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(PAD);
    let tile = Rect::from_min_size(inner.min, vec2(TILE, TILE));
    painter.rect_filled(
        tile,
        13.0,
        tool.tint.gamma_multiply(if t.dark { 0.22 } else { 0.14 }),
    );
    tool.icon.paint(
        ui,
        Rect::from_center_size(tile.center(), vec2(22.0, 22.0)),
        tool.tint,
    );
    words(ui, inner, tile, tool, t);
    if open {
        let at = pos2(inner.right() - 10.0 + 4.0 * lift, tile.center().y);
        let ink = t.muted.lerp_to_gamma(t.ink, lift);
        Icon::ChevronRight.paint(ui, Rect::from_center_size(at, vec2(18.0, 18.0)), ink);
        if r.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, open, tool.name));
    r.clicked()
}

/// The name under the tile, what it is for, and what it holds at the foot.
fn words(ui: &Ui, inner: Rect, tile: Rect, tool: &Tool, t: &Tokens) {
    let painter = ui.painter();
    let name_at = pos2(inner.left(), tile.bottom() + 16.0);
    let name_font = theme::semibold(t.text + 6.0);
    let name = painter.text(name_at, Align2::LEFT_TOP, tool.name, name_font, t.ink);
    let mut job = egui::text::LayoutJob::simple(
        tool.about.to_owned(),
        FontId::proportional(t.text),
        t.muted,
        inner.width(),
    );
    job.wrap.max_rows = 2;
    job.wrap.overflow_character = Some('…');
    let about = painter.layout_job(job);
    painter.galley(pos2(inner.left(), name.bottom() + 6.0), about, t.muted);
    let foot = pos2(inner.left(), inner.bottom());
    let held_font = theme::medium(t.text_small);
    let ink = if tool.room.is_some() {
        tool.tint
    } else {
        t.muted
    };
    painter.text(foot, Align2::LEFT_BOTTOM, &tool.held, held_font, ink);
}
