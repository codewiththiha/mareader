//! Reads a narrated book aloud: its player, the phrase lit, the bar.

use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use egui::{Color32, Context, CornerRadius, Id, Order, Rect, Ui};
use mareader_engine::command::Command;
use mareader_engine::read_aloud::lit::Span;
use mareader_engine::read_aloud::parts;
use mareader_engine::read_aloud::place::Stand;
use mareader_engine::read_aloud::player::{Act, Lyrics, View};
use mareader_engine::read_aloud::state::{State as ReadAloudState, remember_skip};
use mareader_engine::session::Session;
use mareader_session::Anchor;
use mareader_settings::{Player as Look, ReadAloudSettings};
use mareader_workspace::PaneId;

pub(crate) use mareader_engine::read_aloud::booth::Booth;
pub(crate) use mareader_engine::read_aloud::lit::Lit;
pub(crate) use mareader_engine::read_aloud::state::{Offer, Origin, Speaker};

use crate::documents::Reading;
use crate::icons::Icon;
use crate::selection::Pages;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::{motion, widgets};

mod ask;
mod card;
mod control;
mod edges;
pub mod marker;

use control::Fit;
use edges::Resize;

/// Seconds the player takes to rise into place.
const APPEAR: f64 = 0.22;
/// Seconds the pill takes to open into the card, and back.
const MORPH: f32 = 0.34;
/// Seconds the words lit may be out of sight before the card shows.
const HIDDEN: f64 = 0.8;
/// The player's place closes this share of its gap each second, roughly.
const GLIDE: f32 = 14.0;
/// The speaker on a word's card, and the same button while it speaks.
pub const SAY: &str = "Say aloud";
pub const STOP_SAYING: &str = "Stop saying";

/// Where the player may show, and whether the words read are in sight.
pub struct Stage<'a> {
    /// The panes are up: the reader is the page shown.
    pub reader: bool,
    pub panes: &'a [(PaneId, Rect)],
    /// The window's room below the title bar.
    pub content: Rect,
    /// Whether the words lit are in sight, when they can be told.
    pub shown: Option<bool>,
    /// The title of the book read.
    pub title: &'a str,
    /// Library identity for per-book choices; none for an unsaved document.
    pub book: Option<mareader_library::BookId>,
    /// Its pane's own look, while it has one; else the window's.
    pub tint: Option<Tokens>,
    /// Active toasts occupying the corner, if any.
    pub toasts: Option<Rect>,
    /// The Dictionary's floating window, if it is up.
    pub dict: Option<Rect>,
    pub parking: mareader_settings::EdgeParking,
}

/// The player's colours, easing from one look to the next.
#[derive(Clone, Copy)]
struct Tint {
    from: Tokens,
    to: Tokens,
    since: f64,
}

impl Tint {
    /// The colours at `now`, eased over `time` seconds.
    fn at(&self, now: f64, time: f64) -> Tokens {
        let k = match time > 0.0 {
            true => ((now - self.since) / time).clamp(0.0, 1.0) as f32,
            false => 1.0,
        };
        self.from.toward(&self.to, ease_in_out(k))
    }
}

/// The player's shape and place, kept between frames.
#[derive(Default)]
struct Shape {
    /// The card or the pill, asked by hand until the need for it changes.
    chosen: Option<bool>,
    /// The words read can't be seen, so the card would help.
    auto: bool,
    /// Since when the words lit have been out of sight.
    hidden_since: Option<f64>,
    /// Distinguishes a scroll-away from a highlight hidden at first.
    highlight_seen: bool,
    /// Whether the card is wanted, this frame.
    card: bool,
    /// The foot centre it sits on, gliding where it should.
    at: Option<egui::Pos2>,
    /// The panel's current rect, shared with dictionary avoidance.
    rect: Option<Rect>,
    /// The player sliding partly beyond the reader's bottom edge.
    parked: Option<crate::park::Tuck>,
    orb_target: Option<Rect>,
    /// A short grace period while the player is summoned or restored.
    unparked_until: Option<f64>,
    /// A reveal or shape change keeps the dictionary from covering the player.
    player_priority: bool,
    /// The mode last shown, to detect a user change.
    last_look: Option<Look>,
    time: Option<f64>,
    scroll: card::Scroll,
    ticker: card::Ticker,
    /// When the player first showed for this reading.
    appeared: Option<f64>,
    /// Its width while a side is dragged.
    dragged: Option<f32>,
    /// The width it eases to while toasts hold the corner.
    dodge_toast: Option<f32>,
    tint: Option<Tint>,
    /// How far it stands above its book's pane foot; 0 elsewhere.
    rise: f32,
    /// The time left it shows, running down evenly.
    countdown: mareader_engine::read_aloud::countdown::Countdown,
    /// The part last asked about, kept while its question fades.
    asked: Option<parts::Kind>,
    /// Opted in for the current unanswered question only.
    remember_skip: bool,
}

