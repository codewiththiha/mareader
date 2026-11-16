//! Search: the bar's field, its suggestions, and the card a choice lights.

use super::Covers;
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use crate::z_order::{self, Tier};
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Color32, Context, EventFilter, FontId, Id, Key, Modifiers, Rect, Sense, Stroke, StrokeKind,
    TextFormat, Ui, pos2, vec2,
};
use mareader_engine::command::Command;
use mareader_engine::library::{Action, Field, RING_END, RING_HOLD, SearchAction};
use mareader_engine::session::Session;
use mareader_library::{Library, Suggestion, Target};
use std::ops::Range;

/// The field's id: the app asks it whether it holds the keys.
pub const FIELD: &str = "library-search";
pub const LABEL: &str = "Search the library";
/// Set to focus the field once it's next drawn.
const FOCUS_ASK: &str = "library-search-focus";
const PANEL: &str = "library-suggestions";
const ROW_H: f32 = 48.0;
/// Each halo grows [`HALO`] points out over a pulse.
const PULSE: f64 = 1.6;
const HALO: f32 = 16.0;

/// Focuses the field when it's next drawn, all of it selected.
pub fn focus(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new(FOCUS_ASK), true));
}

/// Lets go of the keys: a choice was made.
pub fn blur(ctx: &Context) {
    ctx.memory_mut(|m| m.surrender_focus(Id::new(FIELD)));
}

/// Whether the field holds the keys, as of last frame.
pub fn focused(ctx: &Context) -> bool {
    ctx.memory(|m| m.has_focus(Id::new(FIELD)))
}

fn act(out: &mut Commands, action: SearchAction) {
    out.push(Sent::Engine(Command::Library(Action::Search(action))));
}

/// The bar's pill in `rect`; arrows, Enter and Escape act before typing.
pub fn field(ui: &mut Ui, rect: Rect, field: &Field, out: &mut Commands) {
    let id = Id::new(FIELD);
    let focused = ui.memory(|m| m.has_focus(id));
    if focused {
        keys(ui, field, out);
    }
    let mut text = field.text.clone();
    // Escape and the arrows are the field's own, not focus moves.
    let filter = EventFilter {
        horizontal_arrows: true,
        vertical_arrows: true,
        escape: true,
        ..EventFilter::default()
    };
    let res = ui
        .scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            widgets::input(&mut text)
                .id(id)
                .icon(Icon::Search)
                .hint(&field.placeholder)
                .label(LABEL)
                .clearable(true)
                .pill()
                .width(rect.width())
                .height(rect.height())
                .text_size(13.0)
                .event_filter(filter)
                .show(ui)
        })
        .inner;
    if ui.data_mut(|d| d.remove_temp::<bool>(Id::new(FOCUS_ASK))) == Some(true) {
        super::crumbs::select_all(&res.response, id, &text);
    }
    // Typing, or coming back to a search, lists what it finds.
    let back = res.response.gained_focus() && !text.trim().is_empty();
    if res.cleared {
        act(out, SearchAction::Clear);
    } else if res.response.changed() || back {
        act(out, SearchAction::Text(text));
    }
    let pass = ui.ctx().cumulative_pass_nr();
    ui.data_mut(|d| d.insert_temp(Id::new(PANEL).with("anchor"), (rect, pass)));
}

/// Taken before the field sees them, so they move through suggestions.
fn keys(ui: &mut Ui, field: &Field, out: &mut Commands) {
    let shown = field.open && !field.text.trim().is_empty();
    let mut asked = Vec::new();
    let mut leave = false;
    ui.input_mut(|i| {
        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
            asked.push(SearchAction::Move(1));
        }
        if shown && i.consume_key(Modifiers::NONE, Key::ArrowUp) {
            asked.push(SearchAction::Move(-1));
        }
        if shown && field.lit.is_some() && i.consume_key(Modifiers::NONE, Key::Enter) {
            asked.push(SearchAction::ChooseLit);
        }
        // Escape peels: the list, then the text, then the focus.
        if i.consume_key(Modifiers::NONE, Key::Escape) {
            match () {
                _ if shown => asked.push(SearchAction::Close),
                _ if !field.text.is_empty() => asked.push(SearchAction::Clear),
                _ => leave = true,
            }
        }
    });
    if leave {
        ui.memory_mut(|m| m.surrender_focus(Id::new(FIELD)));
    }
    for a in asked {
        act(out, a);
    }
}

