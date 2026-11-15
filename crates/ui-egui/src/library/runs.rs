//! A corner card per long import or walk; quick runs only toast.

use std::f32::consts::TAU;

use egui::{
    Align, Align2, Area, Context, FontId, Frame, Id, Layout, Margin, Order, Pos2, Rect, RichText,
    Shape, Stroke, Ui, pos2, vec2,
};

use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::library::Action;
use mareader_engine::session::Session;

use mareader_engine::library::{DELAY, Phase, Run};

const CARD_W: f32 = 248.0;
/// Wider, for a stopped run's two buttons.
const PAUSED_W: f32 = 330.0;
const RING: f32 = 30.0;
const INSET: f32 = 16.0;

/// The cards, stacked up from `stage`'s bottom-left; `lift` clears a page bar.
pub fn show(
    ctx: &Context,
    session: &mut Session,
    at: (Rect, f32),
    paused: bool,
    out: &mut Commands,
) {
    let (stage, lift) = at;
    if session.library().runs.list().is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    let (due, waiting) = session.library_runs_due(now, paused);
    let runs = session.library().runs.list();
    let t = Tokens::get(ctx);
    let mut bottom = stage.bottom() - INSET - lift;
    for (run, ready) in runs.iter().zip(due) {
        if !ready {
            continue;
        }
        let pos = pos2(stage.left() + INSET, bottom);
        let area = Area::new(Id::new(("import-run", run.id)))
            .order(Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(pos)
            .interactable(run.stoppable() || run.paused());
        let shown = area.show(ctx, |ui| card(ui, run, now, &t, out));
        bottom = shown.response.rect.top() - 8.0;
    }
    // The ring turns, and a waiting card is due; paused cards stay still.
    let soon = std::time::Duration::from_secs_f64(DELAY / 2.0);
    let still = runs.iter().all(Run::paused);
    match waiting {
        true => ctx.request_repaint_after(soon),
        false if still => {}
        false => ctx.request_repaint(),
    }
}

fn card(ui: &mut Ui, run: &Run, now: f64, t: &Tokens, out: &mut Commands) {
    let frame = Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(t.radius_lg)
        .shadow(egui::epaint::Shadow {
            offset: [0, 4],
            blur: 16,
            spread: 0,
            color: t.shadow,
        })
        .inner_margin(Margin::symmetric(12, 10));
    frame.show(ui, |ui| {
        ui.set_width(if run.paused() { PAUSED_W } else { CARD_W });
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(RING, RING), egui::Sense::hover());
            ring(ui, rect.center(), run.fraction(), now, t);
            ui.add_space(4.0);
            let lines = |ui: &mut Ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let head = RichText::new(run.headline()).font(theme::medium(t.text_small));
                    ui.add(egui::Label::new(head.color(t.ink)).truncate());
                    let detail = RichText::new(run.detail()).size(t.text_small - 1.0);
                    ui.add(egui::Label::new(detail.color(t.muted)).truncate());
                });
            };
            if run.paused() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    paused_buttons(ui, run, out);
                    lines(ui);
                });
                return;
            }
            if !run.stoppable() {
                return lines(ui);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // What's done is kept; the paused card carries on.
                let stop = widgets::small_icon_button(ui, Icon::Close, "Pause organizing");
                if stop.clicked() {
                    out.push(Sent::Engine(Command::Library(Action::StopOrganizing)));
                }
                lines(ui);
            });
        });
    });
}

/// Let go, then carry on or review, right to left.
fn paused_buttons(ui: &mut Ui, run: &Run, out: &mut Commands) {
    let (tip, go, go_tip) = match run.phase {
        Phase::Ready => (
            "Discard the plan",
            "Review",
            "See the plan before anything moves",
        ),
        _ => ("Discard what's done", "Resume", "Carry on where it stopped"),
    };
    if widgets::small_icon_button(ui, Icon::Close, tip).clicked() {
        out.push(Sent::Engine(Command::Library(Action::DiscardPaused)));
    }
    if ui.button(go).on_hover_text(go_tip).clicked() {
        out.push(Sent::Engine(Command::Library(Action::ResumeOrganizing)));
    }
}

/// A track and its arc: filled to `fraction`, else a turning quarter.
fn ring(ui: &Ui, centre: Pos2, fraction: Option<f32>, now: f64, t: &Tokens) {
    let painter = ui.painter();
    let r = RING / 2.0 - 2.0;
    painter.circle_stroke(centre, r, Stroke::new(3.0, t.line));
    let (from, sweep) = match fraction {
        Some(f) => (-TAU / 4.0, f * TAU),
        None => (((now * 1.4).fract() as f32) * TAU, TAU / 4.0),
    };
    if sweep > 0.0 {
        let steps = ((sweep / TAU) * 48.0).ceil().max(2.0) as usize;
        let points = (0..=steps)
            .map(|i| from + sweep * i as f32 / steps as f32)
            .map(|a| centre + vec2(a.cos(), a.sin()) * r)
            .collect();
        painter.add(Shape::line(points, Stroke::new(3.0, t.accent)));
    }
    if let Some(f) = fraction {
        let percent = format!("{}", (f * 100.0).round() as u32);
        let font = FontId::proportional(9.5);
        painter.text(centre, Align2::CENTER_CENTER, percent, font, t.ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::library::{Heard, Proposal, Spot};
    use mareader_engine::library::{Out, State};
    use mareader_library::{Book, BookId, Origin};

    /// `copies` imported books, then one read in place.
    fn library(copies: usize) -> State {
        let mut state = State::default();
        state.seed_copies(copies);
        state.seed(1);
        state
    }

    /// The imported books, in a steady order.
    fn ids(state: &State) -> Vec<BookId> {
        let model = &state.model;
        let stored = |b: &&Book| b.home().is_some_and(|l| l.origin == Origin::Store);
        let mut ids: Vec<BookId> = model.books().filter(stored).map(|b| b.id).collect();
        ids.sort();
        ids
    }

    #[test]
    fn a_run_holds_its_books_until_its_plan_is_let_go() {
        let mut state = library(4);
        let all = ids(&state);
        state.organizing(all.clone(), "Fast");
        assert!(!state.held.is_empty());
        assert!(!state.picks.active(), "a run doesn't start selecting");
        assert!(state.ask_organize(None).is_err(), "one run at a time");
        let mut out = Vec::new();
        let progress = Heard::Progress {
            filing: true,
            done: 1,
            total: 2,
        };
        state.heard(progress, &mut out);
        assert_eq!(state.runs.list().len(), 1);
        let first = all.first().copied().unwrap_or(BookId(0));
        let plan = Proposal {
            shelves: vec![("Poems".into(), None)],
            placed: vec![(first, Spot::New(0))],
            left: Vec::new(),
        };
        state.heard(Heard::Plan(plan), &mut out);
        assert!(state.runs.list().is_empty(), "the card goes with the run");
        assert!(state.organizer().review().is_some());
        assert!(state.held.contains(&first));
        state.act(
            mareader_engine::library::Action::ApplyOrganized,
            0.0,
            &mut out,
        );
        assert!(state.held.is_empty());
        assert_eq!(state.model.shelves_of(first).len(), 1);
        let undo = out.iter().find_map(|c| match c {
            Out::Toast(t) => t.undo.clone(),
            _ => None,
        });
        let Some(undo) = undo else {
            panic!("an Undo toast")
        };
        let Command::Library(action) = *undo else {
            panic!("a library undo")
        };
        state.act(action, 0.0, &mut out);
        assert!(state.model.shelves_of(first).is_empty());
        assert_eq!(state.model.shelves().count(), 0);
    }
}
