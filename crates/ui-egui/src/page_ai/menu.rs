//! The page's menu and a bubble's: what to ask, and what to do after.

use super::{BubbleMenu, Ledger, Menu, PageAiUi, Step, label};
use crate::documents::DocumentsView;
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Context, CornerRadius, FontId, Frame, Id, Key, Margin, Modifiers, Order, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2, pos2, vec2,
};
use mareader_engine::command::Command;
use mareader_engine::page_ai::{Action, How, Slot};
use mareader_engine::settings::Tab;
use mareader_library::Library;
use mareader_settings::{AiSettings, LANGUAGES, PageStart, PageTask, language_name};
use mareader_workspace::PaneId;

/// The language search field, for screen readers and tests.
pub const FIND: &str = "Find a language";

const WIDTH: f32 = widgets::GROUPED_MENU_W;
const BUBBLE_WIDTH: f32 = widgets::MENU_W;
/// The language list's height before it scrolls.
const LIST_H: f32 = 260.0;

impl PageAiUi {
    /// After the panes: the open menu, over them.
    pub fn show(
        &mut self,
        ctx: &Context,
        (ledger, main): (&Ledger, &Library),
        ai: &AiSettings,
        (panes, documents): (&[(PaneId, Rect)], DocumentsView<'_>),
        out: &mut Commands,
    ) {
        let pressed = ctx.input(|i| {
            let p = i.pointer.press_origin();
            p.filter(|_| i.pointer.any_pressed())
        });
        let in_popup = pressed.is_some_and(|p| {
            ctx.memory(|m| {
                m.layer_ids()
                    .filter(|l| l.order >= Order::Foreground && m.areas().is_visible(l))
                    .filter_map(|l| m.area_rect(l.id))
                    .any(|r| r.contains(p))
            })
        });
        let outside = |r: Option<Rect>| {
            pressed.is_some_and(|p| !in_popup && !r.is_some_and(|r| r.contains(p)))
        };
        let gone = |pane: PaneId| !panes.iter().any(|(p, _)| *p == pane);
        if self.menu.as_ref().is_some_and(|m| gone(m.pane))
            || (self.menu.is_some() && outside(self.menu_rect))
        {
            self.menu = None;
        }
        let bubble = self.bubble_menu;
        if bubble.is_some_and(|m| gone(m.pane)) || (bubble.is_some() && outside(self.bubble_rect)) {
            self.bubble_menu = None;
        }
        let open = self.menu.is_some() || self.bubble_menu.is_some();
        if open && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.escape();
        }
        let t = Tokens::get(ctx);
        let now = ctx.input(|i| i.time);
        // The page's menu rides with its page, and goes once that scrolls off.
        let anchor = self.menu.as_ref().and_then(|m| {
            let pin = m.pin.as_ref()?;
            let sheet = documents.sheets(m.pane).iter().find(|s| s.page == m.page);
            let room = panes.iter().find(|(p, _)| *p == m.pane).map(|(_, r)| *r);
            let at = sheet.map(|s| s.rect.min + pin.point.to_vec2() * s.scale);
            let word = sheet.zip(pin.word).map(|(s, w)| s.to_screen(w));
            Some(
                at.filter(|at| room.is_some_and(|r| r.contains(*at)))
                    .map(|at| (at, word)),
            )
        });
        if let Some(None) = anchor {
            self.menu = None;
        }
        if let Some(m) = &mut self.menu {
            m.born.get_or_insert(now);
        }
        let anchor = anchor.flatten();
        let (menus, bubble) = (self.menu.clone(), self.bubble_menu);
        self.menu_rect = menus.map(|m| self.page_menu(ctx, ledger, m, anchor, (ai, &t), out));
        self.bubble_rect = bubble.map(|m| self.bubble_menu(ctx, (ledger, main), m, &t, out));
    }

