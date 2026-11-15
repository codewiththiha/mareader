//! The word list window: options, a live preview, then Copy or Save.

use super::deliver;
use super::preview::{self, Block};
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::{files::Files, icons::Icon, modal::Dialog, widgets};
use egui::{Frame, RichText, ScrollArea, Ui, Vec2, vec2};
use mareader_dict::PACKS;
use mareader_engine::word_list::{Ask, Gather, Made, Progress};
use mareader_engine::{command::Command, dict::Service as Dict, session::Session, settings::Tab};
use mareader_settings::{
    Settings, WORD_LANGUAGES, WordGroup, WordLayout, WordList, WordOutput, WordPreset, WordSort,
};
use mareader_workspace::PaneId;

const MAX_SIZE: Vec2 = vec2(1000.0, 700.0);
const OPTIONS_W: f32 = 300.0;
const FOOT_H: f32 = 52.0;
const PAD: f32 = 16.0;

/// The window, while open.
#[derive(Default)]
pub struct WordWindow {
    open: Option<Open>,
}

struct Open {
    pane: PaneId,
    title: String,
    pages: usize,
    /// Whether the book has an outline to group by.
    chapters: bool,
    gather: Gather,
    progress: Result<Progress, String>,
    /// The preset last chosen, while one is.
    preset: Option<String>,
    /// The name field, saving a new preset or renaming the chosen one.
    naming: Option<(String, bool)>,
    shown: Option<Shown>,
}

/// The list as last set out, and the options it was set out with.
struct Shown {
    list: WordList,
    made: Made,
    blocks: Vec<Block>,
    cut: bool,
}

impl WordWindow {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The pane whose pages are listed, while open.
    pub fn pane(&self) -> Option<PaneId> {
        self.open.as_ref().map(|o| o.pane)
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// Opens on `ask`, made with the options last used.
    pub fn open(&mut self, ctx: &egui::Context, pane: PaneId, ask: Ask, chapters: bool) {
        let (title, pages) = (ask.title.clone(), ask.pages.len());
        self.open = Some(Open {
            pane,
            title,
            pages,
            chapters,
            gather: Gather::new(crate::wake(ctx), ask),
            progress: Ok(Progress::Reading),
            preset: None,
            naming: None,
            shown: None,
        });
    }

    /// Tests: the Markdown shown, once made.
    pub fn markdown(&self) -> Option<&str> {
        let shown = self.open.as_ref()?.shown.as_ref()?;
        Some(&shown.made.markdown)
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        settings: &Settings,
        (session, files): (&mut Session, &Files),
        out: &mut Commands,
    ) {
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let list = &settings.study.words.last;
        if open.gather.list() != list {
            open.gather.set_list(list.clone());
        }
        if open.progress.as_ref().is_ok_and(|p| *p != Progress::Ready) || open.stale(list) {
            let step = session.step_word_list(&mut open.gather);
            open.progress = step.map_err(|t| t.message);
            // Answers land by waker; the list's own steps need frames.
            ctx.request_repaint();
        }
        if open.progress == Ok(Progress::Ready) && open.stale(list) {
            let made = open.gather.made();
            let (blocks, cut) = preview::parse(&made.markdown);
            let list = list.clone();
            open.shown = Some(Shown {
                list,
                made,
                blocks,
                cut,
            });
        }
        let size = (ctx.content_rect().size() * vec2(0.94, 0.88)).min(MAX_SIZE);
        let mut draft = settings.clone();
        let mut close = false;
        let modal = Dialog::new("word-list-window")
            .margin(PAD as i8)
            .show(ctx, |ui| {
                ui.set_width(size.x - 2.0 * PAD);
                ui.set_height(size.y - 2.0 * PAD);
                close = open.header(ui);
                let body = ui.available_height() - FOOT_H;
                ui.horizontal_top(|ui| {
                    ui.set_height(body);
                    ui.vertical(|ui| {
                        ui.set_width(OPTIONS_W);
                        ScrollArea::vertical()
                            .id_salt("word-list-options")
                            .content_margin(egui::Margin {
                                left: 0,
                                right: 12,
                                top: 0,
                                bottom: 0,
                            })
                            .max_height(body)
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                open.options(ui, &mut draft, session.dict(), out, &mut close)
                            });
                    });
                    ui.separator();
                    ui.vertical(|ui| open.preview(ui, body));
                });
                ui.separator();
                open.footer(ui, files, out);
            });
        if draft != *settings {
            // The preview redraws from the saved options, a frame on.
            ctx.request_repaint();
            out.push(Sent::Engine(Command::SetSettings(Box::new(draft))));
        }
        if close || modal.should_close() {
            self.open = None;
        }
    }
}

