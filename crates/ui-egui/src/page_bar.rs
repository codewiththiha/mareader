//! Each pane's counter, progress line and hover-revealed page bar.

use egui::{
    Align, Context, CornerRadius, FontId, Id, Key, LayerId, Layout, Order, Rect, RichText, Slider,
    Stroke, TextEdit, UiBuilder, pos2, vec2,
};
use mareader_settings::{LayoutSettings, PageIndicatorStyle};
use mareader_workspace::PaneId;

use crate::backdrop::Backdrop;
use crate::documents::Reading;
use crate::gpu::{Look, Reach};
use crate::icons::Icon;
use crate::motion;
use crate::theme::{self, Tokens};
use crate::titlebar::{FADE, HIDE_GRACE};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;

pub const BAR_H: f32 = 40.0;
/// The strip along a pane's bottom edge that brings the bar up.
pub const EDGE: f32 = 8.0;
/// An icon button's width, as the bookmark takes at the bar's end.
const BOOKMARK_W: f32 = 28.0;
/// Narrower than this, the bar keeps its buttons and drops the slider.
const SLIDER_MIN_PANE: f32 = 260.0;

/// One pane's bar between frames.
#[derive(Debug, Default, Clone)]
pub struct PageBar {
    shown: bool,
    left_at: Option<f64>,
    /// The page number being typed, while the entry has focus.
    typing: Option<String>,
}

impl PageBar {
    /// Shows the bar as if hovered; it hides after the usual grace.
    pub fn reveal(&mut self) {
        self.shown = true;
    }

    /// Up, or on its way down.
    pub fn shown(&self) -> bool {
        self.shown
    }

    fn update_hover(&mut self, ctx: &Context, (pane, own): (Rect, LayerId), held: bool) {
        let now = ctx.input(|i| i.time);
        let reach = if self.shown { BAR_H } else { EDGE };
        let band = Rect::from_min_max(pos2(pane.left(), pane.bottom() - reach), pane.max);
        let inside = widgets::bare_pointer(ctx, own).is_some_and(|p| band.contains(p));
        if inside || held {
            self.shown = true;
            self.left_at = None;
        } else if self.shown {
            let waited = now - *self.left_at.get_or_insert(now);
            if waited >= HIDE_GRACE {
                self.shown = false;
                self.left_at = None;
            } else {
                ctx.request_repaint_after_secs((HIDE_GRACE - waited) as f32);
            }
        }
    }
}

/// The counter's text: "3 / 20", "3 / 12+" while loading, or "15%".
pub fn counter(reading: &Reading, style: PageIndicatorStyle) -> String {
    let (page, count) = (reading.global_current() + 1, reading.count().max(1));
    if !reading.document.is_complete() {
        return format!("{page} / {count}+");
    }
    match style {
        PageIndicatorStyle::PageNumber => format!("{page} / {count}"),
        PageIndicatorStyle::Percentage => {
            format!("{}%", (page as f32 / count as f32 * 100.0).round())
        }
    }
}

/// How the bars look this frame, and which glass slot this one takes.
pub struct BarLook<'a> {
    pub layout: &'a LayoutSettings,
    pub backdrop: &'a Backdrop,
    pub slot: usize,
    /// The pane's corner radius: the progress line stays between corners.
    pub corner: f32,
    /// Where toasts are up; the counter gives way to them.
    pub toasts: Option<Rect>,
}

