//! The lookup card: one entry at a time, its caret on the words.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Color32, Context, CornerRadius, Direction, Frame, Id, Label, Layout, Margin, Rect,
    RichText, ScrollArea, Sense, Stroke, TextFormat, Ui, vec2,
};
use mareader_cefr::Role;
use mareader_dict::{Entry, Reply, language};

use super::entry;
use crate::bubble::{Bubble, Side};
use crate::icons::Icon;
use crate::shaped;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::dict::Action;
use mareader_engine::settings::Tab;

const WIDTH: f32 = 320.0;
/// The word asked stays whole on up to this many lines, then ends in "…".
const WORD_ROWS: usize = 3;
/// Three header buttons and the air before them.
const BUTTONS_W: f32 = 24.0 * 3.0 + 12.0;
/// Up to this many entries page by dots; more, by a count.
const DOTS_MAX: usize = 7;
const DOT: f32 = 6.0;
/// Room each section keeps from the card's sides, and above and below.
const PAD: i8 = 14;
const PAD_Y: i8 = 10;
/// An entry taller than the card's room scrolls, never below this.
const ENTRY_MIN: f32 = 96.0;
/// The pager row under a word with several entries.
const FOOT: f32 = 36.0;
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
    pub sound: &'a [crate::sounds::Line],
    /// Who says the word, when a voice here speaks its language.
    pub speaker: Option<&'a crate::read_aloud::Speaker>,
}

pub fn show(ctx: &Context, view: &View<'_>, t: &Tokens, out: &mut Commands) -> Rect {
    let id = Id::new((CARD, view.word));
    let bubble = Bubble {
        id,
        at: view.at,
        room: view.room,
        width: WIDTH,
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
    let way = view.reply.and_then(|r| match (&r.from, &r.to) {
        (Some(from), Some(to)) => Some(format!("{} → {}", language(from), language(to))),
        _ => None,
    });
    let class = Class::of(view.role, shown_entry(ui, id, view));
    header(ui, view, (way.as_deref(), &class), t, out);
    rule(ui, t);
    let Some(reply) = view.reply else {
        section(ui, |ui| muted(ui, t, "Looking up…"));
        return;
    };
    let n = reply.entries.len();
    let pager_id = id.with(n);
    let mut p: Pager = ui.data(|d| d.get_temp(pager_id)).unwrap_or_default();
    p.at = p.at.min(n.saturating_sub(1));
    let Some(e) = reply.entries.get(p.at).filter(|_| view.attached) else {
        section(ui, |ui| empty(ui, view, reply, t, out));
        return;
    };
    let paged = n > 1;
    let used = ui.cursor().top() - top + f32::from(PAD_Y) * 2.0;
    let list = (cap - used - if paged { FOOT } else { 0.0 }).max(ENTRY_MIN);
    section(ui, |ui| body(ui, view.word, e, (&mut p, list), t));
    if paged {
        rule(ui, t);
        footer(ui, &mut p, n, t);
    }
    let wheel = wheel_over(ui.ctx(), id);
    turn(&mut p, n, wheel, ui.input(|i| i.time));
    ui.data_mut(|d| d.insert_temp(pager_id, p));
}

/// The entry the pager is on, once there are any.
fn shown_entry<'a>(ui: &Ui, id: Id, view: &View<'a>) -> Option<&'a Entry> {
    let reply = view.reply.filter(|_| view.attached)?;
    let n = reply.entries.len();
    let p: Pager = ui.data(|d| d.get_temp(id.with(n))).unwrap_or_default();
    reply.entries.get(p.at.min(n.saturating_sub(1)))
}

/// The word's class beside it, as the AI card shows it, and its level.
#[derive(Default)]
struct Class {
    name: String,
    level: Option<&'static str>,
}

