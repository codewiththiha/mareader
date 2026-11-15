//! The floating find bar: a capsule for the query, and its results card.

use std::ops::Range;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Context, CornerRadius, FontId, Frame, Id, Key, Layout, Margin, Order, Rect, Response,
    RichText, Sense, Stroke, StrokeKind, TextEdit, TextFormat, Ui, UiBuilder, pos2, vec2,
};
use mareader_search::{Hit, from_page, step};
use mareader_workspace::PaneId;

use crate::icons::Icon;
use crate::motion::entrance;
use crate::search::Found;
use crate::theme::{self, Tokens};
use crate::{loading_mark, widgets};

pub const PLACEHOLDER: &str = "Search in document…";
const WIDTH: f32 = 520.0;
/// The capsule's height; its ends are half of it round.
const BAR_H: f32 = 44.0;
/// A round button in the capsule.
const KNOB: f32 = 28.0;
/// Seconds the bar takes to rise into place, and how far it rises.
const ENTER: f32 = 0.18;
const RISE: f32 = 6.0;
/// The hit map's height: a tick per page holding a hit.
const MAP_H: f32 = 16.0;
/// From the pane's right edge, and below the title bar.
const INSET: f32 = 16.0;
/// Seconds after the last key before a query is asked, as mareader waits.
const SETTLE: f64 = 0.18;
/// Frames a hit's page has to bring its text before the reveal gives up.
const SEEK_FRAMES: u8 = 90;
const ROW_H: f32 = 52.0;
const LIST_MAX_H: f32 = 300.0;

/// One bar, on the pane it was opened over.
#[derive(Debug, Default)]
pub struct FindBar {
    pane: Option<PaneId>,
    /// Kept while closed, as mareader resumes its last query.
    query: String,
    focus: bool,
    /// When the query last changed, until it is asked.
    typed: Option<f64>,
    /// The query last asked, and of which document.
    asked: Option<(String, usize)>,
    /// The current hit.
    active: Option<usize>,
    /// An Enter waiting for its query's hits: forward or back.
    waiting: Option<bool>,
    list: bool,
    /// The hit being brought into view, and the frames left to try.
    seek: Option<(usize, u8)>,
    /// Where the bar was drawn last frame.
    rect: Option<Rect>,
    /// When it last came up, for its rise.
    shown_at: Option<f64>,
    /// The list scrolls to the current hit on its next frame.
    reveal: bool,
}

impl FindBar {
    pub fn open(&mut self, pane: PaneId) {
        if self.pane != Some(pane) {
            self.active = None;
            self.asked = None;
        }
        self.pane = Some(pane);
        self.focus = true;
    }

    pub fn close(&mut self) {
        self.pane = None;
        self.waiting = None;
        self.seek = None;
        self.list = false;
        self.shown_at = None;
    }

    pub fn pane(&self) -> Option<PaneId> {
        self.pane
    }

    /// Where the bar was drawn last frame, if open.
    pub fn rect(&self) -> Option<Rect> {
        self.rect.filter(|_| self.pane.is_some())
    }

