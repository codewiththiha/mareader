//! Read aloud: the engine, its voice, how it sounds, and how reading goes.

use super::downloads::muted;
use super::voice_model::buttons;
use super::{ROW_H, divider, fold_row, group, note, pick_row, row, section, switch_row};
use crate::icons::Icon;
use crate::platform::Platform;
use crate::read_aloud::Booth;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::{keymap, widgets};
use egui::{Align, Color32, Id, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;
use mareader_settings::{Highlight, LANGUAGES, Player, ReadAloudSettings, Skip};
use mareader_speech::{Build, Item, Model, Shelf, Stage, Voice};

/// What the tab shows besides settings: voices, models, the keys.
pub struct Voices<'a> {
    pub booth: &'a Booth,
    /// Voice models on offer and their downloads.
    pub shelf: &'a Shelf,
    pub platform: Platform,
    /// The pronunciation pack, which shares the models' files.
    pub sounds: &'a crate::sounds::Sounds,
    /// Who says English words on cards, picking their accent.
    pub english: Option<crate::sounds::EnglishVoice>,
}

const VOICE_H: f32 = 34.0;
/// The language filter's pick for every language.
const ALL: &str = "";

pub fn tab(ui: &mut Ui, s: &mut ReadAloudSettings, v: &Voices<'_>, out: &mut Commands) {
    engine(ui, s, v, out);
    voice(ui, s, v.booth, out);
    sound(ui, s, v.booth);
    reading(ui, s);
    let keys =
        keymap::hint(v.platform, &Sent::Engine(Command::ToggleReadAloud)).unwrap_or_default();
    note(
        ui,
        &format!("Start or pause reading with {keys}, or right-click a page to read from there."),
    );
    memory(ui, s, v.shelf);
}

/// The system's voices, then each model: picked here, managed in its tab.
fn engine(ui: &mut Ui, s: &mut ReadAloudSettings, v: &Voices<'_>, out: &mut Commands) {
    section(ui, "Engine");
    let shelf = v.shelf;
    let runs = mareader_speech::engines(shelf);
    let system = mareader_speech::SYSTEM;
    let booth = v.booth;
    let heard = booth.engine() == system && booth.ready() && booth.caps().voices;
    let detail = match heard {
        true => format!("Built into your system · {}", count(booth.voices().len())),
        false => "Built into your system".to_owned(),
    };
    let t = Tokens::get(ui.ctx());
    group(ui, |ui| {
        let on = Some(s.engine == system);
        let mark = engine_mark(system, t.muted);
        if pick_row(ui, ("System voices", &detail), "", on, mark, |_| {}) {
            s.engine = system.to_owned();
        }
        for model in shelf.catalogue() {
            divider(ui);
            let runs = runs.iter().any(|(id, _)| *id == model.id);
            if model_row(ui, s, model, (shelf, runs), out) {
                s.engine = model.id.to_owned();
            }
        }
        // One unlisted still shows, so it can be left.
        let listed = s.engine == system || shelf.model(&s.engine).is_some();
        if !listed {
            let lines = match s.engine.as_str() {
                mareader_speech::SILENT => {
                    ("Silent voice", "Says nothing: for tests and screenshots")
                }
                _ => ("Unavailable engine", "Not in this build"),
            };
            divider(ui);
            let mark = engine_mark(&s.engine, t.muted);
            pick_row(ui, lines, "", Some(true), mark, |_| {});
        }
    });
    if !shelf.attached() {
        note(ui, "Downloads need the desktop app.");
    }
    if let Some(fault) = booth.fault() {
        ui.add_space(8.0);
        ui.label(RichText::new(fault).size(t.text_small).color(t.danger));
    }
}

/// A muted engine mark keeps the row's text hierarchy.
fn engine_mark(id: &str, ink: Color32) -> Option<(Icon, Color32)> {
    Icon::engine(id).map(|mark| (mark, ink))
}