    fn page_menu(
        &mut self,
        ctx: &Context,
        ledger: &Ledger,
        m: Menu,
        anchor: Option<(Pos2, Option<Rect>)>,
        (ai, t): (&AiSettings, &Tokens),
        out: &mut Commands,
    ) -> Rect {
        let mut close = false;
        let mut step = m.step.clone();
        let (at, word) = anchor.unwrap_or((m.at, None));
        let size = self
            .menu_rect
            .map_or(vec2(WIDTH + 12.0, 200.0), |r| r.size());
        let (pos, below) = place(at, word, size, ctx.content_rect());
        let now = ctx.input(|i| i.time);
        let k = ((now - m.born.unwrap_or(now)) / APPEAR).clamp(0.0, 1.0) as f32;
        if k < 1.0 {
            ctx.request_repaint();
        }
        let look = (pos, k, below);
        let rect = page_popup(ctx, Id::new("page-ai-menu"), look, t, |ui| {
            close = match step.clone() {
                Step::Tasks => self.tasks(ui, ledger, &m, ai, &mut step, out),
                Step::Languages => self.languages(ui, &m, ai, &mut step, out),
                Step::How(task, tag) => self.how(ui, &m, (task, tag), &mut step, out),
            };
        });
        match (close, &mut self.menu) {
            (true, _) => self.menu = None,
            (false, Some(menu)) => {
                if step == Step::Languages && menu.step != Step::Languages {
                    self.filter.clear();
                }
                menu.step = step;
            }
            (false, None) => {}
        }
        rect
    }

    /// The task as asked, or the next step when it should ask how.
    fn ask(
        m: &Menu,
        ai: &AiSettings,
        (task, tag): (PageTask, Option<String>),
        step: &mut Step,
        out: &mut Commands,
    ) -> bool {
        // Picked pages are asked as they are, in groups.
        if !m.pages.is_empty() {
            out.push(Sent::Engine(Command::PageAi(Action::RunPages {
                pane: m.pane,
                pages: m.pages.clone(),
                task,
                tag,
            })));
            return true;
        }
        let start = match task {
            PageTask::Summarize => PageStart::Page,
            PageTask::Translate => ai.tasks.translate.start,
            PageTask::Simplify => ai.tasks.simplify.start,
        };
        let how = |continuous| How {
            continuous,
            remember: false,
        };
        match start {
            PageStart::Ask => {
                *step = Step::How(task, tag);
                false
            }
            PageStart::Page => {
                out.push(Sent::Engine(Command::PageAi(action(
                    m,
                    task,
                    tag,
                    how(false),
                ))));
                true
            }
            PageStart::Continuous => {
                out.push(Sent::Engine(Command::PageAi(action(
                    m,
                    task,
                    tag,
                    how(true),
                ))));
                true
            }
        }
    }

