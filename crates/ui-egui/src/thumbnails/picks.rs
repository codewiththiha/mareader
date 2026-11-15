//! Pages picked in the thumbnail grid, and the menus that act on them.

use egui::{Align2, CornerRadius, FontId, Id, Rect, Response, RichText, Sense, Ui, pos2, vec2};
use mareader_session::{DocId, Document};
use mareader_settings::{CefrLevel, PageTask, Settings, WordOutput, language_name};
use mareader_workspace::PaneId;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use crate::documents::Reading;
use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets::{self, menu_row};
use mareader_engine::command::Command;
use mareader_engine::page_ai::Action;
use mareader_engine::settings::Tab;

/// What the page AI and vocabulary tools offer picked pages this frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Offer {
    /// Some page task has a model.
    pub any: bool,
    /// The language Translate goes to, by tag and name.
    pub language: Option<(String, String)>,
    /// Valid recently used translation languages, most recent first.
    pub recent: Vec<(String, String)>,
    /// What the quick word list does, and the preset it uses.
    pub quick: (WordOutput, Option<String>),
    /// Fallback CEFR threshold badge (`B2+`) when no custom preset is active.
    pub level_tag: String,
}

impl Offer {
    pub fn of(settings: &Settings) -> Self {
        let ai = &settings.ai;
        let words = &settings.study.words;
        let level = settings.study.cefr.level;
        let level_tag = match level {
            CefrLevel::C2 => "C2".to_owned(),
            other => format!("{}+", other.label()),
        };
        let language = ai.tasks.translate.language.as_deref().and_then(|tag| {
            let name = language_name(tag)?;
            Some((tag.to_owned(), name.to_owned()))
        });
        Self {
            any: PageTask::ALL.iter().any(|t| ai.page_preset(*t).is_some()),
            language,
            recent: ai
                .tasks
                .translate
                .recent
                .iter()
                .filter_map(|tag| language_name(tag).map(|name| (tag.clone(), name.to_owned())))
                .take(5)
                .collect(),
            quick: (words.instant().output, words.instant.clone()),
            level_tag,
        }
    }
}

/// A still press this long on a page picks it, as in the library.
const LONG_PRESS: f64 = 0.45;

/// The pages picked, in the document they were counted in.
#[derive(Default, Clone)]
pub struct Picks {
    /// The pane and document the pages belong to.
    owner: Option<(PaneId, DocId)>,
    /// That document, to find picked pages again; held only while some are.
    counted: Option<Arc<Document>>,
    pages: BTreeSet<usize>,
    /// Where a Shift-click range starts.
    anchor: Option<usize>,
}

impl fmt::Debug for Picks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Picks")
            .field("owner", &self.owner)
            .field("pages", &self.pages)
            .finish_non_exhaustive()
    }
}

impl Picks {
    /// Follows the pane read; the same pages stay picked if they still are.
    pub fn follow(&mut self, reading: &Reading) {
        let here = (reading.pane, reading.doc);
        if self.owner == Some(here) {
            return;
        }
        let same_pane = self.owner.is_some_and(|(pane, _)| pane == reading.pane);
        let was = self.counted.take();
        // Results laid in or out move pages; new type or a new book ends it.
        self.pages = match (same_pane, was) {
            (true, Some(was)) => (self.pages.iter())
                .filter_map(|p| was.same_page(*p, &reading.document))
                .collect(),
            _ => BTreeSet::new(),
        };
        self.anchor = self
            .anchor
            .and_then(|a| self.pages.contains(&a).then_some(a));
        self.owner = Some(here);
        self.counted = Some(Arc::clone(&reading.document));
    }

    /// Ends a frame: the book stays held only while pages are picked.
    pub fn settle(&mut self, reading: &Reading) {
        self.counted = self.active().then(|| Arc::clone(&reading.document));
    }

