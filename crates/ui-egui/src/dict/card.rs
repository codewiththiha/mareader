//! The lookup card: one entry at a time, its caret on the words.

use egui::{
    Align, Context, CornerRadius, CursorIcon, FontId, Id, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_cefr::Role;
use mareader_dict::{Entry, Pair, Query, Reply, Ticket, language};
use mareader_engine::command::Command;
use mareader_engine::dict::Action;
use mareader_engine::settings::Tab;
use mareader_workspace::PaneId;

use super::entry;
use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::{widgets, word_card};

pub use crate::word_card::PURSUE;

/// Wheel travel that turns to the next entry.
const STEP: f32 = 24.0;
/// A pause this long, in seconds, ends one flick of the wheel.
const SETTLE: f64 = 0.15;
/// Points in one line of a mouse wheel, as egui's default.
const LINE: f32 = 40.0;
const CARD: &str = "dict-card";

/// Which entry the card shows, and the wheel flick turning it.
#[derive(Clone, Copy, Debug, Default)]
struct Pager {
    at: usize,
    pull: f32,
    last: f64,
    /// This flick has turned an entry, or scrolled one: it turns no more.
    spent: bool,
    /// Last frame's entry was scrolled to its top, and to its bottom.
    edges: (bool, bool),
}

pub struct View<'a> {
    pub pane: PaneId,
    pub page: Option<usize>,
    pub word: &'a str,
    /// None while the packs are asked.
    pub reply: Option<&'a Reply>,
    /// The tagger's class and level for the word in its sentence.
    pub role: Option<&'a Role>,
    pub attached: bool,
    pub at: Rect,
    pub room: Rect,
    /// Its word is being said aloud.
    pub saying: bool,
    /// How it sounds, once looked up.
    pub sound: &'a [mareader_engine::sounds::Line],
    /// Who says the word, when a voice here speaks its language.
    pub speaker: Option<&'a mareader_engine::read_aloud::state::Speaker>,
}

/// Steps the open lookup card's pager by `delta` (`-1` or `+1`).
pub fn step(ctx: &Context, word: &str, n: usize, delta: i32) {
    if n <= 1 {
        return;
    }
    let pager_id = word_card::bubble_id(word).with(n);
    let mut p: Pager = ctx.data(|d| d.get_temp(pager_id)).unwrap_or_default();
    let max = (n - 1) as i32;
    let next = (p.at as i32 + delta).clamp(0, max) as usize;
    if next != p.at {
        p.at = next;
        p.edges = (true, true);
        ctx.data_mut(|d| d.insert_temp(pager_id, p));
        ctx.request_repaint();
    }
}

impl super::DictUi {
    /// Drops any stashed lookup card.
    pub fn clear_stash(&mut self) {
        self.stash = None;
    }

    /// Number of entries currently on the open lookup card.
    pub fn card_entries(&self) -> usize {
        self.card
            .as_ref()
            .and_then(|c| c.reply.as_ref())
            .map_or(0, |r| r.entries.len())
    }

    /// Steps the open lookup card's pager by `delta` (`-1` or `+1`).
    pub fn step_page(&self, ctx: &Context, delta: i32) {
        if let Some(c) = &self.card {
            step(ctx, &c.word, self.card_entries(), delta);
        }
    }

    /// Stashes the open lookup card so switching back to Dictionary is instant.
    pub fn stash_for_tab(&mut self) -> Option<word_card::TabSnapshot> {
        let mut card = self.card.take()?;
        card.rest = false;
        let snap = word_card::TabSnapshot {
            pane: card.pane,
            word: card.word.clone(),
            context: card.context.clone(),
            asked: card.asked.clone(),
            at: card.at,
            role: card.role.clone(),
            rect: self.card_rect,
        };
        self.stash = Some(card);
        Some(snap)
    }

    /// Restores a stashed lookup card matching `snap`; false if none fits.
    pub fn restore_from_tab(
        &mut self,
        snap: &word_card::TabSnapshot,
        offset: Option<Vec2>,
    ) -> bool {
        let same = |c: &super::Card| c.pane == snap.pane && c.word == snap.word && c.at == snap.at;
        let Some(mut card) = self.stash.take().filter(same) else {
            return false;
        };
        if card.role.is_none() {
            card.role = snap.role.clone();
        }
        card.offset = offset;
        self.card_rect = snap.rect;
        self.card = Some(card);
        true
    }

    /// Opens the Dictionary tab for `snap` with an already-submitted `ticket`.
    pub fn open_tab_lookup(
        &mut self,
        snap: word_card::TabSnapshot,
        ticket: Ticket,
        offset: Option<Vec2>,
    ) {
        self.card_rect = snap.rect;
        self.card = Some(super::Card {
            pane: snap.pane,
            word: snap.word,
            context: snap.context,
            asked: snap.asked,
            at: snap.at,
            offset,
            ticket,
            reply: None,
            rest: false,
            role: snap.role,
        });
    }

