//! Settings search: a local catalog, results and jumps to existing controls.

mod index;
use egui::{Align, Frame, Id, Margin, RichText, Ui, vec2};
use mareader_engine::settings::Tab;

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

pub(super) use index::Entry;

/// The catalog is normalized once, not rebuilt by drawing a settings page.
pub(super) fn find(query: &str, allowed: impl Fn(Tab) -> bool) -> Vec<Entry> {
    index::find(query, allowed)
}

pub(super) fn field(ui: &mut Ui, query: &mut String) -> egui::Response {
    widgets::input(query)
        .id(Id::new("settings-search-field"))
        .icon(Icon::Search)
        .hint("Search settings")
        .label("Search settings")
        .clearable(true)
        .compact()
        .show(ui)
        .response
}

pub(super) fn scroll(tab: Tab, query: &str, room: f32, changed: bool) -> egui::ScrollArea {
    let searching = !query.trim().is_empty();
    let id = if searching { "search" } else { tab.label() };
    let mut scroll = egui::ScrollArea::vertical()
        .id_salt(("settings-body", id))
        .max_height(room)
        .auto_shrink(false);
    if searching && changed {
        scroll = scroll.vertical_scroll_offset(0.0);
    }
    scroll
}

pub(super) fn results(ui: &mut Ui, hits: &[Entry]) -> Option<Entry> {
    let t = Tokens::get(ui.ctx());
    let mut chosen = None;
    if hits.is_empty() {
        ui.add_space(16.0);
        ui.label(RichText::new("No settings found. Try another search.").color(t.muted));
        return None;
    }
    for entry in hits {
        let title = format!("{} · {}", entry.title, entry.tab.label());
        let width = (ui.available_width() - 24.0).max(10.0);
        let main = ui.painter().layout(
            entry.title.to_owned(),
            crate::theme::medium(t.text),
            t.ink,
            width,
        );
        let detail = format!("{}  ›  {}", entry.tab.label(), entry.section);
        let sub = ui.painter().layout(
            detail,
            egui::FontId::proportional(t.text_small),
            t.muted,
            width,
        );
        let height = (main.size().y + sub.size().y + 19.0).max(48.0);
        let (rect, r) =
            ui.allocate_exact_size(vec2(ui.available_width(), height), egui::Sense::click());
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
        let fill = if r.hovered() || r.has_focus() {
            t.hover
        } else {
            t.card
        };
        ui.painter().rect_filled(rect, t.radius, fill);
        ui.painter().rect_stroke(
            rect,
            t.radius,
            egui::Stroke::new(1.0, t.line),
            egui::StrokeKind::Inside,
        );
        let first = rect.min + vec2(12.0, 8.0);
        let second = first + vec2(0.0, main.size().y + 3.0);
        ui.painter().galley(first, main, t.ink);
        ui.painter().galley(second, sub, t.muted);
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if r.clicked() {
            chosen = Some(*entry);
        }
        ui.add_space(5.0);
    }
    chosen
}

/// A jump lasts one complete layout pass; rows take priority over sections.
#[derive(Clone)]
struct Jump {
    entry: Entry,
    row: Option<egui::Rect>,
    section: Option<egui::Rect>,
}
const JUMP: &str = "settings-search-jump";

pub(super) fn begin(ctx: &egui::Context, entry: Entry) {
    ctx.data_mut(|d| {
        d.insert_temp(
            Id::new(JUMP),
            Jump {
                entry,
                row: None,
                section: None,
            },
        )
    });
}

pub(super) fn finish(ui: &Ui) -> bool {
    let jump = ui.data_mut(|d| {
        let jump = d.get_temp::<Jump>(Id::new(JUMP));
        d.remove::<Jump>(Id::new(JUMP));
        jump
    });
    let Some(jump) = jump else { return false };
    let actual =
        jump.row.is_some() || (jump.entry.title == jump.entry.section && jump.section.is_some());
    if let Some(rect) = jump.row.or(jump.section) {
        ui.scroll_to_rect(rect, Some(Align::Center));
        let name = if jump.row.is_some() {
            jump.entry.title
        } else {
            jump.entry.section
        };
        let id = Id::new(("settings-search-result", name));
        let now = ui.input(|i| i.time);
        ui.data_mut(|d| d.insert_temp(id.with("glow"), now));
        ui.ctx().request_repaint();
    }
    actual
}

/// Record a row or fallback section without affecting the control itself.
pub(super) fn touch(ui: &Ui, name: &str, response: &egui::Response, section: bool) {
    let id = Id::new(JUMP);
    let Some(mut jump) = ui.data(|d| d.get_temp::<Jump>(id)) else {
        return;
    };
    if section && name == jump.entry.section && jump.section.is_none() {
        jump.section = Some(response.rect);
    } else if !section && name == jump.entry.title && jump.row.is_none() {
        jump.row = Some(response.rect);
    } else {
        return;
    }
    ui.data_mut(|d| d.insert_temp(id, jump));
}

/// Maintain a short cue after the jump without scrolling again.
pub(super) fn cue(ui: &Ui, name: &str, rect: egui::Rect) {
    let id = Id::new(("settings-search-result", name));
    super::glow(ui, id, rect, ui.input(|i| i.time));
}

pub(super) fn conditional_note(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    Frame::new().inner_margin(Margin { left: 24, right: 24, top: 0, bottom: 10 }).show(ui, |ui| {
        ui.label(RichText::new("Some options appear only when their feature, download or reading context is available.").color(t.muted).small());
    });
}

impl super::SettingsWindow {
    pub(super) fn search_page(
        &mut self,
        ui: &mut Ui,
        (tab, draft, cx, services, out): (
            Tab,
            &mut mareader_settings::Settings,
            &mareader_engine::scope::Scope,
            &super::Services<'_>,
            &mut crate::ui_command::Commands,
        ),
    ) {
        let jump = self.jump.take().filter(|entry| entry.tab == tab);
        if let Some(entry) = jump {
            begin(ui.ctx(), entry);
        }
        super::tab_body(ui, tab, draft, cx, services, out);
        if jump.is_some() && !finish(ui) {
            self.conditional = true;
            ui.ctx().request_repaint();
        }
    }

    pub(super) fn search_enter(&self, ctx: &egui::Context) -> bool {
        !self.query.is_empty()
            && ctx.memory(|m| m.has_focus(Id::new("settings-search-field")))
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
    }

    pub(super) fn search_hits(&mut self, ctx: &egui::Context) -> Vec<Entry> {
        if !self.query.is_empty()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.query.clear();
        }
        find(&self.query, |tab| self.lists(tab))
    }

    pub(super) fn search_results(&mut self, ui: &mut Ui, hits: &[Entry], enter: bool) {
        let selected = results(ui, hits).or_else(|| enter.then(|| hits.first().copied()).flatten());
        if let Some(entry) = selected {
            self.tab = entry.tab;
            self.query.clear();
            self.conditional = entry.conditional;
            self.jump = Some(entry);
            ui.ctx().request_repaint();
        }
    }
}