impl Open {
    /// Whether the list shown was set out with other options.
    fn stale(&self, list: &WordList) -> bool {
        self.shown.as_ref().is_none_or(|s| s.list != *list)
    }

    /// The title, what is listed, and the close button; true to close.
    fn header(&self, ui: &mut Ui) -> bool {
        let t = Tokens::get(ui.ctx());
        let mut close = false;
        ui.horizontal(|ui| {
            ui.add_space(4.0);
            ui.label(RichText::new("Word list").font(theme::medium(t.text * 1.2)));
            let pages = match self.pages {
                1 => "1 page".to_owned(),
                n => format!("{n} pages"),
            };
            let what = match self.title.trim().is_empty() {
                true => pages,
                false => format!("{pages} of {}", self.title.trim()),
            };
            ui.label(RichText::new(what).color(t.muted));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = widgets::small_icon_button(ui, Icon::Close, "Close").clicked();
            });
        });
        ui.separator();
        close
    }

    fn options(
        &mut self,
        ui: &mut Ui,
        draft: &mut Settings,
        dict: &Dict,
        out: &mut Commands,
        close: &mut bool,
    ) {
        ui.spacing_mut().item_spacing.y = 6.0;
        self.presets(ui, draft);
        let words = &mut draft.study.words;
        let list = &mut words.last;
        section(ui, "Languages", "Beside English, in column order");
        languages(ui, list, dict, out, close);
        section(ui, "Show", "");
        let f = &mut list.fields;
        row(ui, "CEFR level", &mut f.level);
        if !list.languages.is_empty() {
            row(ui, "Part of speech", &mut f.part_of_speech);
            row(ui, "Meaning", &mut f.definition);
            row(ui, "Romanization", &mut f.romanization);
            row(ui, "Sense", &mut f.sense);
            row(ui, "Similar words", &mut list.similar);
        }
        row(ui, "Page numbers", &mut list.pages);
        row(ui, "Book title", &mut list.book);
        section(ui, "Order", "");
        let sorts = [
            WordSort::Reading,
            WordSort::AToZ,
            WordSort::ZToA,
            WordSort::Level,
        ];
        widgets::choices(ui, &mut list.sort, &sorts, sort_label, 2);
        section(ui, "Group by", "");
        let mut group = match (self.chapters, list.group) {
            (false, WordGroup::Chapter) => WordGroup::None,
            (_, g) => g,
        };
        let before = group;
        let groups: &[WordGroup] = match self.chapters {
            true => &[WordGroup::Chapter, WordGroup::Page, WordGroup::None],
            false => &[WordGroup::Page, WordGroup::None],
        };
        widgets::choices(ui, &mut group, groups, group_label, groups.len());
        if group != before {
            list.group = group;
        }
        section(ui, "Layout", "");
        let layouts = [WordLayout::Table, WordLayout::List];
        widgets::choices(ui, &mut list.layout, &layouts, layout_label, 2);
        section(
            ui,
            "Quick export",
            "What the thumbnails menu's quick row does",
        );
        let outputs = [WordOutput::Copy, WordOutput::Markdown, WordOutput::Epub];
        widgets::choices(ui, &mut list.output, &outputs, output_label, 3);
        ui.add_space(8.0);
    }

    /// Choosing, saving, renaming and deleting presets.
    fn presets(&mut self, ui: &mut Ui, draft: &mut Settings) {
        let t = Tokens::get(ui.ctx());
        let words = &mut draft.study.words;
        // A preset deleted elsewhere stops being the chosen one.
        let known = |n: &String| words.presets.iter().any(|p| &p.name == n);
        if !self.preset.as_ref().is_some_and(known) {
            self.preset = None;
        }
        section(ui, "Preset", "");
        let chosen = self.preset.clone().unwrap_or_else(|| "Custom".into());
        let mut pick = None;
        widgets::combo("word-list-preset")
            .selected_text(chosen)
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for p in &words.presets {
                    let star = if words.instant.as_ref() == Some(&p.name) {
                        " ★"
                    } else {
                        ""
                    };
                    let on = self.preset.as_ref() == Some(&p.name);
                    if ui
                        .selectable_label(on, format!("{}{star}", p.name))
                        .clicked()
                    {
                        pick = Some(p.clone());
                    }
                }
                if words.presets.is_empty() {
                    ui.label(RichText::new("No presets yet").color(t.muted));
                }
            });
        if let Some(p) = pick {
            words.last = p.list;
            self.preset = Some(p.name);
            self.naming = None;
        }
        if let Some((name, renaming)) = &mut self.naming {
            let field = ui.add(egui::TextEdit::singleline(name).hint_text("Preset name"));
            if !field.has_focus() && name.is_empty() {
                field.request_focus();
            }
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut done = None;
            ui.horizontal(|ui| {
                let name = name.trim().to_owned();
                let save = ui.add_enabled(!name.is_empty(), egui::Button::new("Save"));
                if save.clicked() || (enter && !name.is_empty()) {
                    done = Some(Some(name));
                }
                if ui.button("Cancel").clicked() {
                    done = Some(None);
                }
            });
            match done {
                Some(Some(name)) => {
                    let renaming = *renaming;
                    self.naming = None;
                    save_preset(words, self.preset.take(), name.clone(), renaming);
                    self.preset = Some(name);
                }
                Some(None) => self.naming = None,
                None => {}
            }
            return;
        }
        let chosen = self.preset.clone();
        let stored = chosen
            .as_ref()
            .and_then(|n| words.presets.iter().position(|p| &p.name == n));
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save as preset…").clicked() {
                self.naming = Some((String::new(), false));
            }
            let (Some(name), Some(at)) = (chosen, stored) else {
                return;
            };
            let changed = words.presets.get(at).is_some_and(|p| p.list != words.last);
            if changed
                && ui.button(format!("Update “{name}”")).clicked()
                && let Some(p) = words.presets.get_mut(at)
            {
                p.list = words.last.clone();
            }
            if ui.button("Rename").clicked() {
                self.naming = Some((name.clone(), true));
            }
            if ui.button("Delete").clicked() {
                words.presets.remove(at);
                if words.instant.as_ref() == Some(&name) {
                    words.instant = None;
                }
                self.preset = None;
                return;
            }
            let mut quick = words.instant.as_ref() == Some(&name);
            ui.horizontal(|ui| {
                widgets::switch(ui, &mut quick, "Use for quick export");
                ui.label("Use for quick export");
            });
            let was = words.instant.as_ref() == Some(&name);
            if quick != was {
                words.instant = quick.then_some(name);
            }
        });
    }

    fn preview(&self, ui: &mut Ui, height: f32) {
        let t = Tokens::get(ui.ctx());
        let frame = Frame::new().inner_margin(egui::Margin::symmetric(12, 8));
        frame.show(ui, |ui| match (&self.progress, &self.shown) {
            (Err(why), _) => {
                ui.add_space(24.0);
                ui.label(RichText::new(why).color(t.muted));
            }
            (Ok(p), shown) if *p != Progress::Ready || shown.is_none() => {
                ui.add_space(24.0);
                ui.label(RichText::new(p.said()).color(t.muted));
                ui.add(egui::ProgressBar::new(p.fraction()).desired_width(260.0));
            }
            (_, Some(shown)) => {
                ScrollArea::both()
                    .id_salt("word-list-preview")
                    .max_height(height - 16.0)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        preview::show(ui, &shown.blocks);
                        if shown.cut {
                            let n = shown.made.words;
                            let note = format!(
                                "The preview stops here; Copy and Save hold all {n} words."
                            );
                            ui.label(RichText::new(note).color(t.muted).italics());
                        }
                    });
            }
            _ => {}
        });
    }

    fn footer(&self, ui: &mut Ui, files: &Files, out: &mut Commands) {
        let made = self.shown.as_ref().map(|s| &s.made);
        let ready = self.progress == Ok(Progress::Ready) && made.is_some();
        ui.horizontal_centered(|ui| {
            if let Some(made) = made.filter(|m| !m.missing.is_empty()) {
                let names: Vec<String> = made
                    .missing
                    .iter()
                    .map(|l| mareader_dict::language(l))
                    .collect();
                let note = format!("No {} dictionary yet", names.join(" or "));
                ui.label(RichText::new(note).color(Tokens::get(ui.ctx()).muted));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let ctx = ui.ctx().clone();
                let t = Tokens::get(&ctx);
                // Disabled, it fades like the buttons beside it.
                let primary = widgets::primary_button(ui, None, "Copy", t.accent, &t)
                    .min_size(vec2(84.0, 30.0));
                let copy = ui.add_enabled(ready, primary);
                let md = ui.add_enabled(
                    ready,
                    egui::Button::new("Save .md").min_size(vec2(0.0, 30.0)),
                );
                let epub = ui.add_enabled(
                    ready,
                    egui::Button::new("Save .epub").min_size(vec2(0.0, 30.0)),
                );
                let Some(made) = made else {
                    return;
                };
                let output = if copy.clicked() {
                    Some(WordOutput::Copy)
                } else if md.clicked() {
                    Some(WordOutput::Markdown)
                } else if epub.clicked() {
                    Some(WordOutput::Epub)
                } else {
                    None
                };
                if let Some(output) = output
                    && let Some(toast) = deliver(&ctx, files, &self.title, made, output)
                {
                    out.push(Sent::Ui(UiCommand::Notify(toast)));
                }
            });
        });
    }
}