    /// Builds a reversed-pair search query for the open lookup card.
    pub fn swap_query(&self) -> Option<Query> {
        let card = self.card.as_ref()?;
        let reply = card.reply.as_ref()?;
        let (from, to) = (reply.from.as_ref()?, reply.to.as_ref()?);
        let pair = Pair {
            from: Some(to.clone()),
            to: Some(from.clone()),
        };
        Some(Query::Search {
            text: card.word.clone(),
            pair: Some(pair),
            pack: None,
        })
    }

    /// Updates the open lookup card's pending dictionary query ticket.
    pub fn set_card_ticket(&mut self, ticket: Ticket) {
        if let Some(card) = &mut self.card {
            card.ticket = ticket;
        }
    }
}

pub fn show(ctx: &Context, view: &View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let id = word_card::bubble_id(view.word);
    let bubble = Bubble {
        id,
        at: view.at,
        room: view.room,
        width: word_card::WIDTH,
        margin: Margin::ZERO,
        snug: false,
        prefer: Side::Below,
    };
    bubble
        .show(ctx, t, |ui, cap| card(ui, id, view, cap, t, out))
        .0
}

fn card(ui: &mut Ui, id: Id, view: &View<'_>, cap: f32, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let top = ui.cursor().top();
    let (class, level) = class_and_level(view.role, shown_entry(ui, id, view));
    let say = view.speaker.map(|s| word_card::Say {
        saying: view.saying,
        voice: s.voice.as_deref(),
    });
    let head = word_card::Header {
        word: view.word,
        class: &class,
        level,
        sound: view.sound,
        say,
        mode: word_card::Mode::Dictionary,
        close: Command::Dict(Action::CloseCard),
    };
    word_card::header(ui, &head, t, out);
    word_card::rule(ui, t);
    let Some(reply) = view.reply else {
        word_card::section(ui, word_card::PAD_Y, |ui| muted(ui, t, "Looking up…"));
        word_card::rule(ui, t);
        footer(ui, view, None, (&mut Pager::default(), 0), t, out);
        return;
    };
    let pair = reply.from.as_deref().zip(reply.to.as_deref());
    let n = reply.entries.len();
    let pager_id = id.with(n);
    let mut p: Pager = ui.data(|d| d.get_temp(pager_id)).unwrap_or_default();
    p.at = p.at.min(n.saturating_sub(1));
    let Some(e) = reply.entries.get(p.at).filter(|_| view.attached) else {
        word_card::section(ui, word_card::PAD_Y, |ui| {
            if pair.is_some() {
                lang_row(ui, None, pair, t, out);
                ui.add_space(4.0);
            }
            empty(ui, view, reply, t, out);
        });
        word_card::rule(ui, t);
        footer(ui, view, None, (&mut p, 0), t, out);
        return;
    };
    let used = ui.cursor().top() - top + f32::from(word_card::PAD_Y) * 2.0 + word_card::FOOT;
    let list = (cap - used).max(word_card::BODY_MIN);
    word_card::section(ui, word_card::PAD_Y, |ui| {
        body(ui, view.word, e, pair, (&mut p, list), t, out);
    });
    word_card::rule(ui, t);
    footer(ui, view, Some(e), (&mut p, n), t, out);
    turn(&mut p, n, wheel_over(ui.ctx(), id), ui.input(|i| i.time));
    ui.data_mut(|d| d.insert_temp(pager_id, p));
}

/// The entry the pager is on, once there are any.
fn shown_entry<'a>(ui: &Ui, id: Id, view: &View<'a>) -> Option<&'a Entry> {
    let reply = view.reply.filter(|_| view.attached)?;
    let n = reply.entries.len();
    let p: Pager = ui.data(|d| d.get_temp(id.with(n))).unwrap_or_default();
    reply.entries.get(p.at.min(n.saturating_sub(1)))
}

/// The word's class and CEFR level: tagger first, else the entry's tags.
fn class_and_level(role: Option<&Role>, entry: Option<&Entry>) -> (String, Option<&'static str>) {
    match (role, entry) {
        (Some(r), _) => (r.class().to_owned(), r.cefr()),
        (None, Some(e)) => (e.tags(), None),
        (None, None) => (String::new(), None),
    }
}