    pub fn active(&self) -> bool {
        !self.pages.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn contains(&self, page: usize) -> bool {
        self.pages.contains(&page)
    }

    /// The pages in reading order.
    pub fn pages(&self) -> Vec<usize> {
        self.pages.iter().copied().collect()
    }

    /// Compact 1-based page summary (`p. 4`, `pp. 12–14`, `pp. 2, 5–7`).
    pub fn range_label(&self) -> String {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for &p in &self.pages {
            let one = p + 1;
            match runs.last_mut() {
                Some((_, end)) if one == *end + 1 => *end = one,
                _ => runs.push((one, one)),
            }
        }
        match runs.as_slice() {
            [] => String::new(),
            [(only, end)] if only == end => format!("p. {only}"),
            [(start, end)] => format!("pp. {start}–{end}"),
            [a, b] => {
                let fmt = |(s, e): &(usize, usize)| {
                    if s == e {
                        s.to_string()
                    } else {
                        format!("{s}–{e}")
                    }
                };
                format!("pp. {}, {}", fmt(a), fmt(b))
            }
            many => {
                let first = many.first().map_or(1, |(s, _)| *s);
                let last = many.last().map_or(first, |(_, e)| *e);
                format!("pp. {first}–{last}")
            }
        }
    }

    pub fn toggle(&mut self, page: usize) {
        if !self.pages.remove(&page) {
            self.pages.insert(page);
        }
        self.anchor = Some(page);
    }

    /// Adds every page from the last one picked to `page`.
    pub fn extend_to(&mut self, page: usize) {
        let from = self.anchor.unwrap_or(page);
        self.pages.extend(from.min(page)..=from.max(page));
        self.anchor = Some(page);
    }

    pub fn has_anchor(&self) -> bool {
        self.anchor.is_some()
    }

    pub fn all(&mut self, count: usize) {
        self.pages = (0..count).collect();
    }

    /// Lets go of every page; false if none was picked.
    pub fn clear(&mut self) -> bool {
        self.anchor = None;
        self.counted = None;
        !std::mem::take(&mut self.pages).is_empty()
    }
}

/// A still press held on page `page` picks it; true on its release.
pub fn long_press(ui: &Ui, resp: &Response, page: usize, picks: &mut Picks) -> bool {
    let key = Id::new("thumb-long-press");
    let (now, start, pressed, released, moved) = ui.input(|i| {
        let p = &i.pointer;
        (
            i.time,
            p.press_start_time(),
            p.any_pressed(),
            p.any_released(),
            p.is_decidedly_dragging(),
        )
    });
    // A new press forgets the last one.
    if pressed {
        ui.data_mut(|d| d.remove::<usize>(key));
    }
    let fired = ui.data(|d| d.get_temp::<usize>(key)) == Some(page);
    if released {
        if fired {
            ui.data_mut(|d| d.remove::<usize>(key));
        }
        return fired && resp.clicked();
    }
    // A press that wanders is no long press.
    let held = resp.is_pointer_button_down_on() && !moved;
    let Some(start) = start.filter(|_| held && !fired) else {
        return false;
    };
    let left = LONG_PRESS - (now - start);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32);
        return false;
    }
    ui.data_mut(|d| d.insert_temp(key, page));
    if !picks.contains(page) {
        picks.toggle(page);
    }
    false
}

/// A click while picking, or with a modifier; false to go to the page.
pub fn click(ui: &Ui, page: usize, picks: &mut Picks) -> bool {
    let m = ui.input(|i| i.modifiers);
    match () {
        _ if m.shift && picks.has_anchor() => picks.extend_to(page),
        _ if m.command || m.shift || picks.active() => picks.toggle(page),
        _ => return false,
    }
    true
}

/// "3 pages selected", with All and Clear, over the grid.
pub fn bar(ui: &mut Ui, picks: &mut Picks, count: usize, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        let n = picks.len();
        let words = match n {
            1 => "1 page selected".to_owned(),
            n => format!("{n} pages selected"),
        };
        ui.label(RichText::new(words).size(t.text_small).color(t.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(8.0);
            if ui.small_button("Clear").clicked() {
                picks.clear();
            }
            if n < count && ui.small_button("All").clicked() {
                picks.all(count);
            }
        });
    });
}