    pub fn is_open(&self) -> bool {
        self.pane.is_some()
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn active(&self) -> Option<usize> {
        self.active
    }

    /// Replaces the query, as if typed.
    pub fn set_query(&mut self, query: &str, now: f64) {
        if self.query != query {
            self.query = query.to_owned();
            self.typed = Some(now);
        }
    }

    /// A query asked at once, landing on its first hit: for shots.
    pub fn seed(&mut self, query: &str) {
        self.query = query.to_owned();
        self.typed = None;
        self.waiting = Some(true);
    }

    /// Whether to ask now: settled after typing, or new to this document.
    pub fn due(&self, now: f64, key: usize) -> bool {
        if self.query.trim().is_empty() {
            return false;
        }
        let fresh = self.asked.as_ref() != Some(&(self.query.clone(), key));
        fresh && self.typed.is_none_or(|t| now - t >= SETTLE)
    }

    /// The query was asked of document `key`: its hits start anew.
    pub fn asked(&mut self, key: usize) {
        self.asked = Some((self.query.clone(), key));
        self.typed = None;
        self.active = None;
        self.seek = None;
    }

    /// The hit to bring into view and how many tries are left, if any.
    pub fn seeking(&mut self) -> Option<usize> {
        let (hit, left) = self.seek?;
        self.seek = left.checked_sub(1).map(|l| (hit, l));
        Some(hit)
    }

    pub fn sought(&mut self) {
        self.seek = None;
    }

    fn activate(&mut self, hit: usize) {
        self.active = Some(hit);
        self.seek = Some((hit, SEEK_FRAMES));
        self.reveal = true;
    }

    /// A step from the keys or buttons: through the hits, or once they come.
    fn step(&mut self, found: Option<&Found>, forward: bool, now: f64) {
        match found.filter(|f| !f.hits.is_empty() && self.active.is_some()) {
            Some(f) => {
                if let Some(next) = step(f.hits.len(), self.active, forward) {
                    self.activate(next);
                }
            }
            None => {
                self.waiting = Some(forward);
                // Asked at once, not after the pause.
                self.typed = self.typed.map(|_| now - SETTLE);
            }
        }
    }

    /// F3 or Cmd+G: as the bar's own step buttons.
    pub fn key_step(&mut self, found: Option<&Found>, forward: bool, now: f64) {
        if !self.query.trim().is_empty() {
            self.step(found, forward, now);
        }
    }

    /// A waiting step lands once there's a hit from `page` on, or all are in.
    pub fn settle(&mut self, found: Option<&Found>, page: usize) {
        let (Some(forward), Some(f)) = (self.waiting, found) else {
            return;
        };
        let pages: Vec<usize> = f.hits.iter().map(|h| h.page).collect();
        let ahead = pages.last().is_some_and(|&p| p >= page);
        if !(f.done() || forward && ahead) {
            return;
        }
        self.waiting = None;
        let Some(first) = from_page(&pages, page) else {
            return;
        };
        let hit = if forward {
            Some(first)
        } else {
            step(pages.len(), Some(first), false)
        };
        if let Some(hit) = hit {
            self.activate(hit);
        }
    }

    /// Top right of its pane; closes on a press elsewhere in a pane.
    pub fn show(
        &mut self,
        ctx: &Context,
        panes: &[(PaneId, Rect)],
        top: f32,
        found: Option<&Found>,
    ) {
        let Some(pane) = self.pane else {
            return;
        };
        let Some(rect) = panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r) else {
            self.close();
            return;
        };
        let now = ctx.input(|i| i.time);
        if let Some(t) = self.typed {
            ctx.request_repaint_after_secs((SETTLE - (now - t)).max(0.0) as f32);
        }
        let t = Tokens::get(ctx);
        let width = WIDTH.min(rect.width() - 2.0 * INSET).max(160.0);
        let risen = entrance(ctx, *self.shown_at.get_or_insert(now), now, ENTER);
        let y = top.max(rect.top()) + 12.0 - (1.0 - risen) * RISE;
        let at = pos2(rect.right() - INSET - width, y);
        let mut close = false;
        let area = egui::Area::new(Id::new("find-bar"))
            .fade_in(false)
            .order(Order::Foreground)
            .fixed_pos(at)
            .constrain_to(rect)
            .show(ctx, |ui| {
                ui.set_opacity(risen);
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 8.0;
                close = self.capsule(ui, &t, found, now);
                if self.list {
                    self.results(ui, &t, found);
                }
            });
        let bar = area.response.rect;
        self.rect = Some(bar);
        // Presses on chrome, such as the Search buttons, leave it be.
        let on_pane = |p: egui::Pos2| p.y > top && panes.iter().any(|(_, r)| r.contains(p));
        let pressed = ctx.input(|i| {
            let at = i.pointer.press_origin().filter(|_| i.pointer.any_pressed());
            at.is_some_and(|p| !bar.contains(p) && on_pane(p))
        });
        if close || pressed {
            self.close();
        }
    }