/// The entry turned to, scrolling only when taller than the room.
fn body(
    ui: &mut Ui,
    asked: &str,
    e: &Entry,
    pair: Option<(&str, &str)>,
    (p, list): (&mut Pager, f32),
    t: &Tokens,
    out: &mut Commands,
) {
    let scrolls_id = Id::new((CARD, asked, p.at, "scrolls"));
    let scrolls = ui.data(|d| d.get_temp::<bool>(scrolls_id).unwrap_or(false));
    let shown = ScrollArea::vertical()
        .id_salt((CARD, asked, p.at))
        .max_height(list)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if scrolls {
                super::page::clear_of_bar(ui);
            }
            ui.spacing_mut().item_spacing.y = 4.0;
            let near = entry::near(e, asked);
            if near.is_some() || pair.is_some() {
                lang_row(ui, near, pair, t, out);
            }
            entry::rows(ui, e, t, false);
        });
    let room = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
    ui.data_mut(|d| d.insert_temp(scrolls_id, room > 0.5));
    let off = shown.state.offset.y;
    p.edges = (off <= 0.5, off >= room - 0.5);
}

/// Near lemma on the left, compact swappable language pair on the right.
fn lang_row(
    ui: &mut Ui,
    near: Option<&str>,
    pair: Option<(&str, &str)>,
    t: &Tokens,
    out: &mut Commands,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some((from, to)) = pair {
                lang_swap_pill(ui, from, to, t, out);
            }
            if let Some(word) = near {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    entry::heading(ui, word, t);
                });
            }
        });
    });
}

/// Compact `EN ⇄ FR` pill that swaps source and target languages on click.
fn lang_swap_pill(ui: &mut Ui, from: &str, to: &str, t: &Tokens, out: &mut Commands) {
    let font = FontId::proportional(t.text_small * 0.92);
    let left = ui
        .painter()
        .layout_no_wrap(from.to_ascii_uppercase(), font.clone(), t.muted);
    let right = ui
        .painter()
        .layout_no_wrap(to.to_ascii_uppercase(), font, t.muted);
    let (icon_w, gap, pad_x) = (11.0, 4.0, 6.0);
    let width = pad_x * 2.0 + left.size().x + gap * 2.0 + icon_w + right.size().x;
    let height = (left.size().y.max(right.size().y) + 4.0).max(18.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let fill = if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        t.hover
    } else {
        t.ink.gamma_multiply(0.06)
    };
    let ink = if resp.hovered() { t.ink } else { t.muted };
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), fill);
    let (mut x, cy) = (rect.left() + pad_x, rect.center().y);
    let lw = left.size().x;
    p.galley(pos2(x, cy - left.size().y * 0.5), left, ink);
    x += lw + gap;
    let icon_rect = Rect::from_center_size(pos2(x + icon_w * 0.5, cy), Vec2::splat(icon_w));
    Icon::Swap.paint(ui, icon_rect, ink);
    x += icon_w + gap;
    ui.painter()
        .galley(pos2(x, cy - right.size().y * 0.5), right, ink);
    let way = format!("{} → {}", language(from), language(to));
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &way));
    let tip = format!("{way} — swap languages");
    widgets::tip(&resp, &tip, None);
    if resp.clicked() {
        out.push(Sent::Ui(UiCommand::WordCardSwapLang));
    }
}

/// Compact footer: `< 1 of N >` pager on the left, actions on the right.
fn footer(
    ui: &mut Ui,
    view: &View<'_>,
    shown: Option<&Entry>,
    (p, n): (&mut Pager, usize),
    t: &Tokens,
    out: &mut Commands,
) {
    word_card::footer_frame(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                word_card::pursue_button(ui, view.word, out);
                if let Some(e) = shown {
                    word_card::copy_button(ui, e.answer(), "Copy translation", out);
                }
                if let Some(page) = view.page {
                    let close = Command::Dict(Action::CloseCard);
                    word_card::word_list_button(ui, view.pane, page, close, t, out);
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if n > 1 {
                        pager(ui, p, n, t);
                    }
                });
            });
        });
    });
}

/// Compact `< 1 of N >` numeric pager.
fn pager(ui: &mut Ui, p: &mut Pager, n: usize, t: &Tokens) {
    let back = ui.add_enabled_ui(p.at > 0, |ui| {
        widgets::small_icon_button(ui, Icon::ChevronLeft, "Previous entry").clicked()
    });
    let text = format!("{} of {n}", p.at + 1);
    let r = ui.add(
        egui::Label::new(RichText::new(&text).size(t.text_small).color(t.muted)).selectable(false),
    );
    let accessible = format!("Entry {} of {n}", p.at + 1);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &accessible));
    widgets::tip(&r, "Use ← / → or scroll to switch entries", None);
    let next = ui.add_enabled_ui(p.at + 1 < n, |ui| {
        widgets::small_icon_button(ui, Icon::ChevronRight, "Next entry").clicked()
    });
    if back.inner {
        p.at = p.at.saturating_sub(1);
    }
    if next.inner {
        p.at += 1;
    }
}