/// A model: picked once it runs here, else its download; true when picked.
fn model_row(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    model: &'static Model,
    (shelf, runs): (&Shelf, bool),
    out: &mut Commands,
) -> bool {
    let wanted = s.build_for(model.id).and_then(|b| model.build(b));
    let picked = wanted.or(model.builds.first());
    let offer = offer(model);
    let on = (runs || s.engine == model.id).then_some(s.engine == model.id);
    let t = Tokens::get(ui.ctx());
    let mark = engine_mark(model.id, t.muted);
    pick_row(ui, (model.name, &offer), model.about, on, mark, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !shelf.attached() {
            let bytes = picked.map_or(model.bytes(), |b| model.bytes_with(b));
            muted(ui, &crate::dict::megabytes(bytes));
            return;
        }
        if shelf.ready().iter().any(|m| m.id == model.id) {
            let open = egui::Button::new(("Manage", Icon::ChevronRight.image(ui, 14.0)))
                .image_tint_follows_text_color(true);
            let r = ui.add(open);
            let label = format!("Manage {}", model.name);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            let hint = format!("{}'s voices, builds and sound", model.name);
            if r.on_hover_text(hint).clicked() {
                out.push(Sent::Engine(Command::OpenSettings(Tab::Model(model))));
            }
            let size = crate::dict::megabytes(shelf.on_disk(model));
            muted(ui, &size).on_hover_text("On disk with its voices and builds");
            return;
        }
        // Without builds, or with one here, only the shared files are owed.
        let Some(picked) = picked.filter(|_| shelf.builds(model).is_empty()) else {
            let item = Item::Model(model.id);
            buttons(item, model.name).controls(ui, shelf.stage(item), model.bytes(), out);
            return;
        };
        // A build begun, else the one picked, with the shared files.
        let begun = (model.builds.iter())
            .find(|b| *shelf.stage(Item::Build(model.id, b.id)) != Stage::Absent);
        let build = begun.unwrap_or(picked);
        let item = Item::Build(model.id, build.id);
        let stage = shelf.stage(item);
        buttons(item, model.name).controls(ui, stage, model.bytes_with(build), out);
        if *stage == Stage::Absent
            && model.builds.len() > 1
            && let Some(id) = build_pick(ui, model, build)
        {
            s.builds.insert(model.id.to_owned(), id.to_owned());
        }
    })
}

/// "1 voice", "49 voices".
fn count(voices: usize) -> String {
    match voices {
        1 => "1 voice".to_owned(),
        n => format!("{n} voices"),
    }
}

/// What a model offers, short enough for its row.
pub(super) fn offer(model: &Model) -> String {
    let mut languages: Vec<String> = model.voices.iter().map(|v| primary(v.language)).collect();
    languages.sort();
    languages.dedup();
    let voices = count(model.voices.len());
    match languages.as_slice() {
        [one] => format!("{} · {voices}", language_name(one)),
        many => format!("{} languages · {voices}", many.len()),
    }
}

/// Which build a first download brings; the id when changed.
fn build_pick(ui: &mut Ui, model: &'static Model, on: &Build) -> Option<&'static str> {
    let mut pick = on.id;
    let combo = widgets::combo(("first build", model.id))
        .selected_text(on.name)
        .width(72.0);
    let shown = combo.show_ui(ui, |ui| {
        for b in model.builds {
            let size = crate::dict::megabytes(model.bytes_with(b));
            let r = ui.selectable_value(&mut pick, b.id, format!("{} · {size}", b.name));
            r.on_hover_text(b.about);
        }
    });
    let label = format!("{} build", model.name);
    shown
        .response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, &label));
    shown.response.on_hover_text(on.about);
    (pick != on.id).then_some(pick)
}

/// What downloaded models keep in memory, once one is downloaded.
fn memory(ui: &mut Ui, s: &mut ReadAloudSettings, shelf: &Shelf) {
    if shelf.ready().is_empty() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    section(ui, "Memory");
    group(ui, |ui| {
        let (least, _, most) = ReadAloudSettings::MEMORY_MB;
        let rate = speech_rate(ui, s, shelf);
        let made = "Made ahead and heard lately, to play again at once.";
        let hint = match (rate, shelf.model(&s.engine)) {
            (Some(rate), Some(m)) => {
                let khz = f64::from(rate) / 1000.0;
                format!("{made} Minutes are for {}, at {khz} kHz.", m.name)
            }
            _ => format!("{made} Pick a voice model to see how long it plays."),
        };
        row(ui, "Speech kept", &hint, |ui| {
            ui.spacing_mut().slider_width = 160.0;
            let slider = egui::Slider::new(&mut s.speech_memory_mb, least..=most)
                .logarithmic(true)
                .show_value(false);
            ui.add(slider);
            ui.add_space(t.gap);
            muted(ui, &memory_label(s.speech_memory_mb, rate));
        });
        divider(ui);
        let hint = "Counted from when reading pauses or stops";
        row(ui, "Keep a model loaded", hint, |ui| {
            widgets::combo("model-idle")
                .width(150.0)
                .selected_text(idle_label(s.model_idle_min))
                .show_ui(ui, |ui| {
                    for minutes in ReadAloudSettings::IDLE_MIN {
                        let label = idle_label(minutes);
                        ui.selectable_value(&mut s.model_idle_min, minutes, label);
                    }
                });
        });
        divider(ui);
        row(ui, "In memory now", "", |ui| {
            muted(ui, &usage_label(&shelf.usage()));
        });
    });
}