impl Class {
    /// The tagger's, read in the sentence; else the entry's own tags.
    fn of(role: Option<&Role>, entry: Option<&Entry>) -> Self {
        match (role, entry) {
            (Some(r), _) => Self {
                name: r.class().to_owned(),
                level: r.cefr(),
            },
            (None, Some(e)) => Self {
                name: e.tags(),
                level: None,
            },
            (None, None) => Self::default(),
        }
    }

    fn name(&self, t: &Tokens) -> RichText {
        RichText::new(&self.name)
            .size(t.text)
            .italics()
            .color(t.muted)
    }

    fn level(level: &str, t: &Tokens) -> RichText {
        let text = RichText::new(level).size(t.text_small).strong();
        text.color(t.muted)
    }

    /// Its width on one line, to keep room for it beside the word.
    fn width(&self, ui: &Ui, t: &Tokens) -> f32 {
        // Each part's space before it, plus a point to spare.
        let gap = 7.0;
        let wide = |text: &str, size: f32| {
            let font = egui::FontId::proportional(size);
            ui.painter()
                .layout_no_wrap(text.to_owned(), font, t.muted)
                .size()
                .x
                + gap
        };
        let name = (!self.name.is_empty()).then(|| wide(&self.name, t.text));
        let level = self.level.map(|l| wide(l, t.text_small));
        name.unwrap_or(0.0) + level.unwrap_or(0.0)
    }

    fn show(&self, ui: &mut Ui, t: &Tokens) {
        if !self.name.is_empty() {
            ui.add_space(6.0);
            ui.add(Label::new(self.name(t)).selectable(false));
        }
        if let Some(level) = self.level {
            ui.add_space(6.0);
            let r = ui.add(Label::new(Self::level(level, t)).selectable(false));
            widgets::tip(&r, "This sense's CEFR level", None);
        }
    }
}

/// The button carrying the card's word into the Dictionary.
pub const PURSUE: &str = "Open in the Dictionary";

/// The word large and whole, its buttons, and the languages under it.
fn header(
    ui: &mut Ui,
    view: &View<'_>,
    (way, class): (Option<&str>, &Class),
    t: &Tokens,
    out: &mut Commands,
) {
    let word = view.word;
    let margin = Margin {
        left: PAD,
        right: 8,
        top: 10,
        bottom: 10,
    };
    Frame::new().inner_margin(margin).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        let room = (ui.available_width() - BUTTONS_W - class.width(ui, t)).max(60.0);
        let headword = Headword::new(ui, word, room, t);
        // One line sits level with the buttons; more hang from the top.
        let align = if headword.rows() > 1 {
            Align::Min
        } else {
            Align::Center
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.with_layout(Layout::right_to_left(align), |ui| {
                if widgets::small_icon_button(ui, Icon::Close, "Close").clicked() {
                    out.push(Sent::Engine(Command::Dict(Action::CloseCard)));
                }
                if widgets::small_icon_button(ui, Icon::Dictionary, PURSUE).clicked() {
                    out.push(Sent::Engine(Command::Dict(Action::Pursue(word.to_owned()))));
                }
                if let Some(s) = view.speaker {
                    let voice = s.voice.as_deref();
                    crate::read_aloud::say_button(ui, word, (view.saying, voice), out);
                }
                ui.with_layout(Layout::left_to_right(align), |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    // Its own scope, so a width it sets leaves the class room.
                    ui.scope(|ui| headword.show(ui, t));
                    class.show(ui, t);
                });
            });
        });
        crate::sounds::show(ui, view.sound, t);
        if let Some(way) = way {
            languages(ui, way, t);
        }
    });
}

