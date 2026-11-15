//! What is heard: reading aloud, trying a voice, saying a word.

use mareader_speech::Item;
use mareader_workspace::PaneId;

use super::{note, set};
use crate::command::Command;
use crate::dict::Download;
use crate::event::Event;
use crate::read_aloud::ReadFrom;
use crate::session::Session;
use crate::toast::Toast;

pub(super) fn run(session: &mut Session, intent: Command, now: f64) {
    match intent {
        Command::PlaceReadMarker { pane, spot } => {
            session.clear_document_selection(pane);
            session.emit(Event::PlaceReadMarker { pane, spot, now });
            session.wake();
        }
        Command::ReadAloud { pane, from } => {
            // Reading from elsewhere sends a waiting pin off.
            session.emit(Event::DismissReadMarkers(now));
            read_aloud(session, pane, from);
        }
        Command::ReadAloudAct(act) => session.read_aloud_act(act),
        Command::ToggleReadAloud => {
            session.emit(Event::DismissReadMarkers(now));
            if !session.read_aloud_toggle() {
                let active = session.active();
                read_aloud(session, active, ReadFrom::default());
            }
        }
        Command::StopReadAloud => session.read_aloud_stop(),
        Command::RevealVoice(model, language) => {
            session.emit(Event::RevealVoice { model, language });
        }
        Command::VoiceModel(item, action) => model(session, item, action),
        other => say(session, other, now),
    }
}

fn read_aloud(session: &mut Session, pane: PaneId, from: ReadFrom) {
    if let Some(stand) = session.documents().stand(pane) {
        let fresh = session.read_aloud_start(&stand, from);
        session.emit(Event::ReadAloudStarted { fresh });
    }
}

fn say(session: &mut Session, intent: Command, now: f64) {
    match intent {
        Command::TryVoice => session.try_voice(),
        Command::TryVoiceOf(voice) => session.try_voice_of(&voice),
        Command::StopVoiceTry => session.stop_voice_try(),
        Command::SayWord(word) => heard(session, now, &word, None),
        Command::SayWordWith { word, voice } => heard(session, now, &word, Some(&voice)),
        Command::StopSaying => session.stop_saying(),
        _ => {}
    }
}

fn heard(session: &mut Session, now: f64, word: &str, voice: Option<&str>) {
    if let Err(why) = session.say_word(now, word, voice) {
        note(session, Toast::error(why));
    }
}

/// A download may change the voice picked, so the settings follow it.
fn model(session: &mut Session, item: Item, action: Download) {
    let mut settings = session.settings().read_aloud.clone();
    match session.voice_model(item, action, &mut settings) {
        Ok(()) => set(session, "read_aloud", serde_json::to_string(&settings)),
        Err(why) => note(session, Toast::error(why)),
    }
}