    /// The capsule: the mark, the borderless field, the tally and buttons.
    fn capsule(&mut self, ui: &mut Ui, t: &Tokens, found: Option<&Found>, now: f64) -> bool {
        let field_id = Id::new("find-field");
        let focused = ui.memory(|m| m.has_focus(field_id));
        let edge = if focused {
            t.accent.gamma_multiply(0.6)
        } else {
            t.line
        };
        let round = CornerRadius::same((BAR_H / 2.0) as u8);
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, edge))
            .corner_radius(round)
            .shadow(ui.ctx().global_style().visuals.window_shadow)
            .inner_margin(Margin {
                left: 14,
                right: 8,
                top: 0,
                bottom: 0,
            });
        let mut close = false;
        let shown = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(BAR_H - 2.0);
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                self.mark(ui, t, found, focused);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    close = knob(ui, t, Icon::Close, "Close search", false).clicked();
                    divider(ui, t);
                    let name = if self.list {
                        "Hide results"
                    } else {
                        "Show results"
                    };
                    if knob(ui, t, Icon::List, name, self.list).clicked() {
                        self.list = !self.list;
                        self.reveal = self.list;
                    }
                    let (back, next) = steps(ui, t);
                    if let Some((text, tone)) = self.tally(found) {
                        chip(ui, t, &text, tone);
                    }
                    let enter = self.field(ui, t, field_id, now);
                    let shift = ui.input(|i| i.modifiers.shift);
                    if enter || next.clicked() || back.clicked() {
                        let forward = if enter { !shift } else { next.clicked() };
                        self.step(found, forward, now);
                    }
                });
            });
        });
        if focused {
            // A soft halo round the capsule, in place of a field's box.
            let halo = Stroke::new(3.0, t.accent.gamma_multiply(0.18));
            let r = shown.response.rect.expand(1.5);
            let round = CornerRadius::same((r.height() / 2.0) as u8);
            ui.painter()
                .rect_stroke(r, round, halo, StrokeKind::Outside);
        }
        close
    }

    /// The magnifier, or the loading mark while pages are still read.
    fn mark(&self, ui: &mut Ui, t: &Tokens, found: Option<&Found>, focused: bool) {
        let (rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
        let asked = !self.query.trim().is_empty();
        let reading = asked && (found.is_none_or(|f| !f.done()) || self.typed.is_some());
        if reading {
            loading_mark::paint(ui, rect.shrink(3.0), t.accent);
            ui.ctx().request_repaint();
        } else {
            let tint = if focused { t.accent } else { t.muted };
            Icon::Search.image(ui, 17.0).tint(tint).paint_at(ui, rect);
        }
    }

    /// The query, without a box: the capsule is its frame. True on Enter.
    fn field(&mut self, ui: &mut Ui, t: &Tokens, id: Id, now: f64) -> bool {
        let before = self.query.clone();
        let hint = RichText::new(PLACEHOLDER).color(t.muted.gamma_multiply(0.8));
        let field = TextEdit::singleline(&mut self.query)
            .id(id)
            .frame(Frame::NONE)
            .margin(Margin::ZERO)
            .font(FontId::proportional(t.text + 1.0))
            .text_color(t.ink)
            .hint_text(hint)
            .desired_width(f32::INFINITY);
        let r = ui.add(field);
        widgets::name(&r, "Search in document");
        if self.query != before {
            self.typed = Some(now);
            self.waiting = None;
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if std::mem::take(&mut self.focus) || enter {
            r.request_focus();
        }
        enter
    }

    /// "3 of 57", "57 found" before a step, "No matches"; "…" while read.
    fn tally(&self, found: Option<&Found>) -> Option<(String, Tone)> {
        if self.query.trim().is_empty() {
            return None;
        }
        let f = found?;
        let n = f.hits.len();
        let more = if f.more { "+" } else { "" };
        let still = if f.done() { "" } else { "…" };
        Some(match self.active.filter(|&i| i < n) {
            _ if n == 0 && f.done() => ("No matches".into(), Tone::None),
            _ if n == 0 => return None,
            Some(i) => (format!("{} of {n}{more}{still}", i + 1), Tone::Some),
            None => (format!("{n}{more}{still} found"), Tone::Some),
        })
    }

    /// Every hit's page and words, under a map of where they fall.
    fn results(&mut self, ui: &mut Ui, t: &Tokens, found: Option<&Found>) {
        let frame = Frame::new()
            .fill(t.surface)
            .stroke(Stroke::new(1.0, t.line))
            .corner_radius(t.radius_lg)
            .shadow(ui.ctx().global_style().visuals.window_shadow)
            .inner_margin(Margin::same(8));
        frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 6.0;
            if self.query.trim().is_empty() {
                hint(ui, t);
                return;
            }
            let hits = found.map_or(&[][..], |f| f.hits.as_slice());
            let Some(f) = found.filter(|_| !hits.is_empty()) else {
                self.nothing(ui, t, found);
                return;
            };
            summary(ui, t, f);
            if let Some(hit) = map(ui, t, f, self.active) {
                self.activate(hit);
            }
            let picked = self.rows(ui, t, hits);
            if let Some(i) = picked {
                self.activate(i);
                self.list = false;
            }
        });
    }

    /// Searching still, or nothing found, said quietly.
    fn nothing(&self, ui: &mut Ui, t: &Tokens, found: Option<&Found>) {
        let done = found.is_some_and(Found::done);
        ui.horizontal(|ui| {
            ui.set_min_height(36.0);
            ui.add_space(4.0);
            if done {
                let words = format!("No matches for “{}”", self.query.trim());
                ui.label(RichText::new(words).color(t.muted));
            } else {
                let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                loading_mark::paint(ui, rect, t.accent);
                ui.label(RichText::new("Searching…").color(t.muted));
            }
        });
    }

    /// The rows, scrolling past a few; the one clicked, if any.
    fn rows(&mut self, ui: &mut Ui, t: &Tokens, hits: &[Hit]) -> Option<usize> {
        let mut picked = None;
        let needle = self.query.trim();
        // Inside an area the list would shrink to egui's 64 pt floor.
        ui.spacing_mut().item_spacing.y = 2.0;
        let tall = hits.len() as f32 * (ROW_H + 2.0) - 2.0;
        let mut list = egui::ScrollArea::vertical();
        if let Some(i) = self.active.filter(|_| std::mem::take(&mut self.reveal)) {
            // The current row, centred, wherever it was picked from.
            let top = i as f32 * (ROW_H + 2.0) - (LIST_MAX_H - ROW_H) / 2.0;
            list = list.vertical_scroll_offset(top.max(0.0));
        }
        list.max_height(LIST_MAX_H)
            .min_scrolled_height(tall.min(LIST_MAX_H))
            .auto_shrink([false, true])
            .show_rows(ui, ROW_H, hits.len(), |ui, rows| {
                for i in rows {
                    let Some(hit) = hits.get(i) else {
                        continue;
                    };
                    let on = self.active == Some(i);
                    if result_row(ui, t, (hit, needle), on).clicked() {
                        picked = Some(i);
                    }
                }
            });
        picked
    }
}

