//! Study mode in the frame: each studying pane's veil or deck, and the page
//! menu's Study section.

use super::*;
use crate::study_mode::{Pane, menu::Look};
use mareader_engine::command::Command;
use mareader_engine::study_mode::Action;
use mareader_settings::Provider;
use mareader_study::mode::{Mode, Practice};

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
        let strict = self.session.settings().study.mode.strict;
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
                strict,
                now,
                current: reading.current,
                dict: self.session.study_have(id).dict,
            };
            self.ui.study_mode.show(ctx, &pane, out);
        }
    }
}

/// The study mode keys a shot or a test sets by name.
///
/// `study_mode=proactive|reactive|off` turns it on or off in the focused
/// pane, `study_gate=on` takes that pane to its gate,
/// `study_deck=reveal` turns the deck's card over once the deck opens, and
/// `study_cards=ready|silent|failing` has a scripted AI write the cards.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    if key == "study_cards" {
        return Some(practice(app, value));
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
        ("study_mode", _) => return Some(Err("study_mode: proactive, reactive or off".into())),
        ("study_gate", _) => return Some(Err("study_gate: on".into())),
        ("study_deck", _) => return Some(Err("study_deck: reveal".into())),
        _ => return None,
    };
    app.queue(Sent::Engine(Command::StudyMode(action)));
    Some(Ok(()))
}

/// A scripted writer for the cards, with a model chosen so the AI writes them.
fn practice(app: &mut MareaderApp, value: &str) -> Result<(), String> {
    let practice = Practice::parse(value).ok_or("study_cards: ready, silent or failing")?;
    app.session.study_set_cards(Arc::new(practice));
    if app.session.settings().ai.study_preset().is_none() {
        let mut settings = app.session.settings().clone();
        let id = settings.ai.add_preset(Provider::Ollama, "practice");
        settings.ai.tasks.study.preset = Some(id);
        app.set_settings(settings);
    }
    Ok(())
}
