//! Study mode's commands, the jumps its gate holds back, and what it refuses.

use mareader_study::mode::Batch;
use mareader_workspace::PaneId;

use crate::command::Command;
use crate::event::Event;
use crate::page_ai::Action as PageAi;
use crate::session::{Done, Session};
use crate::study_mode::{self as mode, Action};
use crate::toast::Toast;

pub(super) fn run(session: &mut Session, action: Action, now: f64) -> Done {
    match action {
        Action::Toggle { pane, mode: wanted } => {
            let same = session.study_mode().run(pane).map(|r| r.mode) == Some(wanted);
            if same {
                return off(session, pane);
            }
            match session.study_mode_on(pane, wanted) {
                Ok(()) => say(session, Toast::info(mode::ON)),
                Err(why) => return Done::refused(why),
            }
        }
        Action::Off(pane) => return off(session, pane),
        Action::Start(pane) => {
            if let Err(why) = session.study_mode_start(pane, now) {
                return Done::refused(why);
            }
        }
        Action::Known { pane, key } => return changed(session.study_mode_known(pane, &key)),
        Action::Undo(pane) => return changed(session.study_mode_undo(pane)),
        Action::Retry(pane) => return changed(session.study_mode_retry(pane)),
        Action::UseDictionary(pane) => return changed(session.study_mode_dictionary(pane)),
        Action::NextBatch(pane) => {
            let Some(gate) = session.study_gate(pane) else {
                return Done::refused("Every batch is unlocked.".to_owned());
            };
            session.study_show(pane, gate, now);
        }
        Action::ReadPrevious(pane) => {
            let previous = session.study_mode().run(pane).and_then(|r| r.previous());
            let Some(from) = previous.map(|b| b.from) else {
                return Done::refused("Nothing is unlocked yet.".to_owned());
            };
            session.study_show(pane, from, now);
            say(session, Toast::info(mode::READ_BACK));
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

/// `page`, or `pane`'s gate when study mode locks the pages past it.
pub(super) fn clamp(session: &mut Session, pane: PaneId, page: usize) -> usize {
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
        _ => return false,
    };
    if !session.study_mode().on(pane) {
        return false;
    }
    say(session, Toast::warning(mode::REFUSED));
    true
}