/// The engine's rate, as [`mareader_speech::speech_rate`], read once.
fn speech_rate(ui: &Ui, s: &ReadAloudSettings, shelf: &Shelf) -> Option<u32> {
    // A download can tell the rate; the count keys it afresh.
    let here = shelf.ready().len();
    let id = Id::new(("speech rate", s.engine.as_str(), s.voice(), here));
    if let Some(rate) = ui.data(|d| d.get_temp::<Option<u32>>(id)) {
        return rate;
    }
    let rate = mareader_speech::speech_rate(&s.engine, s.voice(), shelf);
    ui.data_mut(|d| d.insert_temp(id, rate));
    rate
}

/// Megabytes, and about how long that much speech plays at `rate`.
fn memory_label(mb: u32, rate: Option<u32>) -> String {
    let Some(rate) = rate.filter(|r| *r > 0) else {
        return format!("{mb} MB");
    };
    // Kept as 16-bit samples: two bytes each.
    let minutes = u64::from(mb) * (1 << 20) / (u64::from(rate) * 2) / 60;
    let time = match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    };
    format!("{mb} MB · about {time}")
}

fn idle_label(minutes: u32) -> String {
    match minutes {
        0 => "While reading".to_owned(),
        1 => "For 1 minute".to_owned(),
        n => format!("For {n} minutes"),
    }
}

fn usage_label(usage: &mareader_speech::Usage) -> String {
    let speech = (usage.speech > 0).then(|| {
        let mb = crate::dict::megabytes(usage.speech as u64);
        format!("{mb} of speech")
    });
    let parts: Vec<String> = (usage.models.iter().map(|m| (*m).to_owned()))
        .chain(speech)
        .collect();
    match parts.is_empty() {
        true => "Nothing loaded".to_owned(),
        false => parts.join(" · "),
    }
}

fn voice(ui: &mut Ui, s: &mut ReadAloudSettings, booth: &Booth, out: &mut Commands) {
    section(ui, "Voice");
    let open = booth.engine() == s.engine && booth.ready();
    let failed = booth.engine() == s.engine && booth.fault().is_some() && !open;
    group(ui, |ui| {
        if failed {
            row(ui, "Voice", "", |ui| {
                muted(ui, "Voices show once the engine starts");
            });
            divider(ui);
        } else if open && booth.caps().voices {
            let picked = s.voice().map(str::to_owned);
            let language = language_filter(ui, booth.voices(), picked.as_deref());
            divider(ui);
            if let Some(pick) = voice_list(ui, booth, &language, picked.as_deref(), out) {
                match pick {
                    Some(id) => s.voices.insert(s.engine.clone(), id),
                    None => s.voices.remove(&s.engine),
                };
            }
            divider(ui);
        } else if open {
            row(ui, "Voice", "", |ui| {
                muted(ui, "This engine speaks in one voice");
            });
            divider(ui);
        }
        preview_row(ui, booth, open, out);
    });
}

/// The language picker over the voices; returns the primary tag picked.
fn language_filter(ui: &mut Ui, voices: &[Voice], picked: Option<&str>) -> String {
    let id = Id::new("read-aloud-language");
    let mut languages: Vec<String> = voices.iter().map(|v| primary(&v.language)).collect();
    languages.sort_by_key(|l| language_name(l));
    languages.dedup();
    let chosen = ui.data(|d| d.get_temp::<String>(id));
    // First shown: the picked voice's language, else English, else all.
    let mut language = chosen.unwrap_or_else(|| {
        let of_pick = picked.and_then(|p| voices.iter().find(|v| v.id == p));
        match of_pick {
            Some(v) => primary(&v.language),
            None if languages.iter().any(|l| l == "en") => "en".to_owned(),
            None => ALL.to_owned(),
        }
    });
    row(
        ui,
        "Language",
        "Shows only voices for this language",
        |ui| {
            let shown = match language.as_str() {
                ALL => "All languages".to_owned(),
                tag => language_name(tag),
            };
            widgets::combo("read-aloud-language")
                .selected_text(shown)
                .width(180.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut language, ALL.to_owned(), "All languages");
                    for tag in &languages {
                        ui.selectable_value(&mut language, tag.clone(), language_name(tag));
                    }
                });
        },
    );
    ui.data_mut(|d| d.insert_temp(id, language.clone()));
    language
}