    /// The tasks; true once one is picked.
    fn tasks(
        &self,
        ui: &mut Ui,
        ledger: &Ledger,
        m: &Menu,
        ai: &AiSettings,
        step: &mut Step,
        out: &mut Commands,
    ) -> bool {
        let t = Tokens::get(ui.ctx());
        if let Some(from) = m.aloud {
            // On a word it names it; off text, the page's start.
            let pinned = m.pin.as_ref().and_then(|p| p.text.as_deref());
            let row = aloud_row(ui, pinned.filter(|_| from.at.is_some()), &t);
            // Hovered, its word lights up more.
            let hot = row.hovered();
            ui.ctx()
                .data_mut(|d| d.insert_temp(super::aloud_hot(m.pane), hot));
            if row.clicked() {
                // A pin shows where it starts; resting, it reads.
                let page = mareader_speech::Spot {
                    page: m.page,
                    char: 0,
                };
                let spot = from.at.unwrap_or(page);
                out.push(Sent::Engine(Command::PlaceReadMarker {
                    pane: m.pane,
                    spot,
                }));
                return true;
            }
            widgets::menu_rule(ui);
        }
        let heading = match m.pages.len() {
            0 => format!("Ask AI · Page {}", m.page + 1),
            1 => "Ask AI · 1 page".to_owned(),
            n => format!("Ask AI · {n} pages"),
        };
        widgets::menu_section(ui, Some(Icon::Ai), &heading);
        if !PageTask::ALL
            .iter()
            .any(|task| ai.page_preset(*task).is_some())
        {
            let why = "Summarize, translate and simplify pages with an AI model.";
            ui.add_space(2.0);
            widgets::menu_note(ui, why);
            ui.add_space(2.0);
            if widgets::menu_row(ui, Icon::Settings, "Set up AI…", None, false).clicked() {
                // With no model saved, adding one comes first.
                let tab = if ai.presets.is_empty() {
                    Tab::Models
                } else {
                    Tab::Ai
                };
                out.push(Sent::Engine(Command::OpenSettings(tab)));
                return true;
            }
            return false;
        }
        let (pane, page, group) = (m.pane, m.page, !m.pages.is_empty());
        let what = match m.pages.len() {
            0 => "page".to_owned(),
            1 => "1 page".to_owned(),
            n => format!("{n} pages"),
        };
        // A task set to go on as one reads says so beside it.
        let hint = |s| (s == PageStart::Continuous && !group).then_some("Continuous");
        let summarize = format!("Summarize {what}");
        if widgets::menu_row(ui, Icon::Summarize, &summarize, None, false).clicked() {
            return Self::ask(m, ai, (PageTask::Summarize, None), step, out);
        }
        let translate_hint = hint(ai.tasks.translate.start);
        let mut recent: Vec<(String, String)> = ai
            .tasks
            .translate
            .recent
            .iter()
            .filter_map(|tag| language_name(tag).map(|name| (tag.clone(), name.to_owned())))
            .take(5)
            .collect();
        if let Some(tag) = ai.tasks.translate.language.as_deref()
            && !recent.iter().any(|(r, _)| r == tag)
            && let Some(name) = language_name(tag)
        {
            recent.insert(0, (tag.to_owned(), name.to_owned()));
        }
        if recent.is_empty() {
            let to = format!("Translate {what}…");
            if widgets::menu_row_chevron(ui, Icon::Translate, &to, translate_hint).clicked() {
                *step = Step::Languages;
            }
        } else {
            let current = ai.tasks.translate.language.as_deref();
            let current_name = current.and_then(language_name).or(translate_hint);
            let to = format!("Translate {what}");
            let mut chosen = None;
            let mut more = false;
            widgets::submenu_row(ui, Icon::Translate, &to, current_name, 188.0, |ui| {
                widgets::menu_section(ui, None, "Recent");
                for (tag, name) in &recent {
                    if widgets::menu_check_row(ui, name, current == Some(tag.as_str())).clicked() {
                        chosen = Some(tag.clone());
                        ui.close();
                    }
                }
                widgets::menu_rule(ui);
                if widgets::menu_row(ui, Icon::Translate, "More languages…", None, false).clicked()
                {
                    more = true;
                    ui.close();
                }
            });
            if let Some(tag) = chosen {
                return Self::ask(m, ai, (PageTask::Translate, Some(tag)), step, out);
            }
            if more {
                *step = Step::Languages;
                return false;
            }
        }
        let simplify = hint(ai.tasks.simplify.start);
        let label = format!("Simplify {what}");
        let simplify = widgets::menu_row(ui, Icon::Simplify, &label, simplify, false);
        if simplify
            .on_hover_text("Rewrites the text in plain English")
            .clicked()
        {
            return Self::ask(m, ai, (PageTask::Simplify, None), step, out);
        }
        if group {
            return false;
        }
        let shown = ledger.showing_on(pane, page).is_some();
        let following = ledger.following(pane);
        if shown || following.is_some() {
            widgets::menu_rule(ui);
        }
        if shown && widgets::menu_row(ui, Icon::Book, "Show original page", None, false).clicked() {
            out.push(Sent::Engine(Command::PageAi(Action::Show {
                pane,
                page,
                slot: None,
            })));
            return true;
        }
        if following.is_some()
            && widgets::menu_row(ui, Icon::Stop, "Stop continuous mode", None, false).clicked()
        {
            out.push(Sent::Engine(Command::PageAi(Action::StopFollow(pane))));
            return true;
        }
        false
    }

