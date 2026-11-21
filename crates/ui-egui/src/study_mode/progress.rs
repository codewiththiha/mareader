//! The corner toast while the AI writes a studying pane's cards.
//!
//! Folded, it is a ring in the pane's bottom left corner, rising over the
//! page bar while that is up: spinning while the new words are found,
//! filling as cards are written, red once writing failed.
//! A hover widens it to say so in words. Dictionary cards come at once, so
//! they never show it; a deck finished shows *Next cards ready* a moment.

use egui::{Align2, Color32, Context, Id, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use mareader_engine::study_mode::{Ahead, DeckJob, DeckState, Doing, Run};
use mareader_study::mode::Via;

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
/// Seconds to widen and to fold.
const OPEN: f32 = 0.2;

/// Where the AI's work on a pane's cards stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Finding,
    Writing { done: usize, of: usize },
    Failed,
    Ready,
}

impl Status {
    /// The words the widened toast says, `range` naming the pages.
    pub fn said(self, range: &str) -> String {
        match self {
            Self::Finding => format!("Finding new words · {range}"),
            Self::Writing { done, of } => format!("Writing cards {done} of {of} · {range}"),
            Self::Failed => "Couldn’t write cards · open the deck to retry".to_owned(),
            Self::Ready => "Next cards ready".to_owned(),
        }
    }

    pub fn busy(self) -> bool {
        matches!(self, Self::Finding | Self::Writing { .. })
    }
}

fn of_job(job: &DeckJob) -> Option<Status> {
    if job.via() != Via::Ai {
        return None;
    }
    Some(match (job.doing(), job.progress()) {
        (Doing::Writing, Some((done, of))) => Status::Writing { done, of },
        _ => Status::Finding,
    })
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

/// The toast at `pane`'s bottom left, saying `status`.
pub fn show(ctx: &Context, pane: &Pane<'_>, (range, status): Work, alpha: f32) {
    let t = Tokens::get(ctx);
    let id = Id::new(("study-progress", pane.id));
    let said = status.said(&range);
    let font = egui::FontId::proportional(t.text_small);
    let galley = ctx.fonts_mut(|f| f.layout_no_wrap(said.clone(), font, t.ink));
    let hovered = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    let open = motion::slide(ctx, id.with("open"), f32::from(u8::from(hovered)), OPEN);
    let full = SIDE + galley.rect.width() + 22.0;
    let room = (pane.rect.width() - 2.0 * INSET).max(SIDE);
    let width = (SIDE + (full - SIDE) * open).min(room);
    let lift = match pane.bar {
        true => crate::page_bar::BAR_H + LIFT,
        false => INSET,
    };
    let lift = ctx.animate_value_with_time(id.with("lift"), lift, motion::secs(ctx, OPEN));
    let bottom = pane.rect.bottom() - lift;
    let at = pos2(pane.rect.left() + INSET, bottom);
    let shown = z_order::area(ctx, Tier::PageCard, id)
        .pivot(Align2::LEFT_BOTTOM)
        .fixed_pos(at)
        .constrain(false)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, SIDE), Sense::hover());
            paint(ui, rect, status, &t, alpha);
            if open > 0.0 {
                let ink = t.ink.gamma_multiply(open * alpha);
                let text = pos2(rect.left() + SIDE + 4.0, rect.center().y);
                let painter = ui.painter_at(rect.shrink(1.0));
                let top = text - vec2(0.0, galley.rect.height() / 2.0);
                painter.galley_with_override_text_color(top, galley, ink);
            }
            let label = if hovered { said.as_str() } else { PROGRESS };
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, label));
            resp.hovered()
        });
    ctx.data_mut(|d| d.insert_temp(id, shown.inner));
    if status.busy() || (open > 0.0 && open < 1.0) || alpha < 1.0 {
        ctx.request_repaint();
    }
    crate::toast::keep_clear(ctx, shown.response.rect);
}

/// The toast's paper and its ring.
fn paint(ui: &Ui, rect: Rect, status: Status, t: &Tokens, alpha: f32) {
    let painter = ui.painter();
    let fade = |c: Color32| c.gamma_multiply(alpha);
    let shadow = egui::Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: fade(t.shadow),
    };
    painter.add(shadow.as_shape(rect, SIDE / 2.0));
    painter.rect_filled(rect, SIDE / 2.0, fade(t.surface));
    let edge = Stroke::new(1.0, fade(t.line));
    painter.rect_stroke(rect, SIDE / 2.0, edge, egui::StrokeKind::Inside);
    let c = pos2(rect.left() + SIDE / 2.0, rect.center().y);
    let r = 8.0;
    if status == Status::Failed {
        painter.circle_filled(c, 5.0, fade(t.danger));
        return;
    }
    painter.circle_stroke(c, r, Stroke::new(2.5, fade(t.track)));
    let now = ui.input(|i| i.time) as f32;
    let (from, span) = match status {
        Status::Writing { done, of } if done > 0 && of > 0 => {
            (0.0, done as f32 / of as f32 * std::f32::consts::TAU)
        }
        Status::Ready => (0.0, std::f32::consts::TAU),
        _ => (now * 5.0, 1.6),
    };
    arc(ui, c, r, (from, span), Stroke::new(2.5, fade(t.accent)));
    if status == Status::Ready {
        let (a, b) = (c + vec2(-3.5, 0.0), c + vec2(-1.0, 2.5));
        let tick = vec![a, b, c + vec2(3.5, -2.5)];
        painter.line(tick, Stroke::new(2.0, fade(t.accent)));
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
        let writing = Status::Writing { done: 1, of: 4 };
        let work = Some(("pages 1–2".to_owned(), writing));
        assert_eq!(watch.status(work.clone(), 0.0), work);
        let ready = watch.status(None, 1.0).map(|(_, s)| s);
        assert_eq!(ready, Some(Status::Ready));
        assert_eq!(watch.alpha(1.5), 1.0);
        assert!(watch.alpha(3.9) < 0.5);
        assert_eq!(watch.status(None, 4.5), None);
    }

    #[test]
    fn nothing_shows_with_no_work_started() {
        let mut watch = Watch::default();
        assert_eq!(watch.status(None, 0.0), None);
    }

    #[test]
    fn the_toast_says_where_the_writing_stands() {
        let writing = Status::Writing { done: 3, of: 8 };
        assert_eq!(
            writing.said("pages 4–6"),
            "Writing cards 3 of 8 · pages 4–6"
        );
        assert!(Status::Finding.busy());
        assert!(!Status::Failed.busy());
    }
}