/// The default voice, then each in `language`, under a fold.
/// A click picks one.
fn voice_list(
    ui: &mut Ui,
    booth: &Booth,
    language: &str,
    picked: Option<&str>,
    out: &mut Commands,
) -> Option<Option<String>> {
    let voices = booth.voices();
    let shown: Vec<&Voice> = (voices.iter())
        .filter(|v| language == ALL || primary(&v.language) == language)
        .collect();
    let name = picked
        .and_then(|p| voices.iter().find(|v| v.id == p))
        .map_or("System default", |v| v.name.as_str());
    let detail = format!("{name} · {}", count(shown.len()));
    if !fold_row(ui, Id::new("read-aloud-voices"), ("Voices", &detail)) {
        return None;
    }
    divider(ui);
    let mut pick = None;
    ui.add_space(4.0);
    if voice_row(ui, ("System default", ""), picked.is_none(), None, out) {
        pick = Some(None);
    }
    for v in shown {
        let on = picked == Some(v.id.as_str());
        let saying = booth.trying_voice() == Some(v.id.as_str());
        if voice_row(ui, (&v.name, &v.language), on, Some((&v.id, saying)), out) {
            pick = Some(Some(v.id.clone()));
        }
    }
    ui.add_space(4.0);
    pick
}

/// A voice: name, tag, check if picked;
/// `say` is its id and if it sounds.
fn voice_row(
    ui: &mut Ui,
    (name, tag): (&str, &str),
    on: bool,
    say: Option<(&str, bool)>,
    out: &mut Commands,
) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), VOICE_H), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, on, name));
    // Its button sits on top; the row stays lit under it.
    let hot = ui.rect_contains_pointer(rect);
    let fill = match (on, hot) {
        (true, _) => t.selected,
        (false, true) => t.hover,
        (false, false) => egui::Color32::TRANSPARENT,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius, fill);
    let font = egui::FontId::proportional(t.text);
    let left = pos2(rect.left() + 10.0, rect.center().y);
    painter.text(left, egui::Align2::LEFT_CENTER, name, font, t.ink);
    let small = egui::FontId::proportional(t.text_small);
    let room = if say.is_some() { 64.0 } else { 34.0 };
    let right = pos2(rect.right() - room, rect.center().y);
    painter.text(right, egui::Align2::RIGHT_CENTER, tag, small, t.muted);
    if let Some((id, saying)) = say {
        let at =
            Rect::from_center_size(pos2(rect.right() - 46.0, rect.center().y), vec2(24.0, 24.0));
        let place = UiBuilder::new()
            .max_rect(at)
            .layout(Layout::left_to_right(Align::Center));
        let mut slot = ui.new_child(place);
        let (icon, verb, command) = match saying {
            true => (Icon::Stop, "Stop", Sent::Engine(Command::StopVoiceTry)),
            false => (
                Icon::Try,
                "Preview",
                Sent::Engine(Command::TryVoiceOf(id.to_owned())),
            ),
        };
        let label = format!("{verb} {name}");
        if widgets::small_icon_button(&mut slot, icon, &label).clicked() {
            out.push(command);
        }
    }
    if on {
        let mark =
            Rect::from_center_size(pos2(rect.right() - 17.0, rect.center().y), vec2(16.0, 16.0));
        Icon::Check
            .image(ui, 16.0)
            .tint(t.accent)
            .paint_at(ui, mark);
    }
    r.clicked() && !on
}

/// Tries the voice picked, or stops the try.
fn preview_row(ui: &mut Ui, booth: &Booth, open: bool, out: &mut Commands) {
    let hint = "Says a line in this voice, at this speed";
    row(ui, "Hear a sample", hint, |ui| {
        let (icon, text, command) = match booth.trying() {
            true => (Icon::Pause, "Stop", Sent::Engine(Command::StopVoiceTry)),
            false => (Icon::Resume, "Preview", Sent::Engine(Command::TryVoice)),
        };
        let button = egui::Button::image_and_text(icon.image(ui, 14.0), text)
            .image_tint_follows_text_color(true);
        if ui.add_enabled(open, button).clicked() {
            out.push(command);
        }
        if booth.loading() {
            let t = Tokens::get(ui.ctx());
            let text = RichText::new("Loading voice…").size(t.text_small);
            ui.label(text.color(t.muted));
        }
    });
}