/// Window-ephemeral read-aloud UI state: player shape, animations, parking.
#[derive(Default)]
pub struct ReadAloudUi {
    shape: Shape,
}

/// Read-only view over engine read-aloud state and window player overlays.
#[derive(Clone, Copy)]
pub struct ReadAloudView<'a> {
    pub booth: &'a Booth,
    pub shelf: &'a mareader_speech::Shelf,
    state: &'a ReadAloudState,
    ui: &'a ReadAloudUi,
}

impl<'a> ReadAloudView<'a> {
    pub fn new(state: &'a ReadAloudState, ui: &'a ReadAloudUi) -> Self {
        Self {
            booth: &state.booth,
            shelf: &state.shelf,
            state,
            ui,
        }
    }

    pub fn active(&self) -> bool {
        self.state.active()
    }

    pub fn playing(&self) -> bool {
        self.state.playing()
    }

    pub fn is_asking(&self) -> bool {
        self.state.is_asking()
    }

    pub fn away(&self) -> Option<&'a Origin> {
        self.state.away()
    }

    pub fn origin(&self) -> Option<&'a Origin> {
        self.state.origin()
    }

    pub fn reading_spot(&self) -> Option<(usize, Option<Range<usize>>)> {
        self.state.reading_spot()
    }

    pub fn home(&self) -> Option<PaneId> {
        self.state.home()
    }

    pub fn saying(&self) -> Option<&'a str> {
        self.state.saying()
    }

    pub fn words_said(&self) -> u64 {
        self.state.words_said()
    }

    pub fn said(&self) -> Option<(PaneId, Anchor)> {
        self.state.said()
    }

    pub fn lit(&self, pane: PaneId) -> Option<Lit> {
        self.state.lit(pane)
    }

    pub fn spot(&self) -> Option<(PaneId, usize, Option<Range<usize>>)> {
        self.state.spot()
    }

    pub fn colours(&self) -> Option<Tokens> {
        self.ui.colours()
    }

    pub fn carded(&self) -> bool {
        self.state.active() && self.ui.shape.card
    }

    pub fn island_anchor(&self) -> Option<Rect> {
        self.ui.island_anchor(self.state.active())
    }

    pub fn clearance_rect(&self, now: f64) -> Option<Rect> {
        self.ui.clearance_rect(self.state.active(), now)
    }
}

impl ReadAloudUi {
    /// Starts reading from `reading`, resetting window shape when fresh.
    /// Updates the player bar's transient state after playback starts.
    pub fn on_started(&mut self, fresh: bool) {
        if fresh {
            self.shape = Shape::default();
        } else {
            self.shape.hidden_since = None;
            self.shape.highlight_seen = false;
        }
    }

    /// Answer a pending question from the keyboard; playback is untouched.
    pub fn answer_question(
        &mut self,
        session: &mut Session,
        read: bool,
        settings: &mut ReadAloudSettings,
    ) -> bool {
        let remember = self
            .shape
            .remember_skip
            .then_some(self.shape.asked)
            .flatten();
        if !session.read_aloud_answer(read, remember, settings) {
            return false;
        }
        self.shape.remember_skip = false;
        true
    }

    pub fn rise(&self) -> f32 {
        self.shape.rise
    }

    /// The look the player wears, or eases to, once it has shown.
    pub fn colours(&self) -> Option<Tokens> {
        self.shape.tint.map(|t| t.to)
    }

    /// Lets a dictionary interaction reclaim its space.
    pub fn yield_to_dictionary(&mut self) {
        self.shape.player_priority = false;
        self.shape.unparked_until = None;
    }

    pub fn island_anchor(&self, active: bool) -> Option<Rect> {
        if !active
            || self.shape.parked.is_some()
            || self.shape.orb_target.is_some()
            || self.shape.card
        {
            return None;
        }
        self.shape.rect
    }

    /// The player's intended rect while summoned, restoring or kept open.
    pub fn clearance_rect(&self, active: bool, now: f64) -> Option<Rect> {
        if !active {
            return None;
        }
        let clear = self.shape.player_priority
            || self.shape.unparked_until.is_some_and(|until| now < until);
        match self.shape.parked {
            Some(tuck) if tuck.revealing || clear => Some(tuck.home),
            Some(_) => None,
            None if clear => self.shape.rect,
            None => None,
        }
    }