/// Saves the options as `name`; renaming, `from` takes the new name.
fn save_preset(
    words: &mut mareader_settings::WordListSettings,
    from: Option<String>,
    name: String,
    renaming: bool,
) {
    if renaming && let Some(from) = from {
        if let Some(p) = words.presets.iter_mut().find(|p| p.name == from) {
            p.name = name.clone();
        }
        if words.instant.as_ref() == Some(&from) {
            words.instant = Some(name.clone());
        }
        let mut seen = false;
        words
            .presets
            .retain(|p| p.name != name || !std::mem::replace(&mut seen, true));
        return;
    }
    let list = words.last.clone();
    match words.presets.iter_mut().find(|p| p.name == name) {
        Some(p) => p.list = list,
        None => words.presets.push(WordPreset { name, list }),
    }
}

/// The languages in column order, each movable, and one more to add.
fn languages(ui: &mut Ui, list: &mut WordList, dict: &Dict, out: &mut Commands, close: &mut bool) {
    let t = Tokens::get(ui.ctx());
    let ready = dict.languages();
    let mut act = None;
    let n = list.languages.len();
    for (i, lang) in list.languages.iter().enumerate() {
        ui.horizontal(|ui| {
            let name = mareader_dict::language(lang);
            ui.label(format!("{}. {name}", i + 1));
            if !ready.contains(&lang.as_str()) {
                ui.label(
                    RichText::new("not downloaded")
                        .color(t.muted)
                        .size(t.text_small),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::small_icon_button(ui, Icon::Close, &format!("Remove {name}")).clicked()
                {
                    act = Some((i, None));
                }
                let down = ui.add_enabled_ui(i + 1 < n, |ui| {
                    widgets::small_icon_button(ui, Icon::MoveDown, &format!("Move {name} down"))
                });
                if down.inner.clicked() {
                    act = Some((i, Some(i + 1)));
                }
                let up = ui.add_enabled_ui(i > 0, |ui| {
                    widgets::small_icon_button(ui, Icon::MoveUp, &format!("Move {name} up"))
                });
                if up.inner.clicked() {
                    act = Some((i, Some(i.saturating_sub(1))));
                }
            });
        });
    }
    match act {
        Some((i, None)) if i < n => {
            list.languages.remove(i);
        }
        Some((i, Some(j))) if i < n && j < n => list.languages.swap(i, j),
        _ => {}
    }
    let left: Vec<&str> = (PACKS.iter())
        .map(|p| p.target)
        .filter(|l| !list.languages.iter().any(|x| x == l))
        .collect();
    if list.languages.len() < WORD_LANGUAGES && !left.is_empty() {
        ui.menu_button("Add language", |ui| {
            for lang in left {
                let mut name = mareader_dict::language(lang);
                if !ready.contains(&lang) {
                    name.push_str(" · not downloaded");
                }
                if ui.button(name).clicked() {
                    list.languages.push(lang.to_owned());
                    ui.close();
                }
            }
        });
    }
    if list.languages.is_empty() {
        ui.label(RichText::new("No languages: the list holds the words alone.").color(t.muted));
    }
    if ready.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Download a dictionary to add meanings.").color(t.muted));
            if ui.link("Dictionary settings").clicked() {
                out.push(Sent::Engine(Command::OpenSettings(Tab::Dictionary)));
                *close = true;
            }
        });
    }
}

