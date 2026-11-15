//! A downloaded model's tab.

use egui::Ui;
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Build, Item, Model, ModelVoice, Shelf, Stage};

use super::downloads::{Buttons, muted};
use super::read_aloud::{Voices, offer};
use super::{
    detail_row, detail_row_tip, divider, fold_row, folded, group, note, radio, row, section,
};
use crate::dict::megabytes;
use crate::icons::Icon;
use crate::read_aloud::Booth;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::dict::Download;

pub fn tab(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    model: &'static Model,
    v: &Voices<'_>,
    out: &mut Commands,
) {
    let shelf = v.shelf;
    if model.builds.len() > 1 {
        builds(ui, s, model, shelf, out);
    }
    section(ui, "Voices");
    // Voices with no file of their own come inside every build.
    let inside = model.voices.iter().all(|v| v.file.is_none());
    // A voice picked, then removed, falls back to the first.
    let picked = s.voice_for(model.id).and_then(|v| model.voice(v));
    let picked = picked
        .filter(|v| shelf.has(model, v.id))
        .map_or(model.first_voice, |v| v.id);
    let room = Room {
        model,
        shelf,
        booth: v.booth,
        picked,
    };
    group(ui, |ui| {
        if !inside {
            let every: Vec<&'static ModelVoice> = model.voices.iter().collect();
            every_voice(ui, model, &every, shelf, out);
            divider(ui);
        }
        if let Some(id) = languages(ui, &room, inside, out) {
            s.voices.insert(model.id.to_owned(), id.to_owned());
        }
    });
    let failed = v.booth.engine() == model.id && !v.booth.ready();
    if let Some(why) = v.booth.fault().filter(|_| failed) {
        note(ui, &format!("Voices can't be heard here: {why}"));
    }
    if !model.knobs.is_empty() {
        sound(ui, s, model);
    }
    section(ui, "Model");
    group(ui, |ui| {
        let offer = offer(model);
        detail_row_tip(ui, (model.name, &offer), model.about, |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            buttons(Item::Model(model.id), model.name).remove(ui, out);
            let size = megabytes(shelf.on_disk(model));
            let r = muted(ui, &size);
            r.on_hover_text("Removing it deletes all of this: its voices and builds too");
        });
    });
}

/// The model's own settings, each a share of how a voice was made.
fn sound(ui: &mut Ui, s: &mut ReadAloudSettings, model: &'static Model) {
    section(ui, "Sound");
    let knobs = s.knobs.entry(model.id.to_owned()).or_default();
    group(ui, |ui| {
        for (i, knob) in model.knobs.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            let mut value = knob.clamp(knobs.get(knob.id).copied());
            row(ui, knob.name, knob.about, |ui| {
                ui.spacing_mut().slider_width = 160.0;
                let (least, most) = knob.range;
                let slider = egui::Slider::new(&mut value, least..=most)
                    .step_by(0.05)
                    .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                if ui.add(slider).changed() {
                    knobs.insert(knob.id.to_owned(), value);
                }
            });
        }
        divider(ui);
        let set = knobs.values().any(|v| *v != 1.0);
        row(ui, "As made", "Every setting back to 100%", |ui| {
            if ui.add_enabled(set, egui::Button::new("Reset")).clicked() {
                knobs.clear();
            }
        });
    });
    if knobs.is_empty() {
        s.knobs.remove(model.id);
    }
}

/// The builds on offer: each downloads alone, one is used.
fn builds(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    model: &'static Model,
    shelf: &Shelf,
    out: &mut Commands,
) {
    section(ui, "Build");
    let wanted = s.build_for(model.id);
    let used = shelf.build(model, wanted).map(|b| b.id);
    let here = shelf.builds(model).len();
    group(ui, |ui| {
        for (i, build) in model.builds.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if build_row(
                ui,
                (model, build),
                (used == Some(build.id), here),
                shelf,
                out,
            ) {
                s.builds.insert(model.id.to_owned(), build.id.to_owned());
            }
        }
    });
}

/// One build: used, or its download; true when picked now.
fn build_row(
    ui: &mut Ui,
    (model, build): (&'static Model, &'static Build),
    (on, here): (bool, usize),
    shelf: &Shelf,
    out: &mut Commands,
) -> bool {
    let item = Item::Build(model.id, build.id);
    let name = mareader_speech::label(model, build);
    let mut pick = false;
    detail_row(ui, build.name, build.about, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let stage = shelf.stage(item);
        if *stage != Stage::Ready {
            buttons(item, &name).controls(ui, stage, build.bytes(), out);
            return;
        }
        pick = radio(ui, on, &format!("Use {}", build.name));
        // The last build goes with the model, below.
        if here > 1 {
            buttons(item, &name).remove(ui, out);
        }
        muted(ui, &megabytes(build.bytes()));
    });
    pick
}

/// What a voice row looks at besides its voice.
struct Room<'a> {
    model: &'static Model,
    shelf: &'a Shelf,
    /// Says a line in a voice, once it opened this model.
    booth: &'a Booth,
    picked: &'static str,
}