    /// The player: a pill by its pane, or a card when the words are away.
    pub fn bar(
        &mut self,
        ctx: &Context,
        state: &ReadAloudState,
        stage: Stage<'_>,
        settings: &mut ReadAloudSettings,
        out: &mut Commands,
        cover: &mut dyn FnMut(&Ui, Rect) -> bool,
    ) -> Option<Act> {
        if !state.active() {
            self.shape = Shape::default();
            return None;
        }
        let home = state.home();
        let shape = &mut self.shape;
        let now = ctx.input(|i| i.time);
        let pane = (stage.panes.iter())
            .find(|(p, _)| stage.reader && Some(*p) == home)
            .map(|(_, r)| *r);
        if stage.shown == Some(true) {
            shape.highlight_seen = true;
        }
        match stage.shown {
            Some(false) => _ = *shape.hidden_since.get_or_insert(now),
            _ => shape.hidden_since = None,
        }
        // The page shows the words unless they are elsewhere or drift.
        let drifted = shape.hidden_since.is_some_and(|t| now - t >= HIDDEN);
        let auto = pane.is_none() || !settings.follow || drifted;
        if auto != shape.auto {
            shape.auto = auto;
            shape.chosen = None;
        }
        let room = pane.unwrap_or(stage.content);
        // Bar only opens when asked, and then compact; small rooms shrink it.
        let style = settings.player;
        let style_changed = shape.last_look.is_some_and(|last| last != style);
        shape.last_look = Some(style);
        if style_changed {
            shape.player_priority = true;
            shape.unparked_until = None;
        }
        let fits = room.height() >= card::COMPACT_HEIGHT + 80.0 && room.width() >= 300.0;
        let roomy = room.height() >= card::HEIGHT + 120.0 && room.width() >= 360.0;
        let wanted = shape.chosen.unwrap_or(auto && style != Look::Bar);
        if !shape.card && wanted && fits && drifted && shape.highlight_seen {
            shape.player_priority = true;
            shape.unparked_until = None;
        }
        shape.card = wanted && fits;
        let morph = motion::secs(ctx, MORPH);
        let k = ctx.animate_bool_with_time(Id::new("read-aloud-card"), shape.card, morph);
        let k = ease_in_out(k);
        let full = style == Look::Full && roomy;
        let f = ctx.animate_bool_with_time(Id::new("read-aloud-full"), full, morph);
        let f = ease_in_out(f);
        let just_appeared = shape.appeared.is_none();
        let since = now - *shape.appeared.get_or_insert(now);
        if just_appeared {
            shape.unparked_until = Some(now + 4.0);
        }
        let shown = match motion::enabled(ctx) {
            true => ease_out((since / APPEAR).clamp(0.0, 1.0)) as f32,
            false => 1.0,
        };
        if shown < 1.0 {
            ctx.request_repaint();
        }
        // It sits over the page bar in a pane, else near the window's foot.
        let foot = match pane.is_some() || stage.reader {
            true => room.bottom() - crate::page_bar::BAR_H - 12.0,
            false => room.bottom() - 20.0,
        };
        let goal = egui::pos2(room.center().x, foot);
        let dt = shape.time.map_or(0.0, |t| (now - t) as f32).min(0.1);
        shape.time = Some(now);
        let at = match shape.at {
            Some(at) if motion::enabled(ctx) => at + (goal - at) * (1.0 - (-dt * GLIDE).exp()),
            _ => goal,
        };
        shape.at = Some(at);
        if (goal - at).length() > 0.5 {
            ctx.request_repaint();
        }
        // It wears its book's look, easing there as it moves between panes.
        let goal = stage.tint.unwrap_or_else(|| Tokens::get(ctx));
        let tint = shape.tint.get_or_insert(Tint {
            from: goal,
            to: goal,
            since: now,
        });
        if tint.to != goal {
            let from = tint.at(now, f64::from(morph));
            *tint = Tint {
                from,
                to: goal,
                since: now,
            };
        }
        let colours = tint.at(now, f64::from(morph));
        if colours != goal {
            ctx.request_repaint();
        }
        let act = colours.lend(ctx, || {
            let hint = |share: f32| state.hint(share);
            let cost = state.ask_cost(settings.rate);
            let view = state.view(&hint)?;
            let estimate = mareader_engine::read_aloud::countdown::Estimate {
                left: view.left,
                at: view.at,
                playing: view.playing,
                rate: settings.rate,
            };
            let left = shape.countdown.show(estimate, now);
            let view = View { left, ..view };
            let about = state.about(stage.title)?;
            let lyrics = state.lyrics();
            // Compact grows up from the bar; full widens into a card.
            let escape = view.escape.is_some();
            // Unset, it starts as narrow as it goes, its least parts kept.
            let (narrowest, widest) = ReadAloudSettings::PLAYER_WIDTH;
            let parts = narrowest.max(Fit::new(0.0, escape).width(escape));
            // A width set by hand holds in every shape, if the room has it.
            let set = shape.dragged.or(settings.player_width);
            let pill = set.map_or(parts, |w| w.clamp(narrowest, widest));
            let open = card::lerp(pill, card::FULL_LEAST.max(pill), f);
            let most = (room.width() - 16.0).max(0.0);
            let mut width = card::lerp(pill, open, k).min(most);
            let tall = card::lerp(card::COMPACT_HEIGHT, card::HEIGHT, f);
            let height = card::lerp(control::HEIGHT, tall, k);
            let bottom = at.y + (1.0 - shown) * 10.0;
            let mut rect = Rect::from_min_size(
                egui::pos2(at.x - width / 2.0, bottom - height),
                egui::vec2(width, height),
            );
            // Idling across an active toast bed: it narrows, easing both ways.
            let toasts_in_way = stage
                .toasts
                .filter(|t| shape.dragged.is_none() && rect.intersects(*t));
            if let Some(toasts) = toasts_in_way {
                let open_parts = card::lerp(parts, card::FULL_LEAST.max(parts), f);
                let min_width = card::lerp(parts, open_parts, k).min(most);
                let clear = ((toasts.left() - 8.0 - at.x) * 2.0).max(narrowest);
                shape.dodge_toast = Some(min_width.min(clear));
            }
            let dodging = toasts_in_way.is_some();
            let dodge_k =
                ctx.animate_bool_with_time(Id::new("read-aloud-toast-dodge"), dodging, morph);
            if let Some(dodged) = shape.dodge_toast {
                width = card::lerp(width, dodged, ease_in_out(dodge_k));
                rect = Rect::from_min_size(
                    egui::pos2(at.x - width / 2.0, bottom - height),
                    egui::vec2(width, height),
                );
            }
            if !dodging && dodge_k <= 0.0 {
                shape.dodge_toast = None;
            }
            // Something summoned sits beneath it: it glides above, and back.
            let under = crate::toast::kept_clear(ctx)
                .into_iter()
                .filter(|foe| foe.intersects(rect))
                .map(|foe| rect.bottom() - foe.top() + 8.0)
                .fold(0.0_f32, f32::max);
            let risen =
                ctx.animate_value_with_time(Id::new("read-aloud-keeper-rise"), under, morph);
            rect = rect.translate(egui::vec2(0.0, -risen));
            let asked_up =
                shape.player_priority || shape.unparked_until.is_some_and(|until| now < until);
            if stage.dict.is_some()
                && let Some(until) = shape.unparked_until.filter(|until| now < *until)
            {
                ctx.request_repaint_after(Duration::from_secs_f64(until - now));
            }
            let covered = stage.dict.is_some_and(|dict| rect.intersects(dict));
            let obstructed = covered && !asked_up && under <= 0.0;
            let orb = obstructed && stage.parking == mareader_settings::EdgeParking::Tab;
            let parking = obstructed && stage.parking == mareader_settings::EdgeParking::Sliver;
            let orb_k = ctx.animate_bool_with_time(Id::new("read-aloud-orb-move"), orb, morph);
            if orb && let Some(dict) = stage.dict {
                shape.orb_target = Some(orb_rect(room, dict, stage.toasts));
            }
            if !orb && orb_k <= 0.0 {
                shape.orb_target = None;
            }
            if obstructed
                && stage.parking == mareader_settings::EdgeParking::Off
                && let Some(dict) = stage.dict
            {
                let above = dict.top() - 8.0 - rect.height();
                rect = rect.translate(egui::vec2(
                    0.0,
                    (above - rect.top()).max(room.top() - rect.top()),
                ));
            }
            let park_stage = room;
            if shape.parked.is_none() && parking {
                shape.parked = Some(crate::park::Tuck::new(
                    crate::park::Edge::Bottom,
                    rect,
                    rect,
                    park_stage,
                ));
            }
            let tucked = crate::park::factor(
                ctx,
                Id::new("read-aloud-park"),
                parking,
                shape.parked.is_some(),
            );
            if let Some(mut tuck) = shape.parked {
                if parking {
                    tuck.revealing = false;
                }
                rect = tuck.rect(tucked);
                if !parking && tucked <= 0.0 {
                    rect = tuck.resume;
                    shape.parked = None;
                } else {
                    shape.parked = Some(tuck);
                }
            }
            let active =
                parking && tucked >= 1.0 && shape.parked.is_some_and(|tuck| !tuck.revealing);
            let lift = crate::park::magnetic_offset(
                ctx,
                Id::new("read-aloud-park-magnet"),
                shape.parked,
                rect,
                park_stage,
                active,
            );
            rect = rect.translate(lift);
            shape.rect = Some(rect);
            let parked = shape.parked.is_some();
            let tucked_rest = parked && parking && tucked >= 1.0;
            shape.rise = if parked {
                0.0
            } else {
                pane.map_or(0.0, |p| (p.bottom() - rect.top()).max(0.0))
            };
            let escape = view.escape.is_some();
            let fit = Fit::new(width, escape).fill(width, escape);
            let least = if f >= 0.5 {
                card::FULL_LEAST
            } else {
                narrowest
            };
            let span = (width, least.min(most), widest.min(most));
            let mut resize = None;
            let mut act = None;
            if shape.orb_target.is_none() {
                egui::Area::new(Id::new("read-aloud-bar"))
                    .fade_in(false)
                    .order(Order::Foreground)
                    .fixed_pos(rect.min)
                    .constrain(false)
                    .interactable(!parked)
                    .show(ctx, |ui| {
                        ui.set_opacity(shown);
                        ui.set_clip_rect(if parked { park_stage } else { room });
                        let (r, _) = ui.allocate_exact_size(rect.size(), egui::Sense::hover());
                        card::frame(ui, r, k);
                        if tucked_rest {
                            return;
                        }
                        if parked {
                            ui.disable();
                        }
                        let row = r.with_min_y(r.bottom() - control::HEIGHT);
                        // Words wait for room; compact and full cross-fade.
                        let words = ((k - 0.35) / 0.65).clamp(0.0, 1.0);
                        let top = r.with_max_y(row.top());
                        let both = (&about, lyrics.as_ref());
                        let light = |w: f32| ease_out(f64::from(w)) as f32;
                        let small = words * (1.0 - f).powi(2);
                        if small > 0.01 {
                            let at = (top, light(small), f < 0.5);
                            if card::compact(ui, at, both, &mut shape.ticker) {
                                act = Some(Act::Reveal);
                            }
                        }
                        let large = words * f * f;
                        if large > 0.01 {
                            let at = (top, light(large), f >= 0.5);
                            if card::show(ui, at, both, &mut shape.scroll, cover) {
                                act = Some(Act::Reveal);
                            }
                        }
                        if let Some(a) =
                            control::show(ui, row, (&view, fit, k), settings, stage.book, out)
                        {
                            act = Some(a);
                        }
                        resize = edges::show(ui, r, span);
                    });
            } else if let Some(target) = shape.orb_target {
                let k = ease_in_out(orb_k);
                let orb_rect =
                    Rect::from_min_max(rect.min.lerp(target.min, k), rect.max.lerp(target.max, k));
                shape.rect = Some(orb_rect);
                if show_orb(ctx, orb_rect, &view, lyrics.as_ref()) {
                    act = Some(Act::Toggle);
                }
            }
            if tucked_rest
                && crate::park::reveal(
                    ctx,
                    Id::new("read-aloud-edge-reveal"),
                    rect,
                    park_stage,
                    "Show the read-aloud player",
                )
                && let Some(mut tuck) = shape.parked
            {
                tuck.revealing = true;
                shape.parked = Some(tuck);
                shape.unparked_until = None;
                shape.player_priority = true;
            }
            // Asked about a part, the question rises over the player.
            if view.ask.is_some() {
                if shape.asked != view.ask {
                    shape.remember_skip = false;
                }
                shape.asked = view.ask;
            }
            let ease = motion::secs(ctx, 0.18);
            let asking =
                ctx.animate_bool_with_time(Id::new("read-aloud-asking"), view.ask.is_some(), ease);
            if !parked && let (Some(part), true) = (shape.asked, asking > 0.0) {
                let k = ease_out(f64::from(asking)) as f32 * shown;
                let can_remember = stage.book.is_some_and(|book| {
                    settings.book_skips.contains_key(&book) || settings.book_skips.len() < 1024
                });
                let answer = ask::show(
                    ctx,
                    (rect, room),
                    part,
                    cost.as_deref(),
                    k,
                    can_remember.then_some(&mut shape.remember_skip),
                );
                if let (Some(read), true) = (answer, view.ask.is_some()) {
                    if !read
                        && shape.remember_skip
                        && let Some(book) = stage.book
                    {
                        remember_skip(settings, book, part);
                    }
                    shape.remember_skip = false;
                    act = Some(Act::Answer(read));
                }
            }
            if matches!(act, Some(Act::Card(_))) || resize.is_some() {
                shape.player_priority = true;
                shape.unparked_until = None;
            }
            apply_resize(shape, resize, stage.toasts, rect, settings, out);
            act
        });
        match act {
            Some(Act::Card(to)) => {
                shape.chosen = Some(to);
                None
            }
            Some(Act::Reveal) => {
                out.push(Sent::Ui(UiCommand::ShowReading));
                None
            }
            other => other,
        }
    }
}