/// Draws `pane`'s counter, progress line and bar over its document.
pub fn show(
    ctx: &Context,
    pane: Rect,
    reading: &Reading,
    (look, marked): (&BarLook<'_>, bool),
    bar: &mut PageBar,
    out: &mut Commands,
) {
    let layout = look.layout;
    let entry_id = Id::new(("page-entry", reading.pane));
    let typing = ctx.memory(|m| m.has_focus(entry_id));
    let home = Rect::from_min_max(pos2(pane.left(), pane.bottom() - BAR_H), pane.max);
    let dragging = bar.shown
        && ctx.input(|i| {
            let origin = i.pointer.press_origin();
            i.pointer.any_down() && origin.is_some_and(|p| home.contains(p))
        });
    let tabbing = crate::focus::within(ctx, home);
    let id = Id::new(("page-bar", reading.pane));
    let own = LayerId::new(Order::Foreground, id);
    bar.update_hover(ctx, (pane, own), typing || dragging || tabbing);
    let fade = ctx.animate_bool_with_time(id, bar.shown, motion::secs(ctx, FADE));
    let t = Tokens::get(ctx);
    let layer = LayerId::new(Order::Middle, id.with("marks"));
    let painter = ctx.layer_painter(layer).with_clip_rect(pane);
    if layout.progress_bar && reading.document.is_complete() {
        // A faint track first, so the bar shows even on the first page.
        let x = look.corner.min(pane.width() * 0.5);
        let track = Rect::from_min_max(
            pos2(pane.left() + x, pane.bottom() - 2.0),
            pos2(pane.right() - x, pane.bottom()),
        );
        painter.rect_filled(track, CornerRadius::ZERO, t.line.gamma_multiply(0.7));
        let width = track.width() * reading.progress.clamp(0.0, 1.0);
        let line = Rect::from_min_size(track.min, vec2(width, 2.0));
        painter.rect_filled(line, CornerRadius::ZERO, t.accent.gamma_multiply(0.8));
    }
    // The counter gives way to the bar, and to toasts, rather than sit under.
    if layout.page_indicator && fade < 1.0 {
        // A pill of the bars' glass, so it belongs with them.
        let text = counter(reading, layout.page_indicator_style);
        let size = painter
            .layout_no_wrap(text.clone(), theme::medium(11.0), t.ink)
            .size();
        let size = size + vec2(18.0, 6.0);
        let badge = Rect::from_min_size(pane.max - size - vec2(12.0, 12.0), size);
        let covered = look.toasts.is_some_and(|r| r.expand(8.0).intersects(badge));
        let time = motion::secs(ctx, FADE);
        let away = ctx.animate_bool_with_time(id.with("under-toasts"), covered, time);
        let show = (1.0 - fade) * (1.0 - away);
        let ink = t.ink.gamma_multiply(0.85 * show);
        let galley = painter.layout_no_wrap(text, theme::medium(11.0), ink);
        let round = CornerRadius::same(255);
        painter.rect_filled(badge, round, t.glass.gamma_multiply(show));
        let edge = egui::Stroke::new(1.0, t.line.gamma_multiply(show));
        painter.rect_stroke(badge, round, edge, egui::StrokeKind::Inside);
        painter.galley(badge.center() - galley.size() / 2.0, galley, ink);
    }
    if fade <= 0.0 {
        return;
    }
    // It rises a little as it fades in, as in mareader.
    let rise = (1.0 - fade) * 12.0;
    let rect = home.translate(vec2(0.0, rise));
    egui::Area::new(id)
        .fade_in(false)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .interactable(bar.shown)
        .show(ctx, |ui| {
            ui.set_opacity(fade);
            ui.set_min_size(rect.size());
            ui.set_clip_rect(pane);
            let glass = Look {
                slot: look.slot,
                reach: Reach::Above,
                opacity: fade,
            };
            look.backdrop.paint(ui, rect, glass);
            ui.painter()
                .hline(rect.x_range(), rect.top(), Stroke::new(1.0, t.line));
            let row = rect.shrink2(vec2(10.0, 0.0));
            let builder = UiBuilder::new()
                .max_rect(row)
                .layout(Layout::left_to_right(Align::Center));
            ui.scope_builder(builder, |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                contents(ui, reading, bar, entry_id, (pane.width(), marked), out);
            });
            if ui.input(|i| i.pointer.any_pressed()) && ui.rect_contains_pointer(rect) {
                out.push(Sent::Engine(Command::FocusPane(reading.pane)));
            }
        });
}