/// Before any query: what the list will hold, not a search.
fn hint(ui: &mut Ui, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        Icon::Search.paint(ui, rect, t.muted);
        let words = "Type a word to see every place it appears";
        ui.label(RichText::new(words).color(t.muted));
    });
}

/// A count's mood: something found, or nothing.
#[derive(Clone, Copy)]
enum Tone {
    Some,
    None,
}

/// A round, borderless button: washed on hover, accented while `on`.
fn knob(ui: &mut Ui, t: &Tokens, icon: Icon, label: &str, on: bool) -> Response {
    let (rect, r) = ui.allocate_exact_size(vec2(KNOB, KNOB), Sense::click());
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    let fill = match (on, r.is_pointer_button_down_on(), r.hovered()) {
        (true, ..) => Some(t.selected),
        (false, true, _) => Some(t.pressed),
        (false, false, true) => Some(t.hover),
        _ => None,
    };
    if let Some(fill) = fill {
        ui.painter().circle_filled(rect.center(), KNOB / 2.0, fill);
    }
    if r.has_focus() {
        let ring = Stroke::new(1.5, t.accent);
        ui.painter()
            .circle_stroke(rect.center(), KNOB / 2.0 - 0.5, ring);
    }
    let tint = if on {
        t.accent
    } else {
        t.ink.gamma_multiply(0.8)
    };
    let glyph = Rect::from_center_size(rect.center(), vec2(15.0, 15.0));
    icon.image(ui, 15.0).tint(tint).paint_at(ui, glyph);
    widgets::tip(&r, label, None);
    r
}