fn apply_resize(
    shape: &mut Shape,
    resize: Option<Resize>,
    toasts: Option<Rect>,
    rect: Rect,
    settings: &mut ReadAloudSettings,
    out: &mut Commands,
) {
    match resize {
        Some(Resize::To(w)) => {
            shape.dragged = Some(w);
            if toasts.is_some_and(|t| rect.intersects(t)) {
                out.push(Sent::Ui(UiCommand::ClearToasts));
            }
        }
        Some(Resize::Set(w)) => {
            (shape.dragged, settings.player_width) = (None, Some(w));
            if toasts.is_some_and(|t| rect.intersects(t)) {
                out.push(Sent::Ui(UiCommand::ClearToasts));
            }
        }
        Some(Resize::Reset) => {
            (shape.dragged, settings.player_width) = (None, None);
        }
        None => {}
    }
}

fn orb_rect(room: Rect, obstacle: Rect, toast: Option<Rect>) -> Rect {
    let size = egui::vec2(
        room.width().clamp(0.0, 160.0),
        room.height().clamp(0.0, 44.0),
    );
    let left = (room.left() + 12.0).min((room.right() - size.x).max(room.left()));
    let right = (room.right() - size.x - 12.0).max(room.left());
    let y = (room.bottom() - size.y - 12.0).max(room.top());
    let first = if obstacle.center().x > room.center().x {
        left
    } else {
        right
    };
    let second = if first == left { right } else { left };
    let above =
        (obstacle.top() - size.y - 8.0).clamp(room.top(), (room.bottom() - size.y).max(room.top()));
    let candidates = [
        Rect::from_min_size(egui::pos2(first, y), size),
        Rect::from_min_size(egui::pos2(second, y), size),
        Rect::from_min_size(egui::pos2(first, above), size),
        Rect::from_min_size(egui::pos2(second, above), size),
    ];
    candidates
        .into_iter()
        .min_by(|a, b| {
            let overlap = |r: Rect| {
                let area = |other| {
                    let hit = r.intersect(other);
                    hit.width().max(0.0) * hit.height().max(0.0)
                };
                area(obstacle) + toast.map_or(0.0, area)
            };
            overlap(*a).total_cmp(&overlap(*b))
        })
        .unwrap_or(Rect::from_min_size(room.min, size))
}