/// The word asked, laid out for the header.
enum Headword<'a> {
    /// Burmese and its kin, shaped on drawing.
    Shaped(&'a str, f32),
    Laid(&'a str, std::sync::Arc<egui::Galley>),
}

impl<'a> Headword<'a> {
    /// `word` laid out within `room`.
    fn new(ui: &Ui, word: &'a str, room: f32, t: &Tokens) -> Self {
        if shaped::needs_shaping(word) {
            return Self::Shaped(word, room);
        }
        let font = theme::medium(t.text * 1.3);
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
                shaped::label(ui, word, t.text * 1.15, t.ink);
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

/// "English → Burmese" in a quiet pill under the word.
fn languages(ui: &mut Ui, way: &str, t: &Tokens) {
    Frame::new()
        .fill(t.ink.gamma_multiply(0.07))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            let text = RichText::new(way).size(t.text_small * 0.95);
            ui.add(Label::new(text.color(t.muted)).selectable(false));
        });
}

/// The entry turned to, scrolling only when taller than the room.
fn body(ui: &mut Ui, asked: &str, e: &Entry, (p, list): (&mut Pager, f32), t: &Tokens) {
    // Only an entry that scrolls gives its bar the room.
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
            if let Some(word) = entry::near(e, asked) {
                entry::heading(ui, word, t);
            }
            // The class sits by the word, as on the AI card.
            entry::rows(ui, e, t, false);
        });
    let room = (shown.content_size.y - shown.inner_rect.height()).max(0.0);
    ui.data_mut(|d| d.insert_temp(scrolls_id, room > 0.5));
    let off = shown.state.offset.y;
    p.edges = (off <= 0.5, off >= room - 0.5);
}

/// Dots, or "2 of 9", between the arrows: one entry at a time.
fn footer(ui: &mut Ui, p: &mut Pager, n: usize, t: &Tokens) {
    Frame::new()
        .inner_margin(Margin::symmetric(8, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let back = ui.add_enabled_ui(p.at > 0, |ui| {
                    widgets::small_icon_button(ui, Icon::ChevronLeft, "Previous entry").clicked()
                });
                let middle = vec2(ui.available_width() - 24.0, 24.0);
                let centred = Layout::centered_and_justified(Direction::LeftToRight);
                ui.allocate_ui_with_layout(middle, centred, |ui| {
                    if n <= DOTS_MAX {
                        dots(ui, p, n, t);
                    } else {
                        let count = format!("{} of {n}", p.at + 1);
                        ui.label(RichText::new(count).size(t.text_small).color(t.muted));
                    }
                });
                let next = ui.add_enabled_ui(p.at + 1 < n, |ui| {
                    widgets::small_icon_button(ui, Icon::ChevronRight, "Next entry").clicked()
                });
                if back.inner {
                    p.at = p.at.saturating_sub(1);
                }
                if next.inner {
                    p.at += 1;
                }
            });
        });
}

/// A dot per entry, the shown one long and accented; a click turns.
fn dots(ui: &mut Ui, p: &mut Pager, n: usize, t: &Tokens) {
    let gap = DOT * 1.2;
    let long = DOT * 2.6;
    let width = long + (n.saturating_sub(1)) as f32 * (DOT + gap);
    let row = Rect::from_center_size(ui.max_rect().center(), vec2(width, DOT * 3.0));
    let response = ui.allocate_rect(row, Sense::hover());
    let label = format!("Entry {} of {n}", p.at + 1);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    let mut x = row.left();
    for i in 0..n {
        let w = if i == p.at { long } else { DOT };
        let dot = Rect::from_min_size(egui::pos2(x, row.center().y - DOT / 2.0), vec2(w, DOT));
        let hit = ui.interact(
            dot.expand(gap / 2.0),
            ui.id().with(("dot", i)),
            Sense::click(),
        );
        let fill: Color32 = match (i == p.at, hit.hovered()) {
            (true, _) => t.accent,
            (false, true) => t.muted,
            (false, false) => t.muted.gamma_multiply(0.45),
        };
        ui.painter().rect_filled(dot, CornerRadius::same(255), fill);
        if hit.clicked() {
            p.at = i;
        }
        x += w + gap;
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

/// A padded block across the card.
fn section(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new()
        .inner_margin(Margin::symmetric(PAD, PAD_Y))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// A hairline from edge to edge.
fn rule(ui: &mut Ui, t: &Tokens) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter()
        .hline(r.x_range(), r.center().y, Stroke::new(1.0, t.line));
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