fn sound(ui: &mut Ui, s: &mut ReadAloudSettings, booth: &Booth) {
    section(ui, "Sound");
    let caps = booth.caps();
    // Before the engine opens, nothing is greyed out on a guess.
    let known = booth.engine() == s.engine && booth.ready();
    group(ui, |ui| {
        row(ui, "Speed", "Narrated books play at this speed too", |ui| {
            ui.spacing_mut().slider_width = 160.0;
            let (lo, hi) = (ReadAloudSettings::SLOWEST, ReadAloudSettings::FASTEST);
            let slider = egui::Slider::new(&mut s.rate, lo..=hi)
                .step_by(0.05)
                .custom_formatter(|v, _| crate::read_aloud::rate_label(v as f32));
            ui.add(slider);
        });
        divider(ui);
        let hint = |can: bool, what: &'static str| match can || !known {
            true => "",
            false => what,
        };
        let can = caps.pitch || !known;
        row(
            ui,
            "Pitch",
            hint(caps.pitch, "This engine keeps its own pitch"),
            |ui| {
                ui.spacing_mut().slider_width = 160.0;
                let (lo, hi) = (ReadAloudSettings::LOWEST, ReadAloudSettings::HIGHEST);
                percent(ui, &mut s.pitch, (lo, hi), can);
            },
        );
        divider(ui);
        let can = caps.volume || !known;
        row(
            ui,
            "Volume",
            hint(caps.volume, "This engine keeps its own volume"),
            |ui| {
                ui.spacing_mut().slider_width = 160.0;
                percent(ui, &mut s.volume, (0.0, 1.0), can);
            },
        );
    });
}

/// A share as a percent slider, greyed out when the engine can't.
fn percent(ui: &mut Ui, share: &mut f32, (lo, hi): (f32, f32), can: bool) {
    let mut pct = *share * 100.0;
    let slider = egui::Slider::new(&mut pct, lo * 100.0..=hi * 100.0)
        .step_by(5.0)
        .fixed_decimals(0)
        .suffix("%");
    if ui.add_enabled(can, slider).changed() {
        *share = pct / 100.0;
    }
}

fn reading(ui: &mut Ui, s: &mut ReadAloudSettings) {
    section(ui, "While reading");
    group(ui, |ui| {
        let hint = "The page and scroll keep up with the words read";
        switch_row(ui, "Turn pages to follow", hint, &mut s.follow);
        divider(ui);
        highlight(ui, s);
        divider(ui);
        let hint = "What the player becomes when the words read are out of view";
        row(ui, "Player", hint, |ui| {
            // Added right to left, so they read left to right.
            ui.selectable_value(&mut s.player, Player::Full, "Full");
            ui.selectable_value(&mut s.player, Player::Compact, "Compact");
            ui.selectable_value(&mut s.player, Player::Bar, "Bar only");
        });
        divider(ui);
        let hint = "Numbers at the top or foot of a page are passed over";
        switch_row(ui, "Skip page numbers", hint, &mut s.skip_page_numbers);
        divider(ui);
        let hint = "Narrated books pass over their footnotes and asides";
        switch_row(ui, "Skip notes", hint, &mut s.skip_notes);
        divider(ui);
        let hint = "Pass over tables, or ask at each one";
        skip_row(ui, "Skip tables", hint, &mut s.skips.tables);
        divider(ui);
        let hint = "Pass over bulleted and numbered lists, or ask at each one";
        skip_row(ui, "Skip lists", hint, &mut s.skips.lists);
        divider(ui);
        let hint = "Pass over figures and their captions, or ask at each one";
        skip_row(ui, "Skip figures", hint, &mut s.skips.figures);
        divider(ui);
        let hint = "Pass over sidebars and boxed asides, or ask at each one";
        skip_row(ui, "Skip asides", hint, &mut s.skips.asides);
    });
}