fn fit_orb_text(lyrics: &Lyrics, mut fits: impl FnMut(&str) -> bool) -> &str {
    let get = |range: &std::ops::Range<usize>| {
        let first = lyrics.words.get(range.start)?;
        let last = lyrics.words.get(range.end.checked_sub(1)?)?;
        if range.start >= range.end {
            return None;
        }
        lyrics.now.get(first.start..last.end)
    };
    let word = lyrics.heard..lyrics.heard.saturating_add(1);
    for range in [&lyrics.preferred, &lyrics.lit, &word] {
        if let Some(text) = get(range)
            && fits(text)
        {
            return text;
        }
    }
    get(&word).unwrap_or(lyrics.now.as_str())
}

fn show_orb(ctx: &Context, rect: Rect, view: &View<'_>, lyrics: Option<&Lyrics>) -> bool {
    egui::Area::new(Id::new("read-aloud-orb"))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .movable(false)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect.expand(12.0));
            card::frame(ui, rect, 0.0);
            ui.set_clip_rect(rect);
            let slot = Rect::from_min_size(
                rect.min + egui::vec2(4.0, 0.0),
                egui::vec2(40.0, rect.height()),
            );
            let t = Tokens::get(ctx);
            if let Some(progress) = view.at.filter(|p| p.is_finite()) {
                let count = (progress.clamp(0.0, 1.0) * 48.0) as usize;
                let points: Vec<_> = (0..=count)
                    .map(|i| {
                        let a =
                            (i as f32 / 48.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
                        slot.center() + egui::vec2(a.cos(), a.sin()) * 20.0
                    })
                    .collect();
                if points.len() > 1 {
                    ui.painter()
                        .add(egui::Shape::line(points, egui::Stroke::new(2.0, t.accent)));
                }
            }
            let available = (rect.width() - 58.0).max(0.0);
            let font = egui::FontId::proportional(12.0);
            let text = lyrics
                .map(|lyrics| {
                    fit_orb_text(lyrics, |text| {
                        ui.painter()
                            .layout_no_wrap(text.to_owned(), font.clone(), t.muted)
                            .size()
                            .x
                            <= available
                    })
                })
                .filter(|text| !text.is_empty())
                .unwrap_or("Read aloud");
            let text_area = Rect::from_min_max(
                egui::pos2((rect.left() + 50.0).min(rect.right()), rect.top()),
                rect.max,
            );
            ui.painter().with_clip_rect(text_area.intersect(rect)).text(
                egui::pos2(text_area.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                text,
                font,
                t.muted,
            );
            let label = ui.interact(
                text_area.intersect(rect),
                Id::new("read-aloud-orb-words"),
                egui::Sense::hover(),
            );
            label.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
            control::play_button(ui, slot, view)
        })
        .inner
}