    /// This page, or on as the reader reads; true once one is picked.
    fn how(
        &mut self,
        ui: &mut Ui,
        m: &Menu,
        (task, tag): (PageTask, Option<String>),
        step: &mut Step,
        out: &mut Commands,
    ) -> bool {
        let t = Tokens::get(ui.ctx());
        let title = match (&task, tag.as_deref()) {
            (PageTask::Translate, Some(tag)) => {
                format!("Translate to {}", language_name(tag).unwrap_or(tag))
            }
            (PageTask::Translate, None) => "Translate".to_owned(),
            (PageTask::Simplify, _) => "Simplify".to_owned(),
            (PageTask::Summarize, _) => "Summarize".to_owned(),
        };
        if widgets::menu_row(ui, Icon::ChevronLeft, &title, None, false).clicked() {
            *step = Step::Tasks;
            return false;
        }
        widgets::menu_rule(ui);
        let remember = self.remember;
        let how = |continuous| How {
            continuous,
            remember,
        };
        let page = widgets::menu_row(ui, Icon::Book, "This page", None, false);
        if page.clicked() {
            out.push(Sent::Engine(Command::PageAi(action(
                m,
                task,
                tag,
                how(false),
            ))));
            return true;
        }
        let tip = "Does each page as you read on, a few pages ahead";
        let on = widgets::menu_row(ui, Icon::Repeat, "Continuous", None, false);
        if on.on_hover_text(tip).clicked() {
            out.push(Sent::Engine(Command::PageAi(action(
                m,
                task,
                tag,
                how(true),
            ))));
            return true;
        }
        widgets::menu_rule(ui);
        ui.horizontal(|ui| {
            ui.add_space(ui.spacing().button_padding.x);
            let label = RichText::new("Remember my choice").size(t.text_small);
            ui.checkbox(&mut self.remember, label)
                .on_hover_text("Change it later in Settings › AI");
        });
        ui.add_space(2.0);
        false
    }

    /// Every language, recent ones first; true once one is picked.
    fn languages(
        &mut self,
        ui: &mut Ui,
        m: &Menu,
        ai: &AiSettings,
        step: &mut Step,
        out: &mut Commands,
    ) -> bool {
        if widgets::menu_row(ui, Icon::ChevronLeft, "Translate to…", None, false).clicked() {
            *step = Step::Tasks;
            return false;
        }
        ui.add_space(2.0);
        let edit = TextEdit::singleline(&mut self.filter)
            .hint_text("Find a language")
            .desired_width(f32::INFINITY);
        let field = ui.add(edit);
        let name = |b: &mut egui::accesskit::Node| b.set_label(FIND);
        ui.ctx().accesskit_node_builder(field.id, name);
        if !field.has_focus() && self.filter.is_empty() {
            field.request_focus();
        }
        ui.add_space(2.0);
        let find = self.filter.trim().to_lowercase();
        let matches = |tag: &str, name: &str| {
            find.is_empty()
                || name.to_lowercase().contains(&find)
                || tag.to_lowercase().starts_with(&find)
        };
        let recent: Vec<&str> = match find.is_empty() {
            true => ai
                .tasks
                .translate
                .recent
                .iter()
                .map(String::as_str)
                .collect(),
            false => Vec::new(),
        };
        let all: Vec<(&str, &str)> = LANGUAGES
            .iter()
            .copied()
            .filter(|(tag, name)| matches(tag, name))
            .collect();
        let current = ai.tasks.translate.language.as_deref();
        let mut picked = None;
        if field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
            picked = all.first().map(|(tag, _)| (*tag).to_owned());
        }
        ScrollArea::vertical().max_height(LIST_H).show(ui, |ui| {
            for tag in &recent {
                let Some(name) = language_name(tag) else {
                    continue;
                };
                if widgets::menu_check_row(ui, name, current == Some(*tag)).clicked() {
                    picked = Some((*tag).to_owned());
                }
            }
            if !recent.is_empty() {
                widgets::menu_rule(ui);
            }
            for (tag, name) in &all {
                if widgets::menu_check_row(ui, name, current == Some(*tag)).clicked() {
                    picked = Some((*tag).to_owned());
                }
            }
            if all.is_empty() {
                widgets::menu_note(ui, "No language matches");
            }
        });
        let Some(tag) = picked else {
            return false;
        };
        Self::ask(m, ai, (PageTask::Translate, Some(tag)), step, out)
    }

    fn bubble_menu(
        &mut self,
        ctx: &Context,
        (ledger, main): (&Ledger, &Library),
        m: BubbleMenu,
        t: &Tokens,
        out: &mut Commands,
    ) -> Rect {
        let (pane, page, slot) = (m.pane, m.page, m.slot);
        let shown = ledger.showing_on(pane, page) == Some(slot);
        // What it is, who wrote it, and when; or why it failed.
        let (what, by, when, failed, busy) = match slot {
            Slot::Saved(id) => match ledger.saved(main, id) {
                Some(p) => (
                    label(&p.task),
                    p.by.clone(),
                    Some(mareader_engine::export::date(p.added)),
                    None,
                    false,
                ),
                None => {
                    self.bubble_menu = None;
                    return Rect::NOTHING;
                }
            },
            Slot::Run(id) => match ledger.runs.iter().find(|r| r.id == id) {
                Some(r) => (
                    label(&r.task),
                    r.by.clone(),
                    None,
                    r.failed.clone(),
                    r.failed.is_none(),
                ),
                None => {
                    self.bubble_menu = None;
                    return Rect::NOTHING;
                }
            },
        };
        let mut close = false;
        let rect = popup(ctx, Id::new("page-ai-bubble-menu"), m.at, t, |ui| {
            let mut act = |ui: &mut Ui, icon, text: &str, action| {
                if widgets::menu_row(ui, icon, text, None, false).clicked() {
                    out.push(Sent::Engine(Command::PageAi(action)));
                    close = true;
                }
            };
            if let Some(why) = &failed {
                ui.add(
                    egui::Label::new(RichText::new(why).size(t.text_small).color(t.danger)).wrap(),
                );
                act(
                    ui,
                    Icon::Rescan,
                    "Try again",
                    Action::Redo { pane, page, slot },
                );
                act(ui, Icon::Close, "Dismiss", Action::Delete(slot));
            } else if busy {
                match shown {
                    true => act(
                        ui,
                        Icon::Book,
                        "Show original page",
                        Action::Show {
                            pane,
                            page,
                            slot: None,
                        },
                    ),
                    false => act(
                        ui,
                        Icon::Show,
                        "Show as it comes",
                        Action::Show {
                            pane,
                            page,
                            slot: Some(slot),
                        },
                    ),
                }
                act(ui, Icon::Stop, "Stop", Action::Delete(slot));
            } else {
                match shown {
                    true => act(
                        ui,
                        Icon::Book,
                        "Show original page",
                        Action::Show {
                            pane,
                            page,
                            slot: None,
                        },
                    ),
                    false => act(
                        ui,
                        Icon::Show,
                        "Show this result",
                        Action::Show {
                            pane,
                            page,
                            slot: Some(slot),
                        },
                    ),
                }
                act(ui, Icon::Rescan, "Redo", Action::Redo { pane, page, slot });
                act(ui, Icon::Copy, "Copy text", Action::Copy(slot));
                if widgets::menu_row_danger(ui, Icon::Remove, "Delete").clicked() {
                    out.push(Sent::Engine(Command::PageAi(Action::Delete(slot))));
                    close = true;
                }
            }
            widgets::menu_rule(ui);
            let small = |s: String| RichText::new(s).size(t.text_small).color(t.muted);
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(ui.spacing().button_padding.x);
                ui.vertical(|ui| {
                    ui.label(small(what.clone()));
                    if !by.is_empty() {
                        ui.label(small(format!("By {by}")));
                    }
                    if let Some(when) = &when {
                        ui.label(small(format!("Made {when}")));
                    }
                });
            });
            ui.add_space(4.0);
        });
        if close {
            self.bubble_menu = None;
        }
        rect
    }
}

