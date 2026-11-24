//! The corner toast while the AI writes a studying pane's cards.
//!
//! Folded, it is a ring in the pane's bottom left corner, rising over the
//! page bar while that is up. It fills as far as the deck is made, from
//! [`DeckJob::made`], easing to each step, with a soft pulse at its head
//! while the work goes on; red once writing failed. Where the pane's
//! deck leaves no room there, it moves to the top left corner, and smaller
//! when only that fits, gliding between them.
//! A hover widens it to say so in words. Dictionary cards come at once, so
//! they never show it; a deck finished shows *Next cards ready* a moment.

use egui::{Align2, Color32, Context, Id, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use mareader_engine::study_mode::{Ahead, DeckJob, DeckState, Doing, Run};
use mareader_study::mode::Via;
use mareader_workspace::PaneId;

use super::Pane;
use crate::motion;
use crate::theme::Tokens;
use crate::z_order::{self, Tier};

/// The folded toast's name, for screen readers and tests.
pub const PROGRESS: &str = "Card progress";
/// Seconds *Next cards ready* stays, and the last of them fading.
pub const READY_SECS: f64 = 3.0;
const FADE: f64 = 0.4;
/// The folded toast's side; its room from the pane's edges.
const SIDE: f32 = 30.0;
const INSET: f32 = 14.0;
const LIFT: f32 = 12.0;
/// The small toast's side and room, where the deck leaves little.
const SMALL: f32 = 18.0;
const TIGHT: f32 = 4.0;
/// The least room kept between the toast and the deck.
const GAP: f32 = 2.0;
/// Seconds to widen and to fold.
const OPEN: f32 = 0.2;
/// Seconds the fill eases to a new step; one pulse at its head.
const EASE: f32 = 0.45;
const BREATH: f64 = 1.6;

/// Where the AI's work on a pane's cards stands; `made` is the share of
/// the deck made, 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Finding {
        made: f32,
    },
    /// Waiting in line for its model, with the words found.
    Queued {
        made: f32,
    },
    Writing {
        done: usize,
        of: usize,
        made: f32,
    },
    Failed,
    Ready,
}

impl Status {
    /// The words the widened toast says, `range` naming the pages.
    pub fn said(self, range: &str) -> String {
        match self {
            Self::Finding { .. } => format!("Finding new words · {range}"),
            Self::Queued { .. } => format!("Waiting for the model · {range}"),
            Self::Writing { done, of, .. } => format!("Writing cards {done} of {of} · {range}"),
            Self::Failed => "Couldn’t write cards · open the deck to retry".to_owned(),
            Self::Ready => "Next cards ready".to_owned(),
        }
    }

    pub fn busy(self) -> bool {
        self.made().is_some()
    }

    /// The share of the deck made, while it is being made.
    fn made(self) -> Option<f32> {
        match self {
            Self::Finding { made } | Self::Queued { made } => Some(made),
            Self::Writing { made, .. } => Some(made),
            Self::Failed | Self::Ready => None,
        }
    }
}

fn of_job(job: &DeckJob) -> Option<Status> {
    if job.via() != Via::Ai {
        return None;
    }
    let made = job.made();
    Some(match (job.doing(), job.progress()) {
        _ if job.queued() => Status::Queued { made },
        (Doing::Writing, Some((done, of))) => Status::Writing { done, of, made },
        _ => Status::Finding { made },
    })
}

/// `made`, eased from where `id` last stood, so each step glides.
pub fn eased(ctx: &Context, id: Id, made: f32) -> f32 {
    ctx.animate_value_with_time(id, made.clamp(0.0, 1.0), motion::secs(ctx, EASE))
}

/// A slow breath, 0 to 1, saying work goes on; still where motion is reduced.
pub fn pulse(ctx: &Context) -> f32 {
    if motion::reduced(ctx) || !motion::enabled(ctx) {
        return 0.6;
    }
    ctx.request_repaint();
    let now = ctx.input(|i| i.time);
    (0.5 - 0.5 * (now * std::f64::consts::TAU / BREATH).cos()) as f32
}

/// Work on cards: the pages, and where it stands.
pub type Work = (String, Status);

/// The AI's work on `run`'s cards, nearest batch first, with its pages.
pub fn work(run: &Run) -> Option<Work> {
    match &run.deck {
        DeckState::Making { batch, job } => {
            if let Some(status) = of_job(job) {
                return Some((batch.label(), status));
            }
        }
        DeckState::Failed { batch, .. } => return Some((batch.label(), Status::Failed)),
        DeckState::None | DeckState::Open(_) => {}
    }
    run.ahead.iter().find_map(|ahead| match ahead {
        Ahead::Making { batch, job } => of_job(job).map(|s| (batch.label(), s)),
        Ahead::Failed { batch, .. } => Some((batch.label(), Status::Failed)),
        Ahead::Ready { .. } => None,
    })
}