/// The list under the field, while it's focused or the pointer is on it.
pub fn suggestions(ctx: &Context, session: &mut Session, covers: &mut Covers, out: &mut Commands) {
    let rect_id = Id::new(PANEL).with("rect");
    let pass = ctx.cumulative_pass_nr();
    let anchor = ctx.data(|d| d.get_temp::<(Rect, u64)>(Id::new(PANEL).with("anchor")));
    let anchor = anchor.filter(|(_, at)| *at == pass).map(|(r, _)| r);
    let hovered = ctx
        .data(|d| d.get_temp::<Rect>(rect_id))
        .zip(ctx.pointer_hover_pos())
        .is_some_and(|(r, p)| r.contains(p));
    let lib = session.library();
    let showing = lib.search.open && lib.searching() && (focused(ctx) || hovered);
    let found = if anchor.is_some() && showing {
        session.library_suggestions()
    } else {
        std::rc::Rc::from([])
    };
    let Some(anchor) = anchor.filter(|_| !found.is_empty()) else {
        ctx.data_mut(|d| d.remove::<Rect>(rect_id));
        return;
    };
    let lit = session.library().search.lit;
    let (model, t) = (session.library_model(), Tokens::get(ctx));
    let width = anchor.width().max(340.0);
    let left = (anchor.center().x - width / 2.0).max(ctx.content_rect().left() + 8.0);
    let area = z_order::area(ctx, Tier::Popover, Id::new(PANEL))
        .fixed_pos(pos2(left, anchor.bottom() + 6.0))
        .constrain(true);
    let shown = area.show(ctx, |ui| {
        egui::Frame::popup(ui.style()).show(ui, |ui| {
            ui.set_width(width - 12.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (n, suggestion) in found.iter().enumerate() {
                let row = Row {
                    model,
                    suggestion,
                    lit: lit == Some(n),
                    t: &t,
                };
                if row.show(ui, covers).clicked() {
                    act(out, SearchAction::Choose(suggestion.target));
                }
            }
            ui.add_space(4.0);
            let hint = "Choose with ↑ ↓ · Enter goes there · Esc closes";
            let hint = egui::RichText::new(hint).size(11.0).color(t.muted);
            ui.add(egui::Label::new(hint).selectable(false));
        });
    });
    ctx.data_mut(|d| d.insert_temp(rect_id, shown.response.rect));
}

/// One suggestion: a thumb or the shelf mark, its name and where it is.
struct Row<'a> {
    model: &'a Library,
    suggestion: &'a Suggestion,
    lit: bool,
    t: &'a Tokens,
}