/// A part's skip switch; on, how it skips slides open beneath.
fn skip_row(ui: &mut Ui, label: &str, hint: &str, skip: &mut Skip) {
    switch_row(ui, label, hint, &mut skip.on);
    let id = ui.id().with(("skip how", label));
    let open = ui.ctx().animate_bool_responsive(id, skip.on);
    if open <= 0.0 {
        return;
    }
    // Clipped to its share of the row, it fades in as it grows.
    let height = (ROW_H * open).round();
    let size = vec2(ui.available_width(), height);
    let (slot, _) = ui.allocate_exact_size(size, Sense::hover());
    let whole = Rect::from_min_size(slot.min, vec2(slot.width(), ROW_H));
    let mut child = ui.new_child(UiBuilder::new().max_rect(whole));
    child.set_clip_rect(slot.intersect(ui.clip_rect()));
    child.multiply_opacity(open);
    if open < 1.0 {
        child.disable();
    }
    let t = Tokens::get(ui.ctx());
    child.allocate_ui_with_layout(
        vec2(whole.width(), ROW_H),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(ROW_H);
            ui.add_space(14.0);
            let r = ui.label(RichText::new("Each time").color(t.muted));
            let hint = "Ask first, holding until you answer, or always pass over";
            widgets::tip(&r, hint, None);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Added right to left, so they read left to right.
                ui.selectable_value(&mut skip.ask, false, "Skip");
                ui.selectable_value(&mut skip.ask, true, "Ask me");
            });
        },
    );
}

/// How much of what is read is lit, and words at a time.
fn highlight(ui: &mut Ui, s: &mut ReadAloudSettings) {
    let hint = "Light the whole sentence, the phrase between pauses, or a few words";
    row(ui, "Highlight", hint, |ui| {
        // Added right to left, so they read left to right.
        ui.selectable_value(&mut s.highlight, Highlight::Words, "Words");
        ui.selectable_value(&mut s.highlight, Highlight::Phrases, "Phrases");
        ui.selectable_value(&mut s.highlight, Highlight::Sentence, "Sentence");
    });
    if s.highlight != Highlight::Words {
        return;
    }
    divider(ui);
    let hint = "Most voices keep exact time; the system's voice is estimated";
    row(ui, "Words at a time", hint, |ui| {
        let mut n = f32::from(s.highlight_words);
        let most = f32::from(ReadAloudSettings::WORDS_MOST);
        let show = |n: f32| format!("{n:.0}");
        widgets::stepper(ui, &mut n, 1.0..=most, 1.0, show, "words at a time");
        s.highlight_words = n as u8;
    });
}

/// A language tag's first part, lowercased: `en-GB` is `en`.
fn primary(tag: &str) -> String {
    let first = tag.split(['-', '_']).next().unwrap_or_default();
    first.to_ascii_lowercase()
}

/// A primary tag's English name, else the tag itself.
fn language_name(tag: &str) -> String {
    let named = LANGUAGES.iter().find(|(t, _)| primary(t) == tag);
    match named {
        // Both Chinese scripts share the one spoken name.
        Some(_) if tag == "zh" => "Chinese".to_owned(),
        Some((_, name)) => (*name).to_owned(),
        None => tag.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_rows_read_plainly() {
        assert_eq!(memory_label(16, Some(24_000)), "16 MB · about 5 min");
        assert_eq!(memory_label(64, Some(24_000)), "64 MB · about 23 min");
        // A Piper voice made at 16 kHz holds half as long again.
        assert_eq!(memory_label(64, Some(16_000)), "64 MB · about 34 min");
        assert_eq!(memory_label(400, Some(22_050)), "400 MB · about 2 h 38 min");
        assert_eq!(memory_label(100, None), "100 MB");
        assert_eq!(idle_label(0), "While reading");
        assert_eq!(idle_label(1), "For 1 minute");
        assert_eq!(idle_label(15), "For 15 minutes");
        let mut usage = mareader_speech::Usage::default();
        assert_eq!(usage_label(&usage), "Nothing loaded");
        usage.speech = 2_500_000;
        assert_eq!(usage_label(&usage), "2.5 MB of speech");
        usage.models.push("Kokoro (Small)".into());
        assert_eq!(usage_label(&usage), "Kokoro (Small) · 2.5 MB of speech");
    }

    #[test]
    fn a_model_row_names_its_languages_and_voices() {
        let kokoro = offer(&mareader_speech::KOKORO);
        assert!(kokoro.ends_with(" languages · 49 voices"), "{kokoro}");
        assert!(offer(&mareader_speech::PIPER).starts_with("English · "));
    }

    #[test]
    fn languages_are_named_by_their_first_part() {
        assert_eq!(primary("en-GB"), "en");
        assert_eq!(primary("pt_BR"), "pt");
        assert_eq!(language_name("en"), "English");
        assert_eq!(language_name("zh"), "Chinese");
        assert_eq!(language_name("my"), "Burmese");
        assert_eq!(language_name("tlh"), "tlh");
    }
}
