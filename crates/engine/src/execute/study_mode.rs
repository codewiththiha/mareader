//! Study mode's commands, the jumps its gate holds back, and what it refuses.

use mareader_study::mode::{Batch, Mode};
use mareader_workspace::PaneId;

use crate::command::Command;
use crate::event::Event;
use crate::page_ai::Action as PageAi;
use crate::session::{Done, Session};
use crate::skills::Action as SkillAction;
use crate::study_mode::{self as mode, Action};
use crate::toast::Toast;

pub(super) fn run(session: &mut Session, action: Action, now: f64) -> Done {
    match action {
        Action::Toggle { pane, mode: wanted } => {
            let same = session.study_mode().run(pane).map(|r| r.mode) == Some(wanted);
            if same {
                return off(session, pane);
            }
            let on = match wanted {
                Mode::Proactive => mode::ON,
                Mode::Reactive => mode::ON_REACTIVE,
            };
            match session.study_mode_on(pane, wanted) {
                Ok(()) => say(session, Toast::info(on)),
                Err(why) => return Done::refused(why),
            }
        }
        Action::Off(pane) => return off(session, pane),
        Action::Start(pane) => {
            if let Err(why) = session.study_mode_start(pane, now) {
                return Done::refused(why);
            }
        }
        Action::Shown { pane, shown } => {
            return changed(session.study_mode_shown(pane, shown, now));
        }
        Action::Reopen(pane) => return changed(session.study_mode_reopen(pane)),
        Action::Known { pane, key } => return changed(session.study_mode_known(pane, &key)),
        Action::Undo(pane) => return changed(session.study_mode_undo(pane)),
        Action::Retry(pane) => return changed(session.study_mode_retry(pane)),
        Action::UseDictionary(pane) => return changed(session.study_mode_dictionary(pane)),
        Action::NextBatch(pane) => {
            let Some(gate) = session.study_gate(pane) else {
                return Done::refused("Every batch is unlocked.".to_owned());
            };
            session.study_show(pane, gate, now);
            session.study_mode_reopen(pane);
        }
        Action::ReadPrevious(pane) => {
            let Some(page) = session.study_mode_leave(pane, now) else {
                return Done::refused("Nothing comes before these pages.".to_owned());
            };
            session.study_show(pane, page, now);
            say(session, Toast::info(mode::READ_BACK));
        }
        Action::Recall { pane, ask } => {
            return match session.study_recall(pane, ask) {
                Ok(moved) => changed(moved),
                Err(why) => Done::refused(why),
            };
        }
    }
    Done::changed()
}

fn off(session: &mut Session, pane: PaneId) -> Done {
    if !session.study_mode_off(pane) {
        return Done::quiet();
    }
    say(session, Toast::info(mode::OFF));
    Done::changed()
}

fn changed(moved: bool) -> Done {
    match moved {
        true => Done::changed(),
        false => Done::quiet(),
    }
}

fn say(session: &mut Session, toast: Toast) {
    session.emit(Event::Toast(toast));
}

/// `page`, held by study mode between its locked start and its gate.
pub(super) fn clamp(session: &mut Session, pane: PaneId, page: usize) -> usize {
    let floor = session.study_floor(pane);
    if page < floor {
        say(session, Toast::warning(mode::before(floor)));
        return floor;
    }
    let Some(gate) = session.study_gate(pane) else {
        return page;
    };
    if page <= gate {
        return page;
    }
    let batch = session.study_mode().run(pane).and_then(|r| r.gate_batch());
    let range = batch.map_or_else(|| "these pages".to_owned(), Batch::label);
    say(session, Toast::warning(mode::locked(&range)));
    gate
}

/// Whether study mode refuses `intent`: no word help while a pane studies.
pub(super) fn locks(session: &mut Session, intent: &Command) -> bool {
    let pane = match intent {
        Command::Explain(pane) | Command::LookUp(pane) => *pane,
        Command::PageAi(
            PageAi::Run { pane, .. }
            | PageAi::Translate { pane, .. }
            | PageAi::RunPages { pane, .. }
            | PageAi::PagesMenu { pane, .. }
            | PageAi::Redo { pane, .. },
        ) => *pane,
        Command::Skill(SkillAction::Run { pane, .. } | SkillAction::RunPage { pane, .. }) => *pane,
        _ => return false,
    };
    if !session.study_mode().on(pane) {
        return false;
    }
    say(session, Toast::warning(mode::REFUSED));
    true
}