/// Back and on, joined in one pill; laid right to left, so on first.
fn steps(ui: &mut Ui, t: &Tokens) -> (Response, Response) {
    let (rect, _) = ui.allocate_exact_size(vec2(KNOB * 2.0 + 2.0, KNOB + 2.0), Sense::hover());
    let round = CornerRadius::same(255);
    ui.painter()
        .rect_filled(rect, round, t.ink.gamma_multiply(0.06));
    let inner = UiBuilder::new()
        .max_rect(rect.shrink(1.0))
        .layout(Layout::right_to_left(Align::Center));
    ui.scope_builder(inner, |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let next = knob(ui, t, Icon::ChevronDown, "Next match", false);
        let back = knob(ui, t, Icon::ChevronUp, "Previous match", false);
        (back, next)
    })
    .inner
}

/// A hairline between the steps and the bar's own buttons.
fn divider(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 18.0), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, t.line));
}

/// The count in a pill: accented, or warm when nothing is found.
fn chip(ui: &mut Ui, t: &Tokens, text: &str, tone: Tone) {
    let (fill, ink) = match tone {
        Tone::Some => (t.accent_soft, t.accent),
        Tone::None => (t.danger.gamma_multiply(0.14), t.danger),
    };
    let words = ui
        .painter()
        .layout_no_wrap(text.to_owned(), theme::medium(t.text_small), ink);
    let size = words.size() + vec2(20.0, 8.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, text));
    ui.painter()
        .rect_filled(rect, CornerRadius::same(255), fill);
    ui.painter()
        .galley(rect.center() - words.size() / 2.0, words, ink);
}

/// "12 matches · 4 pages", with "+" past the kept hits.
fn summary(ui: &mut Ui, t: &Tokens, f: &Found) {
    let n = f.hits.len();
    let mut pages: Vec<usize> = f.hits.iter().map(|h| h.page).collect();
    pages.dedup();
    let more = if f.more { "+" } else { "" };
    let noun = |k: usize, one: &str, many: &str| if k == 1 { one } else { many }.to_owned();
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        let lead = format!("{n}{more} {}", noun(n, "match", "matches"));
        ui.label(
            RichText::new(lead)
                .font(theme::medium(t.text_small))
                .color(t.ink),
        );
        let rest = format!("· {} {}", pages.len(), noun(pages.len(), "page", "pages"));
        ui.label(RichText::new(rest).size(t.text_small).color(t.muted));
    });
}

/// Where the hits fall through the book, a tick per page; a click goes.
fn map(ui: &mut Ui, t: &Tokens, f: &Found, active: Option<usize>) -> Option<usize> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, MAP_H), Sense::hover());
    let track = rect.shrink2(vec2(6.0, 0.0));
    let mid = track.center().y;
    let p = ui.painter();
    p.hline(track.x_range(), mid, Stroke::new(2.0, t.line));
    let span = f.pages.saturating_sub(1).max(1) as f32;
    let x_of = |page: usize| track.left() + track.width() * (page as f32 / span).min(1.0);
    let here = active.and_then(|i| f.hits.get(i)).map(|h| h.page);
    let mut went = None;
    let mut last = None;
    for (i, hit) in f.hits.iter().enumerate() {
        if last == Some(hit.page) {
            continue;
        }
        last = Some(hit.page);
        let x = x_of(hit.page);
        let tick = Rect::from_center_size(pos2(x, mid), vec2(2.0, 8.0));
        let id = ui.id().with(("find-tick", hit.page));
        let r = ui.interact(tick.expand2(vec2(3.0, 4.0)), id, Sense::click());
        let ink = if r.hovered() { t.ink } else { t.muted };
        ui.painter().rect_filled(tick, CornerRadius::same(1), ink);
        let page = hit.page + 1;
        r.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tick_name(page))
        });
        let r = r.on_hover_text(format!("Page {page}"));
        if r.clicked() {
            went = Some(i);
        }
    }
    if let Some(page) = here {
        // The hit shown: a dot riding the track in the accent.
        let at = pos2(x_of(page), mid);
        ui.painter().circle_filled(at, 4.5, t.accent);
        ui.painter()
            .circle_stroke(at, 4.5, Stroke::new(1.5, t.surface));
    }
    went
}

/// A map tick's name, for screen readers and tests.
pub fn tick_name(page: usize) -> String {
    format!("Go to page {page}")
}