/// The menu of the picked pages.
pub fn picks_menu(
    ui: &mut Ui,
    (pane, offer): (PaneId, &Offer),
    picks: &mut Picks,
    count: usize,
    t: &Tokens,
    out: &mut Commands,
) {
    header(ui, picks, t);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        let width = ((ui.available_width() - 10.0) / 3.0).max(1.0);
        if tile(ui, Icon::Copy, "Copy text", width, t).clicked() {
            out.push(Sent::Engine(Command::CopyPages {
                pane,
                pages: picks.pages(),
            }));
            ui.close();
        }
        if tile(ui, Icon::Note, "Word list", width, t).clicked() {
            out.push(Sent::Engine(Command::WordListWindow {
                pane,
                pages: picks.pages(),
            }));
            ui.close();
        }
        if tile(ui, Icon::SelectSeveral, "Select all", width, t).clicked() {
            picks.all(count);
            ui.close();
        }
    });
    ui.add_space(2.0);
    widgets::menu_section(ui, None, "Vocabulary");
    let quick = match offer.quick.0 {
        WordOutput::Copy => "Copy word list",
        WordOutput::Markdown => "Save word list (.md)",
        WordOutput::Epub => "Save word list (.epub)",
    };
    let badge = offer.quick.1.as_deref().unwrap_or(offer.level_tag.as_str());
    let words = widgets::menu_row_tag(ui, Icon::Levels, quick, Some(badge), false);
    if words
        .on_hover_text("The words above your level, with their dictionary meanings")
        .clicked()
    {
        out.push(Sent::Engine(Command::WordList {
            pane,
            pages: picks.pages(),
        }));
        ui.close();
    }
    if menu_row(ui, Icon::ViewOptions, "Word list…", None, false).clicked() {
        out.push(Sent::Engine(Command::WordListWindow {
            pane,
            pages: picks.pages(),
        }));
        ui.close();
    }
    widgets::menu_section(ui, Some(Icon::Ai), "Ask AI");
    ai_rows(ui, (pane, offer), picks, out);
}

fn header(ui: &mut Ui, picks: &mut Picks, t: &Tokens) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let y = row.center().y;
    let n = picks.len();
    let heading = match n {
        1 => "1 page".to_owned(),
        n => format!("{n} pages"),
    };
    let head = ui
        .painter()
        .layout_no_wrap(heading, theme::medium(13.0), t.ink);
    let head_x = row.left() + 8.0;
    ui.painter()
        .galley(pos2(head_x, y - head.size().y * 0.5), head.clone(), t.ink);
    let range = picks.range_label();
    if !range.is_empty() {
        ui.painter().text(
            pos2(head_x + head.size().x + 6.0, y),
            Align2::LEFT_CENTER,
            range,
            FontId::proportional(11.5),
            t.muted,
        );
    }

    let clear_text =
        ui.painter()
            .layout_no_wrap("Clear".to_owned(), FontId::proportional(11.5), t.muted);
    let cap_font = FontId::proportional(10.5);
    let esc_w = ui
        .painter()
        .layout_no_wrap("Esc".to_owned(), cap_font, t.muted)
        .size()
        .x
        + 10.0;
    let btn_w = 6.0 + clear_text.size().x + 5.0 + esc_w + 6.0;
    let btn = Rect::from_min_size(pos2(row.right() - btn_w, y - 12.0), vec2(btn_w, 24.0));
    let clear = ui.interact(btn, ui.id().with("picks-clear"), Sense::click());
    clear.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Clear selection")
    });
    if clear.hovered() || clear.has_focus() {
        let fill = if clear.is_pointer_button_down_on() {
            t.pressed
        } else {
            t.hover
        };
        ui.painter().rect_filled(btn, CornerRadius::same(6), fill);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let clear_ink = if clear.hovered() || clear.has_focus() {
        t.ink
    } else {
        t.muted
    };
    ui.painter().text(
        pos2(btn.left() + 6.0, y),
        Align2::LEFT_CENTER,
        "Clear",
        FontId::proportional(11.5),
        clear_ink,
    );
    widgets::paint_keycap(ui, btn.right() - 4.0, y, "Esc", t);
    if clear.clicked() {
        picks.clear();
        ui.close();
    }
}