/// A small heading over a group of options, with a muted note.
fn section(ui: &mut Ui, title: &str, note: &str) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(10.0);
    ui.label(
        RichText::new(title)
            .font(theme::medium(t.text_small))
            .color(t.muted),
    );
    if !note.is_empty() {
        ui.label(RichText::new(note).size(t.text_small).color(t.muted));
    }
}

/// A name and its switch, the switch at the right.
fn row(ui: &mut Ui, label: &str, on: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            widgets::switch(ui, on, label);
        });
    });
}

fn sort_label(s: WordSort) -> &'static str {
    match s {
        WordSort::Reading => "Reading",
        WordSort::AToZ => "A→Z",
        WordSort::ZToA => "Z→A",
        WordSort::Level => "Level",
    }
}

fn group_label(g: WordGroup) -> &'static str {
    match g {
        WordGroup::Chapter => "Chapter",
        WordGroup::Page => "Page",
        WordGroup::None => "None",
    }
}

fn layout_label(l: WordLayout) -> &'static str {
    match l {
        WordLayout::Table => "Table",
        WordLayout::List => "List",
    }
}

fn output_label(o: WordOutput) -> &'static str {
    match o {
        WordOutput::Copy => "Clipboard",
        WordOutput::Markdown => ".md file",
        WordOutput::Epub => ".epub file",
    }
}