/// A page chip, then two lines of words with the match marked.
fn result_row(ui: &mut Ui, t: &Tokens, (hit, needle): (&Hit, &str), on: bool) -> Response {
    let size = vec2(ui.available_width(), ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let label = format!("Page {}: {}", hit.page + 1, hit.snippet);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
    let painter = ui.painter_at(rect);
    if on || r.hovered() {
        let fill = if on { t.selected } else { t.hover };
        painter.rect_filled(rect, t.radius, fill);
    }
    if on {
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 10.0), vec2(3.0, ROW_H - 20.0));
        painter.rect_filled(bar, CornerRadius::same(2), t.accent);
    }
    // The page in a chip of its own.
    let number = format!("{}", hit.page + 1);
    let font = theme::medium(t.text_small - 1.0);
    let galley = painter.layout_no_wrap(number, font, if on { t.accent } else { t.ink });
    let chip_w = (galley.size().x + 14.0).max(30.0);
    let chip = Rect::from_min_size(
        pos2(rect.left() + 10.0, rect.center().y - 11.0),
        vec2(chip_w, 22.0),
    );
    let chip_fill = if on { t.surface } else { t.card };
    painter.rect_filled(chip, CornerRadius::same(6), chip_fill);
    painter.galley(chip.center() - galley.size() / 2.0, galley, t.ink);
    let left = chip.right() + 10.0;
    let job = snippet(t, &hit.snippet, needle, rect.right() - 10.0 - left, on);
    let words = ui.fonts_mut(|f| f.layout_job(job));
    let y = rect.center().y - words.size().y / 2.0;
    painter.galley(pos2(left, y), words, t.ink);
    r
}

/// The snippet over two lines at most, its match washed in the accent.
fn snippet(t: &Tokens, text: &str, needle: &str, width: f32, on: bool) -> LayoutJob {
    let body = FontId::proportional(t.text_small + 0.5);
    let plain = TextFormat::simple(body, if on { t.ink } else { t.muted });
    let marked = TextFormat {
        font_id: theme::medium(t.text_small + 0.5),
        color: t.ink,
        background: t.accent.gamma_multiply(0.22),
        ..TextFormat::default()
    };
    let mut job = LayoutJob::default();
    match marked_range(text, needle) {
        Some(m) => {
            job.append(text.get(..m.start).unwrap_or_default(), 0.0, plain.clone());
            job.append(text.get(m.clone()).unwrap_or_default(), 0.0, marked);
            job.append(text.get(m.end..).unwrap_or_default(), 0.0, plain);
        }
        None => job.append(text, 0.0, plain),
    }
    job.wrap = TextWrapping {
        max_width: width.max(40.0),
        max_rows: 2,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    job
}

/// Where `needle` sits in `text`, ignoring case: the match nearest the middle.
fn marked_range(text: &str, needle: &str) -> Option<Range<usize>> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let want: Vec<char> = needle.chars().map(fold).collect();
    if want.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let middle = text.len() / 2;
    let mut best: Option<Range<usize>> = None;
    let last = chars.len().checked_sub(want.len())?;
    for start in 0..=last {
        let Some(window) = chars.get(start..start + want.len()) else {
            continue;
        };
        let same = window.iter().zip(&want).all(|(&(_, c), &w)| fold(c) == w);
        if !same {
            continue;
        }
        let from = window.first().map_or(0, |&(b, _)| b);
        let to = chars
            .get(start + want.len())
            .map_or(text.len(), |&(b, _)| b);
        let nearer = |r: &Range<usize>| r.start.abs_diff(middle) > from.abs_diff(middle);
        if best.as_ref().is_none_or(nearer) {
            best = Some(from..to);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_match_is_found_in_any_case_nearest_the_middle() {
        let text = "…the Quick fox and the quick dog and the quick cat…";
        let m = marked_range(text, "QUICK").unwrap_or_default();
        assert_eq!(text.get(m.clone()), Some("quick"));
        assert!(m.start > 20 && m.start < 30, "the middle one: {m:?}");
        assert_eq!(marked_range("ÉCOLE", "école"), Some(0.."ÉCOLE".len()));
        assert_eq!(marked_range("abc", ""), None);
        assert_eq!(marked_range("abc", "z"), None);
    }
}