fn contents(
    ui: &mut egui::Ui,
    reading: &Reading,
    bar: &mut PageBar,
    entry_id: Id,
    (pane_width, marked): (f32, bool),
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let pane: PaneId = reading.pane;
    let count = reading.count();
    if widgets::icon_button(ui, Icon::ChevronLeft, "Previous page", false).clicked() {
        out.push(Sent::Engine(Command::Turn {
            pane,
            forward: false,
        }));
    }
    let mut text = bar
        .typing
        .clone()
        .unwrap_or_else(|| (reading.global_current() + 1).to_string());
    let entry = TextEdit::singleline(&mut text)
        .id(entry_id)
        .desired_width(44.0)
        .horizontal_align(Align::Center)
        .font(FontId::proportional(t.text))
        .frame(egui::Frame::NONE);
    let r = ui.add(entry);
    widgets::name(&r, "Page number");
    // A bordered field, as on the bar's glass egui's own frame vanishes.
    let edge = if r.has_focus() { t.accent } else { t.line };
    ui.painter().rect_stroke(
        r.rect.expand2(vec2(2.0, 4.0)),
        CornerRadius::same(6),
        Stroke::new(1.0, edge),
        egui::StrokeKind::Inside,
    );
    if r.has_focus() {
        bar.typing = Some(text.clone());
    }
    if r.lost_focus() {
        // A number jumps; anything else snaps back to the page shown.
        let entered = ui.input(|i| i.key_pressed(Key::Enter));
        if let (true, Ok(n @ 1..)) = (entered, text.trim().parse::<usize>()) {
            let target = n - 1;
            let page = if reading.document.is_complete() {
                Some(target)
            } else {
                reading.document.local_page(target)
            };
            if let Some(page) = page {
                out.push(Sent::Engine(Command::GoTo { pane, page }));
            }
        }
        bar.typing = None;
    }
    let total = if reading.document.is_complete() {
        format!("/ {count}")
    } else {
        format!("/ {count}+")
    };
    ui.label(RichText::new(total).color(t.muted));
    if widgets::icon_button(ui, Icon::ChevronRight, "Next page", false).clicked() {
        out.push(Sent::Engine(Command::Turn {
            pane,
            forward: true,
        }));
    }
    if reading.document.is_complete() && pane_width >= SLIDER_MIN_PANE && count >= 2 {
        let mut page = reading.current + 1;
        // Room is kept at the end for the bookmark.
        let room = ui.available_width() - BOOKMARK_W - 10.0;
        ui.spacing_mut().slider_width = room.max(40.0);
        let slider = Slider::new(&mut page, 1..=count)
            .show_value(false)
            .trailing_fill(true);
        let r = ui.add(slider);
        widgets::name(&r, "Page");
        if r.changed() {
            out.push(Sent::Engine(Command::GoTo {
                pane,
                page: page - 1,
            }));
        }
    }
    let tip = if marked {
        "Remove bookmark"
    } else {
        "Bookmark this page"
    };
    let platform = crate::platform::Platform::current();
    let keys = crate::keymap::hint(platform, &Sent::Engine(Command::Bookmark(None)));
    let r = widgets::icon_button_tagged(
        ui,
        Icon::Bookmark,
        tip,
        "Page bookmark",
        marked,
        keys.as_deref(),
    );
    if r.clicked() {
        out.push(Sent::Engine(Command::Bookmark(Some(pane))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_counter_reads_as_pages_or_a_percentage() -> Result<(), String> {
        let reading = crate::testing::reading(20, 2)?;
        assert_eq!(counter(&reading, PageIndicatorStyle::PageNumber), "3 / 20");
        assert_eq!(counter(&reading, PageIndicatorStyle::Percentage), "15%");
        Ok(())
    }

    #[test]
    fn a_preview_keeps_the_saved_global_page_number() -> Result<(), String> {
        let bytes = std::sync::Arc::new(mareader_epub::sample::epub(40, "Progress"));
        let typo = mareader_session::Typography::default();
        let full = mareader_epub::Book::open(std::sync::Arc::clone(&bytes), &typo)
            .map_err(|error| error.to_string())?;
        let target_page = full.page_count() / 2;
        let anchor = full.anchor(target_page).ok_or("no middle-page anchor")?;
        let spine = full
            .spine_place(anchor)
            .ok_or("no stable middle-page location")?;
        let pages = full.page_count();
        let mut session = mareader_session::Session::new(0, || {});
        let ticket = session.open_at(
            mareader_session::Source::Bytes(bytes),
            None,
            std::sync::Arc::new(typo),
            mareader_session::OpenTarget {
                page: target_page,
                place: Some(anchor),
                spine: Some(spine.clone()),
                pages,
            },
        );
        let mut doc = None;
        for _ in 0..32 {
            doc = session
                .poll(mareader_session::Budget {
                    items: 8,
                    bytes: usize::MAX,
                })
                .into_iter()
                .find_map(|event| match event {
                    mareader_session::Event::Preview { ticket: got, doc } if got == ticket => {
                        Some(doc)
                    }
                    _ => None,
                });
            if doc.is_some() {
                break;
            }
        }
        let doc = doc.ok_or_else(|| "the saved place was not previewed".to_string())?;
        let entry = session
            .get(doc)
            .ok_or_else(|| "no preview entry".to_string())?;
        let current = entry
            .doc
            .locate_spine(&spine)
            .ok_or("the preview lost the saved place")?;
        let reading = Reading {
            pane: PaneId(1),
            doc,
            document: std::sync::Arc::clone(&entry.doc),
            current,
            mode: mareader_engine::pane::ViewMode::Vertical,
            progress: 0.0,
            name: "Progress".into(),
            page: None,
        };
        let available = reading.count().max(1);
        let expected = format!("{} / {available}+", target_page + 1);
        assert_eq!(reading.global_current(), target_page);
        assert_eq!(counter(&reading, PageIndicatorStyle::PageNumber), expected);
        assert_eq!(counter(&reading, PageIndicatorStyle::Percentage), expected);
        Ok(())
    }
}