/// Slow, quick, then slow: for things changing shape.
fn ease_in_out(k: f32) -> f32 {
    let k = k.clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

/// Fast at first, then settling: for things arriving.
fn ease_out(k: f64) -> f64 {
    1.0 - (1.0 - k).powi(3)
}

/// The speaker on a word's card: says it, or stops it; accented meanwhile.
pub fn say_button(
    ui: &mut Ui,
    word: &str,
    (saying, voice): (bool, Option<&str>),
    out: &mut Commands,
) {
    // No voice here speaks Burmese and its kin.
    if crate::shaped::needs_shaping(word) {
        return;
    }
    let (label, command) = match saying {
        true => (STOP_SAYING, Sent::Engine(Command::StopSaying)),
        false => (SAY, say(word, voice)),
    };
    let accent = Tokens::get(ui.ctx()).accent;
    let r = ui.scope(|ui| {
        if saying {
            ui.visuals_mut().override_text_color = Some(accent);
        }
        widgets::small_icon_button(ui, Icon::Speak, label)
    });
    if r.inner.clicked() {
        out.push(command);
    }
}

/// Says `word`, in `voice` when it isn't the one picked.
pub fn say(word: &str, voice: Option<&str>) -> Sent {
    let word = word.to_owned();
    match voice {
        Some(voice) => Sent::Engine(Command::SayWordWith {
            word,
            voice: voice.to_owned(),
        }),
        None => Sent::Engine(Command::SayWord(word)),
    }
}

pub(crate) fn rate_label(rate: f32) -> String {
    let text = format!("{rate:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{text}×")
}

/// The words read on the pages shown, in the accent's tint.
pub fn paint(ui: &Ui, pages: &Pages<'_>, lit: &Lit, t: &Tokens) {
    let fill = fill(t);
    let painter = ui.painter();
    for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let mut runs = Vec::new();
        match lit.span {
            Span::Anchors { start, end } => {
                let inside = |at: Option<(u32, u32)>| {
                    at.is_some_and(|(b, i)| {
                        let at = Anchor {
                            block: b as usize,
                            offset: i as usize,
                        };
                        start <= at && at < end
                    })
                };
                let mut from = None;
                let count = layer.places.len();
                for i in 0..=count {
                    let on = i < count && inside(layer.places.get(i).copied().flatten());
                    match (on, from) {
                        (true, None) => from = Some(i),
                        (false, Some(f)) => {
                            runs.push(f..i);
                            from = None;
                        }
                        _ => {}
                    }
                }
            }
            Span::Chars { start, end } => {
                let page = sheet.page;
                if (start.page..=end.page).contains(&page) {
                    let from = if page == start.page { start.char } else { 0 };
                    let to = if page == end.page {
                        end.char
                    } else {
                        layer.text.len()
                    };
                    runs.push(from..to.min(layer.text.len()));
                }
            }
        }
        for run in runs.into_iter().filter(|r| !r.is_empty()) {
            for b in layer.text.line_boxes(run) {
                let r = sheet.to_screen(b).intersect(sheet.shown);
                if r.is_positive() {
                    painter.rect_filled(r, CornerRadius::same(2), fill);
                }
            }
        }
    }
}

