//! The forms a script sends for a mark, a word, a reading and a voice.

use serde_json::Value;

use mareader_speech::{Build, Item, MODELS, Model, ModelVoice};

use crate::command::Command;
use crate::command_spec::{json, names};
use crate::read_aloud::ReadFrom;
use crate::session::Session;

/// `mark.bookmark`: a pane, or the one in focus when none is named.
pub(crate) fn bookmark(_at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::maybe_pane(params, "pane")?;
    Ok(Command::Bookmark(pane))
}

/// `mark.clear`: clears the active text selection in a pane.
pub(crate) fn clear_selection(at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::ClearSelection(json::pane(at, params)?))
}

/// `mark.highlight`: a pen over the pane's selection, in a colour if one is
/// named.
pub(crate) fn highlight(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let pen = json::maybe_one_of(params, "pen", names::PEN)?;
    let pen = pen.unwrap_or_default();
    let color = json::rgb(params, "color")?;
    Ok(Command::Highlight { pane, pen, color })
}

/// `mark.note`: a note on the pane's selection.
pub(crate) fn note(at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::Note(json::pane(at, params)?))
}

/// `mark.select`: the first of some text on a page, as a drag selects it.
pub(crate) fn select(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let page = json::count(params, "page")?;
    let text = json::text(params, "text")?;
    Ok(Command::SelectText { pane, page, text })
}

/// `word.explain`: the pane whose selection is explained.
pub(crate) fn explain(at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::Explain(json::pane(at, params)?))
}

/// `word.lookup`: the pane whose selection is looked up.
pub(crate) fn lookup(at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::LookUp(json::pane(at, params)?))
}

/// `word.say`: the word a card says.
pub(crate) fn say(_at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::SayWord(json::text(params, "word")?))
}

/// `word.say-with`: a word, and a voice other than the one picked.
pub(crate) fn say_with(_at: &Session, params: &Value) -> Result<Command, String> {
    let word = json::text(params, "word")?;
    let voice = json::text(params, "voice")?;
    Ok(Command::SayWordWith { word, voice })
}

/// `word.stop`: the card's word, stopped.
pub(crate) fn stop_saying(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::StopSaying)
}

/// `read.start`: a pane, and the place, the spot and the end a reading takes.
pub(crate) fn read_start(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let anchor = json::maybe_object(params, "from");
    let from = ReadFrom {
        anchor: anchor.map(json::anchor).transpose()?,
        at: json::spot(params, "at")?,
        until: json::spot(params, "until")?,
    };
    Ok(Command::ReadAloud { pane, from })
}

/// `read.toggle`: the reading played or paused.
pub(crate) fn read_toggle(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::ToggleReadAloud)
}

/// `read.stop`: the reading stopped.
pub(crate) fn read_stop(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::StopReadAloud)
}

/// `read.marker`: the spot reading aloud is pinned to.
pub(crate) fn read_marker(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let spot = json::spot(params, "at")?;
    let Some(spot) = spot else {
        return Err(json::missing("at"));
    };
    Ok(Command::PlaceReadMarker { pane, spot })
}

/// `voice.try`: the voice settings' sample line.
pub(crate) fn voice_try(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::TryVoice)
}

/// `voice.try-of`: a sample line in one voice of the tab's engine.
pub(crate) fn voice_try_of(_at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::TryVoiceOf(json::text(params, "voice")?))
}

/// `voice.stop-try`: the sample line stopped.
pub(crate) fn voice_stop_try(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::StopVoiceTry)
}

/// `voice.model`: a model, one of its builds or one of its voices, and what the
/// download is asked to do.
pub(crate) fn voice_model(_at: &Session, params: &Value) -> Result<Command, String> {
    let model = model_named(params)?;
    let build = json::maybe_text(params, "build")?;
    let voice = json::maybe_text(params, "voice")?;
    let item = match (build, voice) {
        (Some(build), None) => Item::Build(model.id, built(model, &build)?.id),
        (None, Some(voice)) => Item::Voice(model.id, voiced(model, &voice)?.id),
        (None, None) => Item::Model(model.id),
        (Some(_), Some(_)) => {
            let what = "either it or \"voice\", not both";
            return Err(json::wants("build", what));
        }
    };
    let act = json::one_of(params, "act", names::DOWNLOAD)?;
    Ok(Command::VoiceModel(item, act))
}

/// `voice.reveal`: a model, and a language one of its voices speaks.
pub(crate) fn voice_reveal(_at: &Session, params: &Value) -> Result<Command, String> {
    let model = model_named(params)?;
    let language = json::text(params, "language")?;
    let held = model
        .voices
        .iter()
        .find(|voice| voice.language == language)
        .map(|voice| voice.language)
        .ok_or_else(|| no_such("language", model, &language))?;
    Ok(Command::RevealVoice(model, held))
}

/// `vocab.download`: what the word levels' download is asked to do.
pub(crate) fn vocab_download(_at: &Session, params: &Value) -> Result<Command, String> {
    let act = json::one_of(params, "act", names::DOWNLOAD)?;
    Ok(Command::Vocab(act))
}

/// `dict.pronunciation`: what the pronunciation download is asked to do.
pub(crate) fn pronunciation(_at: &Session, params: &Value) -> Result<Command, String> {
    let act = json::one_of(params, "act", names::DOWNLOAD)?;
    Ok(Command::Pronunciation(act))
}

/// `vocab.attach`: the folder the word levels' dataset is in.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn vocab_attach(_at: &Session, params: &Value) -> Result<Command, String> {
    Ok(Command::AttachVocab(json::path(params, "folder")?))
}

/// The model a name picks, out of the four the engine ships.
///
/// A model's own tables are where a script's `&'static` strings come from: its
/// builds' ids, its voices' ids and the languages those voices speak. Nothing
/// here makes a string live longer than the params it was read from.
pub(crate) fn model(name: &str) -> Result<&'static Model, String> {
    let found = MODELS.iter().find(|held| held.id == name);
    found.ok_or_else(|| {
        let known: Vec<&str> = MODELS.iter().map(|held| held.id).collect();
        json::wants("model", &known.join(" or "))
    })
}

/// The model a script names in its params.
fn model_named(params: &Value) -> Result<&'static Model, String> {
    model(&json::text(params, "model")?)
}

/// One of a model's builds, which is where its shared files are named.
fn built(model: &Model, name: &str) -> Result<&'static Build, String> {
    model
        .build(name)
        .ok_or_else(|| no_such("build", model, name))
}

/// One of a model's voices, which is where a language is named.
fn voiced(model: &Model, name: &str) -> Result<&'static ModelVoice, String> {
    model
        .voice(name)
        .ok_or_else(|| no_such("voice", model, name))
}

/// A name a model does not hold, with what it does hold listed.
fn no_such(key: &str, model: &Model, name: &str) -> String {
    format!("the model {:?} has no {key} {:?}", model.id, name)
}