impl Row<'_> {
    fn show(&self, ui: &mut Ui, covers: &mut super::Covers) -> egui::Response {
        let t = self.t;
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
        let hit = &self.suggestion.hit;
        let (name, sub, sub_lit) = self.texts();
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::Button, true, self.lit, &name)
        });
        if self.lit || resp.hovered() {
            let fill = if self.lit { t.selected } else { t.hover };
            ui.painter().rect_filled(rect, t.radius, fill);
        }
        let thumb =
            Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(26.0, 36.0));
        match self.suggestion.target {
            Target::Book(id) => {
                if let Some(book) = self.model.book(id) {
                    super::view::thumb(ui, thumb, book, covers, t);
                }
            }
            Target::Shelf(_) => {
                let mark = Rect::from_center_size(thumb.center(), vec2(20.0, 20.0));
                Icon::Shelf.paint(ui, mark, t.muted);
            }
        }
        let p = ui.painter();
        let x = thumb.right() + 12.0;
        let wide = (rect.right() - 10.0 - x).max(40.0);
        let title = marked(&name, &hit.title, theme::medium(13.0), t.ink, t, wide);
        let g = p.layout_job(title);
        p.galley(pos2(x, rect.center().y - g.size().y - 1.0), g, t.ink);
        let small = FontId::proportional(12.0);
        let g = p.layout_job(marked(&sub, sub_lit, small, t.muted, t, wide));
        p.galley(pos2(x, rect.center().y + 2.0), g, t.muted);
        resp
    }

    /// The name, the line under it, and what's lit in that line.
    fn texts(&self) -> (String, String, &[Range<usize>]) {
        let hit = &self.suggestion.hit;
        let model = self.model;
        match self.suggestion.target {
            Target::Book(id) => {
                let Some(book) = model.book(id) else {
                    return Default::default();
                };
                let author = book.author.value.as_deref().map(str::trim);
                let author = author.filter(|a| !a.is_empty());
                // The address, when that's what the search found.
                let (sub, lit) = match author {
                    Some(a) if hit.address.is_empty() || !hit.author.is_empty() => {
                        (a.to_owned(), hit.author.as_slice())
                    }
                    _ => (model.address(book), hit.address.as_slice()),
                };
                (book.title.value.clone(), sub, lit)
            }
            Target::Shelf(id) => {
                let path = model.path_to(id);
                let name = path.last().map(|s| s.name.value.clone());
                let above: Vec<&str> = (path.iter().rev().skip(1).rev())
                    .map(|s| s.name.value.as_str())
                    .collect();
                let sub = match above.is_empty() {
                    true => "Shelf".to_owned(),
                    false => format!("Shelf in {}", above.join(" › ")),
                };
                (name.unwrap_or_default(), sub, &[])
            }
        }
    }
}

/// `text` truncated to `wide`, its `lit` byte ranges picked out.
fn marked(
    text: &str,
    lit: &[Range<usize>],
    font: FontId,
    color: Color32,
    t: &Tokens,
    wide: f32,
) -> LayoutJob {
    let plain = TextFormat::simple(font.clone(), color);
    let strong = TextFormat {
        color: t.ink,
        background: t.accent.gamma_multiply(if t.dark { 0.4 } else { 0.22 }),
        ..TextFormat::simple(font, t.ink)
    };
    let mut job = LayoutJob::default();
    let mut from = 0;
    for span in lit {
        let (Some(before), Some(hit)) = (text.get(from..span.start), text.get(span.clone())) else {
            continue;
        };
        job.append(before, 0.0, plain.clone());
        job.append(hit, 0.0, strong.clone());
        from = span.end;
    }
    job.append(text.get(from..).unwrap_or_default(), 0.0, plain);
    job.wrap = TextWrapping::truncate_at_width(wide);
    job
}

/// The ring around a revealed card: a steady line and a fading halo.
pub fn ring(ui: &Ui, rect: Rect, radius: f32, age: f64, t: &Tokens) {
    let painter = ui.painter();
    let fade = match age {
        a if a < RING_HOLD => 1.0,
        a => (1.0 - (a - RING_HOLD) / (RING_END - RING_HOLD)).clamp(0.0, 1.0) as f32,
    };
    let line = Stroke::new(2.0, t.accent.gamma_multiply(fade));
    painter.rect_stroke(rect, radius, line, StrokeKind::Outside);
    let pulsing = motion::enabled(ui.ctx()) && age < 2.0 * PULSE;
    if pulsing {
        let swell = ((age % PULSE) / PULSE) as f32;
        let halo = t.accent.gamma_multiply(0.5 * (1.0 - swell) * fade);
        let out = rect.expand(2.0 + swell * HALO);
        let width = 4.0 * (1.0 - swell) + 1.0;
        painter.rect_stroke(
            out,
            radius + swell * HALO,
            Stroke::new(width, halo),
            StrokeKind::Middle,
        );
    }
    // Held still, it sleeps until the fade.
    match pulsing || age >= RING_HOLD {
        true => ui.ctx().request_repaint(),
        false => ui
            .ctx()
            .request_repaint_after_secs((RING_HOLD - age) as f32),
    }
}