/// The accent toned to the paper; a narrated book's own colour could hide
/// the ink.
fn fill(t: &Tokens) -> Color32 {
    let paper = t.book.paper.map(|c| c.clamp(0.0, 1.0) * 255.0);
    tint(paper, [t.accent.r(), t.accent.g(), t.accent.b()])
}

/// A faint share of `colour` over `paper`, fainter on a dark page.
fn tint(paper: [f32; 3], colour: [u8; 3]) -> Color32 {
    let light = paper.iter().sum::<f32>() > 3.0 * 128.0;
    let share = if light { 0.30 } else { 0.26 };
    let target = std::array::from_fn(|i| {
        let p = paper.get(i).copied().unwrap_or(255.0);
        p + (f32::from(colour.get(i).copied().unwrap_or(255)) - p) * share
    });
    over(paper, target)
}

/// The darkest fill that turns `paper` into `target`, so ink stays dark.
fn over(paper: [f32; 3], target: [f32; 3]) -> Color32 {
    let pairs = || paper.into_iter().zip(target);
    let darker = |(p, t): (f32, f32)| (p - t).max(0.0) / p.max(1.0);
    let lighter = |(p, t): (f32, f32)| (t - p).max(0.0) / (255.0 - p).max(1.0);
    // Darkening sets the strength; lightening only when nothing darkens.
    let dark = pairs().map(darker).fold(0.0_f32, f32::max);
    let need = match dark > 0.01 {
        true => dark,
        false => pairs().map(lighter).fold(0.0_f32, f32::max),
    };
    let alpha = need.clamp(0.12, 1.0);
    let channel = |(p, t): (f32, f32)| ((t - (1.0 - alpha) * p) / alpha).clamp(0.0, 255.0);
    let [r, g, b] = {
        let mut out = [0u8; 3];
        for (slot, pair) in out.iter_mut().zip(pairs()) {
            *slot = channel(pair).round() as u8;
        }
        out
    };
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

/// A pane's book, as the reader below the front end sees it.
impl Reading {
    /// Where it stands: its pane, its book, and the page showing.
    pub fn stand(&self) -> Stand {
        Stand {
            pane: self.pane,
            doc: self.doc,
            document: Arc::clone(&self.document),
            current: self.current,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orb_text_uses_the_largest_fitting_group_and_tracks_the_heard_word() {
        let mut lyrics = Lyrics {
            before: None,
            now: "one two three four".to_owned(),
            words: vec![0..3, 4..7, 8..13, 14..18],
            lit: 1..4,
            preferred: 0..4,
            heard: 2,
            after: None,
            key: 1,
        };
        assert_eq!(
            fit_orb_text(&lyrics, |s| s.len() <= 20),
            "one two three four"
        );
        assert_eq!(fit_orb_text(&lyrics, |s| s.len() <= 14), "two three four");
        assert_eq!(fit_orb_text(&lyrics, |s| s.len() <= 5), "three");
        lyrics.heard = 3;
        assert_eq!(fit_orb_text(&lyrics, |s| s.len() <= 5), "four");
        assert_eq!(fit_orb_text(&lyrics, |_| false), "four");
        lyrics.heard = usize::MAX;
        lyrics.preferred = lyrics.words.len()..1;
        lyrics.lit = 9..10;
        assert_eq!(fit_orb_text(&lyrics, |_| true), "one two three four");
    }

    #[test]
    fn settings_and_speech_name_the_system_voice_alike() {
        // Settings can't see speech, so each spells the id itself.
        assert_eq!(ReadAloudSettings::SYSTEM, mareader_speech::SYSTEM);
    }

    /// What `fill` over `under` shows, as egui blends it.
    fn shown(fill: Color32, under: f32) -> [f32; 3] {
        let a = f32::from(fill.a()) / 255.0;
        let c = [fill.r(), fill.g(), fill.b()];
        c.map(|c| f32::from(c) + under * (1.0 - a))
    }

    #[test]
    fn the_tint_on_a_tinted_page_leaves_the_ink_readable() {
        // A pink page: a book's pure yellow once covered the words here.
        let pink = [255.0, 214.0, 232.0];
        let fill = tint(pink, [0x25, 0x63, 0xeb]);
        assert!(fill.a() <= 80, "{fill:?}");
        let ink = shown(fill, 31.0);
        assert!(ink.iter().all(|c| *c < 110.0), "{ink:?}");
    }

    #[test]
    fn the_fill_turns_paper_to_its_target_and_keeps_ink_dark() {
        let fill = over([250.0; 3], [255.0, 255.0, 187.0]);
        let paper = shown(fill, 250.0);
        assert!(
            (paper[2] - 187.0).abs() < 2.0 && paper[0] > 248.0,
            "{paper:?}"
        );
        let ink = shown(fill, 0.0);
        assert!(ink.iter().all(|c| *c < 90.0), "{ink:?}");
        // On a dark page the tint lightens, and is faint.
        let fill = over([30.0; 3], [60.0, 60.0, 40.0]);
        assert!(fill.a() < 64, "{fill:?}");
        assert!((shown(fill, 30.0)[0] - 60.0).abs() < 2.0);
    }

    #[test]
    fn speeds_read_short() {
        assert_eq!(rate_label(1.0), "1×");
        assert_eq!(rate_label(1.25), "1.25×");
        assert_eq!(rate_label(0.75), "0.75×");
        assert_eq!(rate_label(1.5), "1.5×");
    }
}
