//! Study mode in the frame: each studying pane's veil or deck, and the page
//! menu's Study section.

use super::*;
use crate::study_mode::{Pane, menu::Look, recall};
use egui::Context;
use mareader_engine::command::Command;
use mareader_engine::study_mode::{Action, Ask};
use mareader_settings::{Provider, TaskId};
use mareader_study::mode::{Mode, Practice};
use std::ops::Range;

impl MareaderApp {
    /// After the panes are drawn: veils and decks, and the menu's look.
    pub(super) fn show_study_mode(&mut self, ctx: &Context, out: &mut Commands) {
        let panes = self.tree().panes();
        self.ui.page_ai.study = Look::of(&self.session, &panes);
        let state = self.session.study_mode();
        let studying: Vec<PaneId> = panes.into_iter().filter(|&p| state.on(p)).collect();
        self.ui.study_mode.begin(&studying);
        let now = ctx.input(|i| i.time);
        let active = self.active();
        let lock = self.session.settings().study.mode.lock;
        let marks = crate::study_mode::marks(&self.session, active);
        self.ui.thumbs.set_study(marks);
        let docs = self.ui.documents.as_view(self.session.documents());
        for &(id, rect) in &self.ui.pane_rects {
            let (Some(run), Some(reading)) = (state.run(id), docs.reading(id)) else {
                continue;
            };
            let pane = Pane {
                id,
                rect,
                run,
                sheets: docs.sheets(id),
                mode: reading.mode,
                active: id == active,
                lock,
                now,
                current: reading.current,
                dict: self.session.study_have(id).dict,
                bar: !self.ui.chromeless && self.ui.page_bars.get(&id).is_some_and(|b| b.shown()),
            };
            self.ui.study_mode.show(ctx, &pane, out);
            if let Some(recalling) = state.recalling(id) {
                let word = [(recalling.word.page, recalling.word.chars.clone())];
                if let Some(at) = docs.spans_rect(id, &word) {
                    let draft = &mut self.ui.study_mode.recall;
                    recall::show(ctx, recalling, (at, rect), draft, out);
                }
            }
        }
        self.recall_shot(out);
    }

    /// A click on a shaded word: a recall in Reactive mode, else Explain.
    pub(super) fn shaded(&mut self, pane: PaneId, page: usize, chars: Range<usize>, at: Rect) {
        let run = self.session.study_mode().run(pane);
        if run.is_some_and(|r| r.mode == Mode::Reactive) {
            let ask = Ask::Open { page, chars };
            self.queue(Sent::Engine(Command::StudyMode(Action::Recall {
                pane,
                ask,
            })));
        } else {
            self.explain_shaded(pane, page, chars, at);
        }
    }

    /// Shots: opens the recall asked for once the focused pane recalls.
    fn recall_shot(&mut self, out: &mut Commands) {
        let pane = self.active();
        let state = self.session.study_mode();
        let reactive = state.run(pane).is_some_and(|r| r.mode == Mode::Reactive);
        if !reactive || state.recalling(pane).is_some() {
            return;
        }
        let Some(shot) = self.ui.study_mode.recall_shot.take() else {
            return;
        };
        let (needle, answer) = match shot.split_once(':') {
            Some((needle, answer)) => (needle, Some(answer)),
            None => (shot.as_str(), None),
        };
        let Some((page, chars)) = self.documents().find_word(pane, needle) else {
            self.ui.study_mode.recall_shot = Some(shot);
            return;
        };
        let open = Ask::Open { page, chars };
        out.push(Sent::Engine(Command::StudyMode(Action::Recall {
            pane,
            ask: open,
        })));
        if let Some(answer) = answer {
            let ask = Ask::Answer(answer.to_owned());
            out.push(Sent::Engine(Command::StudyMode(Action::Recall {
                pane,
                ask,
            })));
        }
    }
}

/// The study mode keys a shot or a test sets by name.
///
/// `study_mode=proactive|reactive|off` turns it on or off in the focused
/// pane, `study_gate=on` takes that pane to its gate,
/// `study_deck=reveal` turns the deck's card over once the deck opens and
/// `study_deck=drag:-140` holds it 140 points to the left, mid-swipe,
/// `study_cards=ready|silent|failing` has a scripted AI write the cards and
/// judge recalls, and `recall=word` or `recall=word:answer` opens a recall on
/// the first `word` of the page read, and answers it.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    if key == "study_cards" {
        return Some(practice(app, value));
    }
    if key == "recall" {
        app.ui.study_mode.recall_shot = Some(value.to_owned());
        return Some(Ok(()));
    }
    let pane = app.active();
    let action = match (key, value) {
        ("study_mode", "proactive") => Action::Toggle {
            pane,
            mode: Mode::Proactive,
        },
        ("study_mode", "reactive") => Action::Toggle {
            pane,
            mode: Mode::Reactive,
        },
        ("study_mode", "off") => Action::Off(pane),
        ("study_gate", "on") => Action::NextBatch(pane),
        ("study_deck", "reveal") => {
            app.ui.study_mode.reveal_shot = true;
            return Some(Ok(()));
        }
        ("study_deck", held) if held.starts_with("drag:") => {
            let Ok(dx) = held.trim_start_matches("drag:").parse() else {
                return Some(Err("study_deck: drag:points".into()));
            };
            app.ui.study_mode.drag_shot = Some(dx);
            return Some(Ok(()));
        }
        ("study_mode", _) => return Some(Err("study_mode: proactive, reactive or off".into())),
        ("study_gate", _) => return Some(Err("study_gate: on".into())),
        ("study_deck", _) => return Some(Err("study_deck: reveal or drag:points".into())),
        _ => return None,
    };
    app.queue(Sent::Engine(Command::StudyMode(action)));
    Some(Ok(()))
}

/// A scripted writer for the cards, with a model chosen so the AI writes them.
fn practice(app: &mut MareaderApp, value: &str) -> Result<(), String> {
    let practice = Practice::parse(value).ok_or("study_cards: ready, silent or failing")?;
    app.session.study_set_cards(Arc::new(practice));
    app.session.study_set_judge(Arc::new(practice));
    let ai = &app.session.settings().ai;
    if ai.model(TaskId::StudyCards).is_none() {
        let mut settings = app.session.settings().clone();
        let id = settings.ai.add_preset(Provider::Ollama, "practice");
        for task in [TaskId::StudyCards, TaskId::StudyJudge] {
            settings.ai.tasks.choose(task, Some(id));
        }
        app.set_settings(settings);
    }
    Ok(())
}