/// A menu's frame at `at`, kept on screen.
fn popup(ctx: &Context, id: Id, at: egui::Pos2, t: &Tokens, add: impl FnOnce(&mut Ui)) -> Rect {
    let rect = egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(at)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::menu(ui.style())
                .fill(t.paper)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    widgets::menu_style(ui, BUBBLE_WIDTH);
                    add(ui);
                    widgets::close_on_key(ui);
                });
        })
        .response
        .rect;
    // A menu summoned here: toasts give it the room.
    crate::toast::keep_clear(ctx, rect);
    rect
}

/// Seconds the page's menu takes to come in.
const APPEAR: f64 = 0.16;
/// Room kept between the menu, its word and the window's edges.
const GAP: f32 = 6.0;
const EDGE: f32 = 8.0;

/// Below its word, else above, else beside; never on it. True if below.
fn place(at: Pos2, word: Option<Rect>, size: Vec2, room: Rect) -> (Pos2, bool) {
    let keep = word.unwrap_or(Rect::from_center_size(at, Vec2::ZERO));
    let keep = keep.expand(GAP);
    let x = word.map_or(at.x, |w| w.left() - 4.0);
    let max = |lo: f32, hi: f32| hi.max(lo);
    let clamp_x = |x: f32| {
        x.clamp(
            room.left() + EDGE,
            max(room.left() + EDGE, room.right() - size.x - EDGE),
        )
    };
    let fits = |y: f32| y >= room.top() + EDGE && y + size.y <= room.bottom() - EDGE;
    let (below, above) = (keep.bottom(), keep.top() - size.y);
    if fits(below) {
        return (pos2(clamp_x(x), below), true);
    }
    if fits(above) {
        return (pos2(clamp_x(x), above), false);
    }
    // No room under or over: beside it, right if it fits, else left.
    let x = match keep.right() + size.x <= room.right() - EDGE {
        true => keep.right(),
        false => keep.left() - size.x,
    };
    let lo = room.top() + EDGE;
    let y = (keep.center().y - size.y * 0.5).clamp(lo, max(lo, room.bottom() - size.y - EDGE));
    (pos2(x.max(room.left() + EDGE), y), true)
}