/// One pane's toast across frames: whether it was busy, and since when done.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Watch {
    busy: bool,
    ready: Option<f64>,
}

impl Watch {
    /// What to show at `now`: the work going on, or a moment of ready.
    pub fn status(&mut self, work: Option<Work>, now: f64) -> Option<Work> {
        let busy = work.as_ref().is_some_and(|(_, s)| s.busy());
        if self.busy && work.is_none() {
            self.ready = Some(now);
        }
        if work.is_some() {
            self.ready = None;
        }
        self.busy = busy;
        work.or_else(|| {
            let since = self.ready?;
            (now - since < READY_SECS).then(|| (String::new(), Status::Ready))
        })
    }

    /// The toast's opacity: whole, fading in ready's last moment.
    pub fn alpha(&self, now: f64) -> f32 {
        self.ready.map_or(1.0, |since| {
            let left = READY_SECS - (now - since);
            (left / FADE).clamp(0.0, 1.0) as f32
        })
    }
}

/// Marks `rect` as `pane`'s deck this pass; the toast keeps clear of it.
pub fn keep_clear(ctx: &Context, pane: PaneId, rect: Rect) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(taken_id(pane), (pass, rect)));
}

/// Where `pane`'s deck drew this pass or the last; none once it is gone.
fn taken(ctx: &Context, pane: PaneId) -> Option<Rect> {
    let pass = ctx.cumulative_pass_nr();
    let kept: Option<(u64, Rect)> = ctx.data(|d| d.get_temp(taken_id(pane)));
    kept.filter(|(at, _)| at + 1 >= pass).map(|(_, rect)| rect)
}

fn taken_id(pane: PaneId) -> Id {
    Id::new(("study-progress-taken", pane))
}

/// The folded toast in `pane`: the bottom left corner, risen over the page
/// bar while `bar`, else the top left; small where only that is clear of
/// `taken`, and small in the top left when nothing is.
fn spot(pane: Rect, bar: bool, taken: Option<Rect>) -> Rect {
    let corner = |(side, inset): (f32, f32), top: bool| {
        let lift = if bar {
            crate::page_bar::BAR_H + LIFT
        } else {
            inset
        };
        let y = match top {
            true => pane.top() + inset,
            false => pane.bottom() - lift - side,
        };
        Rect::from_min_size(pos2(pane.left() + inset, y), Vec2::splat(side))
    };
    let free = |r: &Rect| taken.is_none_or(|t| !t.intersects(r.expand(GAP)));
    let both = |size| [corner(size, false), corner(size, true)];
    let mut tries = [(SIDE, INSET), (SMALL, TIGHT)].into_iter().flat_map(both);
    let fallback = corner((SMALL, TIGHT), true);
    tries.find(free).unwrap_or(fallback)
}

/// `to`, eased there from where the toast last stood.
fn glide(ctx: &Context, id: Id, to: Rect) -> Rect {
    let secs = motion::secs(ctx, OPEN);
    let ease = |key: &str, to: f32| ctx.animate_value_with_time(id.with(key), to, secs);
    let min = pos2(ease("x", to.left()), ease("y", to.top()));
    Rect::from_min_size(min, Vec2::splat(ease("side", to.height())))
}

/// The toast in `pane`'s corner, saying `status`.
pub fn show(ctx: &Context, pane: &Pane<'_>, (range, status): Work, alpha: f32) {
    let t = Tokens::get(ctx);
    let id = Id::new(("study-progress", pane.id));
    let said = status.said(&range);
    let font = egui::FontId::proportional(t.text_small);
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(said.clone(), font, t.ink));
    let hovered = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let open = motion::slide(ctx, id.with("open"), f32::from(u8::from(hovered)), OPEN);
    let folded = glide(ctx, id, spot(pane.rect, pane.bar, taken(ctx, pane.id)));
    let side = folded.height();
    let full = side + galley.rect.width() + 22.0;
    let room = (pane.rect.right() - folded.left() - INSET).max(side);
    let width = (side + (full - side) * open).min(room);
    let shown = z_order::area(ctx, Tier::PageCard, id)
        .pivot(Align2::LEFT_TOP)
        .fixed_pos(folded.min)
        .constrain(false)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, side), Sense::hover());
            let made = status.made().map(|m| eased(ctx, id.with("made"), m));
            paint(ui, rect, (status, made), &t, alpha);
            if open > 0.0 {
                let ink = t.ink.gamma_multiply(open * alpha);
                let text = pos2(rect.left() + side + 4.0, rect.center().y);
                let painter = ui.painter_at(rect.shrink(1.0));
                let top = text - vec2(0.0, galley.rect.height() / 2.0);
                painter.galley_with_override_text_color(top, galley, ink);
            }
            let label = if hovered { said.as_str() } else { PROGRESS };
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
            resp.hovered()
        });
    ctx.data_mut(|d| d.insert_temp(id, shown.inner));
    if (open > 0.0 && open < 1.0) || alpha < 1.0 {
        ctx.request_repaint();
    }
    // News wakes the frame; this only keeps a quiet step from stalling.
    if status.busy() {
        ctx.request_repaint_after_secs(0.25);
    }
    crate::toast::keep_clear(ctx, shown.response.rect);
}