/// A frequent action, with a single accessible name and a generous target.
fn tile(ui: &mut Ui, icon: Icon, label: &str, width: f32, t: &Tokens) -> Response {
    // Keep hover motion when this tile is used outside its context menu.
    let _act = anim::announce(ui, Act::Hover);
    let (rect, r) = ui.allocate_exact_size(vec2(width, 54.0), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let rest = t
        .paper
        .lerp_to_gamma(t.ink, if t.dark { 0.055 } else { 0.04 });
    let fill = if r.is_pointer_button_down_on() {
        t.pressed
    } else if r.hovered() || r.has_focus() {
        t.hover
    } else {
        rest
    };
    ui.painter().rect_filled(rect, CornerRadius::same(10), fill);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let icon_tint = if r.hovered() || r.has_focus() {
        t.ink
    } else {
        t.ink.gamma_multiply(0.88)
    };
    let icon_rect = Rect::from_center_size(
        pos2(rect.center().x, rect.center().y - 9.0),
        vec2(16.0, 16.0),
    );
    anim::paint(ui, icon_rect, icon, icon_tint, label, &r);
    ui.painter().text(
        pos2(rect.center().x, rect.center().y + 11.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        t.ink,
    );
    r
}

/// Summarize, translate and simplify the picked pages, a group at a time.
fn ai_rows(ui: &mut Ui, (pane, offer): (PaneId, &Offer), picks: &Picks, out: &mut Commands) {
    if !offer.any {
        if menu_row(ui, Icon::Settings, "Set up AI…", None, false).clicked() {
            out.push(Sent::Engine(Command::OpenSettings(Tab::Ai)));
            ui.close();
        }
        return;
    }
    let run = |task, tag| {
        let pages = picks.pages();
        Sent::Engine(Command::PageAi(Action::RunPages {
            pane,
            pages,
            task,
            tag,
        }))
    };
    if menu_row(ui, Icon::Summarize, "Summarize", None, false).clicked() {
        out.push(run(PageTask::Summarize, None));
        ui.close();
    }
    let current_name = offer.language.as_ref().map(|(_, name)| name.as_str());
    widgets::submenu_row(
        ui,
        Icon::Translate,
        "Translate",
        current_name,
        188.0,
        |ui| {
            if !offer.recent.is_empty() {
                widgets::menu_section(ui, None, "Recent");
                for (tag, name) in &offer.recent {
                    let on = offer
                        .language
                        .as_ref()
                        .is_some_and(|(selected, _)| selected == tag);
                    if widgets::menu_check_row(ui, name, on).clicked() {
                        out.push(run(PageTask::Translate, Some(tag.clone())));
                        ui.close();
                    }
                }
                widgets::menu_rule(ui);
            }
            if menu_row(ui, Icon::Translate, "More languages…", None, false).clicked() {
                let (at, pages) = (ui.max_rect().right_top(), picks.pages());
                let at = [at.x, at.y];
                out.push(Sent::Engine(Command::PageAi(Action::PagesMenu {
                    pane,
                    pages,
                    at,
                })));
                ui.close();
            }
        },
    );
    let simplify = menu_row(ui, Icon::Simplify, "Simplify", None, false);
    if simplify
        .on_hover_text("Rewrites the text in plain English")
        .clicked()
    {
        out.push(run(PageTask::Simplify, None));
        ui.close();
    }
}

/// The menu of one page, picked or not.
pub fn page_menu(ui: &mut Ui, pane: PaneId, page: usize, picks: &mut Picks, out: &mut Commands) {
    if menu_row(ui, Icon::Copy, "Copy page text", None, false).clicked() {
        let pages = vec![page];
        out.push(Sent::Engine(Command::CopyPages { pane, pages }));
        ui.close();
    }
    if menu_row(ui, Icon::SelectSeveral, "Select", None, false).clicked() {
        picks.toggle(page);
        ui.close();
    }
}