/// Why there is no entry, and the way to one when there is.
fn empty(ui: &mut Ui, view: &View<'_>, reply: &Reply, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.y = 8.0;
    if !view.attached {
        muted(ui, t, "Dictionaries need the desktop app.");
    } else if reply.ready == 0 {
        muted(ui, t, "No dictionary is downloaded.");
        if ui.button("Get dictionaries…").clicked() {
            out.push(Sent::Engine(Command::OpenSettings(Tab::Dictionary)));
        }
    } else {
        let why = match &reply.failed {
            Some(e) => format!("The dictionary couldn't be read: {e}"),
            None => format!("No entry for “{}”.", view.word),
        };
        muted(ui, t, &why);
    }
}

fn muted(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).color(t.muted));
}

/// This frame's wheel, if the pointer is over the card.
fn wheel_over(ctx: &Context, id: Id) -> f32 {
    let card = ctx.memory(|m| m.area_rect(id));
    match (card, ctx.pointer_hover_pos()) {
        (Some(card), Some(at)) if card.contains(at) => ctx.input(wheel_points),
        _ => 0.0,
    }
}

/// The wheel's own travel this frame, before egui smooths it.
fn wheel_points(i: &egui::InputState) -> f32 {
    let one = |e: &egui::Event| match e {
        egui::Event::MouseWheel { unit, delta, .. } => match unit {
            egui::MouseWheelUnit::Point => delta.y,
            egui::MouseWheelUnit::Line => delta.y * LINE,
            egui::MouseWheelUnit::Page => delta.y * STEP * 4.0,
        },
        _ => 0.0,
    };
    i.events.iter().map(one).sum()
}

/// One flick turns one entry, once a scrolled entry has reached its edge.
fn turn(p: &mut Pager, n: usize, dy: f32, now: f64) {
    if dy == 0.0 {
        return;
    }
    if now - p.last > SETTLE {
        (p.spent, p.pull) = (false, 0.0);
    }
    p.last = now;
    let onward = dy < 0.0;
    let edge = if onward { p.edges.1 } else { p.edges.0 };
    if !edge {
        // The flick scrolls the entry; the next one may turn it.
        p.spent = true;
        return;
    }
    if p.spent {
        return;
    }
    p.pull += dy;
    if p.pull.abs() < STEP {
        return;
    }
    let to = if onward {
        Some(p.at + 1).filter(|&to| to < n)
    } else {
        p.at.checked_sub(1)
    };
    if let Some(to) = to {
        p.at = to;
        p.edges = (true, true);
    }
    (p.spent, p.pull) = (true, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `p` after wheel `steps` of `(dy, seconds)`, `n` entries.
    fn wheel(mut p: Pager, n: usize, steps: &[(f32, f64)]) -> Pager {
        for &(dy, now) in steps {
            turn(&mut p, n, dy, now);
        }
        p
    }

    fn fits() -> Pager {
        Pager {
            edges: (true, true),
            last: -1.0,
            ..Pager::default()
        }
    }

    #[test]
    fn each_notch_turns_one_entry_and_stops_at_the_ends() {
        let p = wheel(fits(), 3, &[(-50.0, 0.0), (-50.0, 0.3)]);
        assert_eq!(p.at, 2);
        let p = wheel(p, 3, &[(-50.0, 0.6)]);
        assert_eq!(p.at, 2, "no entry past the last");
        let p = wheel(p, 3, &[(50.0, 0.9), (50.0, 1.2), (50.0, 1.5)]);
        assert_eq!(p.at, 0, "and none before the first");
    }

    #[test]
    fn a_long_trackpad_flick_turns_only_one_entry() {
        let flick: Vec<(f32, f64)> = (0..60).map(|i| (-6.0, f64::from(i) / 60.0)).collect();
        assert_eq!(wheel(fits(), 5, &flick).at, 1);
        // A small nudge turns nothing.
        assert_eq!(wheel(fits(), 5, &[(-6.0, 0.0), (-6.0, 0.02)]).at, 0);
    }

    #[test]
    fn a_tall_entry_scrolls_to_its_end_before_the_next_flick_turns() {
        let tall = Pager {
            edges: (true, false),
            ..fits()
        };
        let p = wheel(tall, 3, &[(-50.0, 0.0)]);
        assert_eq!(p.at, 0, "the entry scrolls instead");
        // Scrolled to its end, the same flick still doesn't turn.
        let p = wheel(
            Pager {
                edges: (false, true),
                ..p
            },
            3,
            &[(-50.0, 0.05)],
        );
        assert_eq!(p.at, 0);
        let p = wheel(p, 3, &[(-50.0, 0.5)]);
        assert_eq!(p.at, 1, "a fresh flick at the end turns");
    }
}