/// The toast's paper and its ring, sized to its height.
fn paint(ui: &Ui, rect: Rect, (status, made): (Status, Option<f32>), t: &Tokens, alpha: f32) {
    let painter = ui.painter();
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let side = rect.height();
    let k = side / SIDE;
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: fade(t.shadow),
    };
    painter.add(shadow.as_shape(rect, side / 2.0));
    painter.rect_filled(rect, side / 2.0, fade(t.surface));
    let edge = Stroke::new(1.0, fade(t.line));
    painter.rect_stroke(rect, side / 2.0, edge, egui::StrokeKind::Inside);
    let c = pos2(rect.left() + side / 2.0, rect.center().y);
    let (r, line) = (8.0 * k, Stroke::new(2.5 * k, fade(t.accent)));
    if status == Status::Failed {
        painter.circle_filled(c, 5.0 * k, fade(t.danger));
        return;
    }
    painter.circle_stroke(c, r, Stroke::new(line.width, fade(t.track)));
    let span = made.unwrap_or(1.0) * std::f32::consts::TAU;
    arc(ui, c, r, (0.0, span), line);
    if made.is_some() {
        let head = c + r * Vec2::angled(span - std::f32::consts::FRAC_PI_2);
        let glow = fade(t.accent).gamma_multiply(0.35 + 0.65 * pulse(ui.ctx()));
        painter.circle_filled(head, line.width * 0.9, glow);
    }
    if status == Status::Ready {
        let marks = [vec2(-3.5, 0.0), vec2(-1.0, 2.5), vec2(3.5, -2.5)];
        let tick = marks.map(|v| c + v * k);
        painter.line(tick.to_vec(), Stroke::new(2.0 * k, fade(t.accent)));
    }
}

/// An arc of `span` radians from `from`, starting at the top.
fn arc(ui: &Ui, c: egui::Pos2, r: f32, (from, span): (f32, f32), stroke: Stroke) {
    let steps = 40;
    let at = |i: usize| {
        let a = from - std::f32::consts::FRAC_PI_2 + span * i as f32 / steps as f32;
        c + r * Vec2::angled(a)
    };
    let points: Vec<_> = (0..=steps).map(at).collect();
    ui.painter().add(egui::Shape::line(points, stroke));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deck_done_says_ready_a_moment_then_goes() {
        let mut watch = Watch::default();
        let writing = Status::Writing {
            done: 1,
            of: 4,
            made: 0.3,
        };
        let work = Some(("pages 1–2".to_owned(), writing));
        assert_eq!(watch.status(work.clone(), 0.0), work);
        let ready = watch.status(None, 1.0).map(|(_, s)| s);
        assert_eq!(ready, Some(Status::Ready));
        assert_eq!(watch.alpha(1.5), 1.0);
        assert!(watch.alpha(3.9) < 0.5);
        assert_eq!(watch.status(None, 4.5), None);
    }

    #[test]
    fn the_toast_moves_and_shrinks_out_of_the_decks_way() {
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 250.0));
        let corner = |taken: Option<Rect>| spot(pane, false, taken);
        let roomy = corner(None);
        assert_eq!(roomy.left_bottom(), pos2(14.0, 236.0));
        assert_eq!(roomy.height(), SIDE);
        let deck = |y: std::ops::RangeInclusive<f32>| Some(Rect::from_x_y_ranges(0.0..=200.0, y));
        let above = corner(deck(150.0..=220.0));
        assert_eq!(above.min, pos2(14.0, 14.0), "the top left");
        assert_eq!(above.height(), SIDE);
        let below = corner(deck(26.0..=225.0));
        assert_eq!(below.left_bottom(), pos2(TIGHT, 246.0), "small below");
        assert_eq!(below.height(), SMALL);
        let none = corner(deck(10.0..=240.0));
        assert_eq!(none.min, pos2(TIGHT, TIGHT), "small, top left");
    }

    #[test]
    fn nothing_shows_with_no_work_started() {
        let mut watch = Watch::default();
        assert_eq!(watch.status(None, 0.0), None);
    }

    #[test]
    fn the_toast_says_where_the_writing_stands() {
        let writing = Status::Writing {
            done: 3,
            of: 8,
            made: 0.44,
        };
        assert_eq!(
            writing.said("pages 4–6"),
            "Writing cards 3 of 8 · pages 4–6"
        );
        let queued = Status::Queued { made: 0.1 };
        assert_eq!(
            queued.said("pages 4–6"),
            "Waiting for the model · pages 4–6"
        );
        assert!(Status::Finding { made: 0.0 }.busy());
        assert!(!Status::Failed.busy());
    }
}