/// The page's menu at `pos`, fading and sliding in as `k` goes to 1.
fn page_popup(
    ctx: &Context,
    id: Id,
    (pos, k, below): (Pos2, f32, bool),
    t: &Tokens,
    add: impl FnOnce(&mut Ui),
) -> Rect {
    let ease = 1.0 - (1.0 - k).powi(3);
    let rise = (1.0 - ease) * 6.0 * if below { -1.0 } else { 1.0 };
    let rect = egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(pos + vec2(0.0, rise))
        // Placed by hand: egui's own keeping-in would fight it.
        .constrain(false)
        .show(ctx, |ui| {
            ui.multiply_opacity(ease);
            Frame::menu(ui.style())
                .fill(t.paper)
                .stroke(Stroke::new(1.0, t.line))
                .corner_radius(t.radius_lg)
                .inner_margin(Margin::same(6))
                .show(ui, |ui| {
                    widgets::menu_style(ui, WIDTH);
                    add(ui);
                    widgets::close_on_key(ui);
                });
        })
        .response
        .rect;
    // A menu summoned here: toasts give it the room.
    crate::toast::keep_clear(ctx, rect);
    rect
}

/// Read aloud's row on the `30pt` menu grid, icon in the lit word's tone.
fn aloud_row(ui: &mut Ui, word: Option<&str>, t: &Tokens) -> egui::Response {
    let warm = crate::read_aloud::marker::warm(t);
    let font = FontId::proportional(13.5);
    let (label, job) = match word {
        Some(w) => {
            let w = short(w);
            let mut job = LayoutJob::default();
            job.append(
                "Read aloud from ",
                0.0,
                TextFormat::simple(font.clone(), t.ink),
            );
            job.append(
                &format!("“{w}”"),
                0.0,
                TextFormat {
                    font_id: font,
                    color: t.ink,
                    underline: Stroke::new(1.5, warm),
                    ..Default::default()
                },
            );
            (format!("Read aloud from “{w}”"), job)
        }
        None => {
            let label = "Read aloud from page start".to_owned();
            let mut job = LayoutJob::default();
            job.append(&label, 0.0, TextFormat::simple(font, t.ink));
            (label, job)
        }
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if resp.is_pointer_button_down_on() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), t.pressed);
    } else if resp.hovered() || resp.has_focus() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(8), t.hover);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let y = rect.center().y;
    let icon_rect = Rect::from_min_size(pos2(rect.left() + 8.0, y - 7.5), Vec2::splat(15.0));
    Icon::ReadAloud.paint(ui, icon_rect, warm);
    let text_x = rect.left() + 32.0;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let clip = Rect::from_min_max(
        pos2(text_x, rect.top()),
        pos2(rect.right() - 8.0, rect.bottom()),
    );
    ui.painter().with_clip_rect(clip).galley(
        pos2(text_x, y - galley.size().y * 0.5),
        galley,
        t.ink,
    );
    resp
}

/// A long word cut short, so the row keeps to one line.
fn short(word: &str) -> String {
    const MOST: usize = 16;
    match word.chars().count() > MOST {
        true => format!("{}…", word.chars().take(MOST - 1).collect::<String>()),
        false => word.to_owned(),
    }
}

/// The action a menu choice makes.
fn action(m: &Menu, task: PageTask, tag: Option<String>, how: How) -> Action {
    let (pane, page) = (m.pane, m.page);
    match (task, tag) {
        (PageTask::Translate, Some(tag)) => Action::Translate {
            pane,
            page,
            tag,
            how,
        },
        (task, _) => Action::Run {
            pane,
            page,
            task,
            how,
        },
    }
}