/// Each language's voices, folded until opened.
fn languages(
    ui: &mut Ui,
    room: &Room<'_>,
    inside: bool,
    out: &mut Commands,
) -> Option<&'static str> {
    let model = room.model;
    let mut languages: Vec<&'static str> = Vec::new();
    for v in model.voices {
        if !languages.contains(&v.language) {
            languages.push(v.language);
        }
    }
    let mut pick = None;
    let mut rows = |ui: &mut Ui, voices: &[&'static ModelVoice]| {
        for (i, voice) in voices.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if let Some(id) = voice_row(ui, room, voice, out) {
                pick = Some(id);
            }
        }
    };
    let one = languages.len() < 2;
    for (i, language) in languages.into_iter().enumerate() {
        let voices: Vec<&'static ModelVoice> = (model.voices.iter())
            .filter(|v| v.language == language)
            .collect();
        if one {
            rows(ui, &voices);
            continue;
        }
        if i > 0 {
            divider(ui);
        }
        let here = (voices.iter())
            .filter(|v| room.shelf.has(model, v.id))
            .count();
        let detail = match (voices.len(), inside) {
            (1, _) => "1 voice".to_owned(),
            (n, true) => format!("{n} voices"),
            (n, false) => format!("{n} voices · {here} downloaded"),
        };
        if fold_row(ui, fold_id(model, language), (accent(language), &detail)) {
            divider(ui);
            folded(ui, |ui| rows(ui, &voices));
        }
    }
    pick
}

/// The fold of a model's voices in `language`, as `fr-FR`.
pub fn fold_id(model: &Model, language: &str) -> egui::Id {
    egui::Id::new(("voice language", model.id, language))
}

/// Downloads every voice shown still missing at once.
fn every_voice(
    ui: &mut Ui,
    model: &'static Model,
    shown: &[&'static ModelVoice],
    shelf: &Shelf,
    out: &mut Commands,
) {
    let missing: Vec<&ModelVoice> = (shown.iter().copied())
        .filter(|v| !shelf.has(model, v.id))
        .collect();
    let detail = format!("{} voices", shown.len());
    detail_row(ui, "All voices", &detail, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if missing.is_empty() {
            muted(ui, "All downloaded");
            return;
        }
        let label = "Download every voice";
        if widgets::icon_button(ui, Icon::Download, label, false).clicked() {
            for v in &missing {
                let item = Item::Voice(model.id, v.id);
                out.push(Sent::Engine(Command::VoiceModel(item, Download::Start)));
            }
        }
        let bytes = shelf.to_fetch(model, &missing);
        muted(ui, &format!("{} to go", megabytes(bytes)));
    });
}

/// One voice: picked, or its download; the id when picked now.
fn voice_row(
    ui: &mut Ui,
    room: &Room<'_>,
    voice: &'static ModelVoice,
    out: &mut Commands,
) -> Option<&'static str> {
    let (model, shelf) = (room.model, room.shelf);
    let on = room.picked == voice.id;
    let mut detail = format!("{}, {}", accent(voice.language), voice.gender.label());
    if !voice.note.is_empty() {
        detail = format!("{detail} · {}", voice.note);
    }
    let item = Item::Voice(model.id, voice.id);
    let mut pick = None;
    detail_row(ui, voice.name, &detail, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !shelf.has(model, voice.id) {
            let bytes = shelf.to_fetch(model, &[voice]);
            buttons(item, voice.name).controls(ui, shelf.stage(item), bytes, out);
            return;
        }
        if radio(ui, on, &format!("Use {}", voice.name)) {
            pick = Some(voice.id);
        }
        preview(ui, room, voice, out);
        // The first voice came with the model and goes with it.
        if voice.id != model.first_voice && voice.file.is_some() {
            buttons(item, voice.name).remove(ui, out);
        }
    });
    pick
}

/// Says a line in a downloaded voice, or stops it.
fn preview(ui: &mut Ui, room: &Room<'_>, voice: &'static ModelVoice, out: &mut Commands) {
    let booth = room.booth;
    let here = booth.engine() == room.model.id;
    let saying = here && booth.trying_voice() == Some(voice.id);
    let (icon, verb, command) = match saying {
        true => (Icon::Stop, "Stop", Sent::Engine(Command::StopVoiceTry)),
        false => (
            Icon::Try,
            "Preview",
            Sent::Engine(Command::TryVoiceOf(voice.id.to_owned())),
        ),
    };
    let label = format!("{verb} {}", voice.name);
    let r = ui.add_enabled_ui(here && booth.ready(), |ui| {
        widgets::icon_button(ui, icon, &label, saying)
    });
    if r.inner.clicked() {
        out.push(command);
    }
    if saying && booth.loading() {
        ui.spinner();
    }
}

/// The buttons of the voice-model download `item`, called `name`.
pub(super) fn buttons(item: Item, name: &str) -> Buttons<'_, impl Fn(Download) -> Sent> {
    Buttons {
        name,
        ask: move |what| Sent::Engine(Command::VoiceModel(item, what)),
    }
}

/// A voice's language as a listener names it: `en-GB` is British English.
pub fn accent(tag: &str) -> &str {
    match tag {
        "en-US" => "American English",
        "en-GB" => "British English",
        "es-ES" => "Spanish",
        "it-IT" => "Italian",
        "pt-BR" => "Brazilian Portuguese",
        "fr-FR" => "French",
        "hi-IN" => "Hindi",
        "zh-CN" => "Mandarin Chinese",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_voices_are_named_by_their_accent() {
        assert_eq!(accent("en-US"), "American English");
        assert_eq!(accent("en-GB"), "British English");
        assert_eq!(accent("it-IT"), "Italian");
        assert_eq!(accent("pt-BR"), "Brazilian Portuguese");
        assert_eq!(accent("fr-FR"), "French");
        assert_eq!(accent("hi-IN"), "Hindi");
        assert_eq!(accent("zh-CN"), "Mandarin Chinese");
        assert_eq!(accent("ja-JP"), "ja-JP");
    }
}
