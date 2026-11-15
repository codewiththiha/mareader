//! Links, marks and shaded words on screen, as focus-only Tab stops.

use std::cmp::Ordering;

use egui::{Color32, Id, Key, KeyboardShortcut, Modifiers, Rect, Sense, Ui};
use mareader_library::MarkId;
use mareader_pdf::LinkTarget;

use crate::focus::{self, Ring};
use crate::selection::Pages;
use crate::study::marks::Placed;
use crate::vocab::Shaded;

/// A stop, by its place in the lists it came from.
#[derive(Clone, Debug, PartialEq)]
pub enum Stop {
    Link(LinkTarget),
    Mark(usize),
    Word(usize),
}

/// What a key did at the focused stop.
#[derive(Clone, Debug, PartialEq)]
pub enum Press {
    /// Enter or Space: as a click there.
    Go(Stop),
    /// Shift+F10 on a mark: its menu, as a right-click.
    Menu(usize),
}

/// What the stops are laid over this frame.
pub struct On<'a> {
    pub pages: &'a Pages<'a>,
    pub placed: &'a [Placed],
    pub shaded: &'a [Shaded],
    /// Shaded words explain on a click, so they take keys too.
    pub words: bool,
    pub accent: Color32,
}

struct Entry {
    sheet: usize,
    rect: Rect,
    id: Id,
    stop: Stop,
}

/// Lays the stops out and returns the key pressed at the focused one.
pub fn show(ui: &Ui, on: &On<'_>) -> Option<Press> {
    // One book can show in two panes: the pane's id keeps their stops apart.
    let mut stops = gather(on, ui.id());
    let room = room(ui);
    // Only what shows whole: Tab never scrolls the page under the reader.
    stops.retain(|s| room.contains_rect(s.rect));
    stops.sort_by(reading_order);
    let menu = KeyboardShortcut::new(Modifiers::SHIFT, Key::F10);
    let mut press = None;
    for s in stops {
        let resp = ui.interact(s.rect, s.id, Sense::focusable_noninteractive());
        if !resp.has_focus() {
            continue;
        }
        focus::shape(
            &resp,
            Ring {
                radius: 3.0,
                outside: true,
                grow: 0.0,
            },
        );
        // mareader tints a focused link as well as ringing it.
        if matches!(s.stop, Stop::Link(_)) && focus::keyboard(ui.ctx()) {
            let fill = on.accent.gamma_multiply(0.22);
            ui.painter().rect_filled(s.rect.expand(1.0), 3.0, fill);
        }
        let keyed = |key| ui.input(|i| i.key_pressed(key));
        press = match s.stop {
            Stop::Mark(i) if ui.input_mut(|i| i.consume_shortcut(&menu)) => Some(Press::Menu(i)),
            stop if keyed(Key::Enter) || keyed(Key::Space) => Some(Press::Go(stop)),
            _ => None,
        };
    }
    press
}

/// The pane clear of the bars Tab brings up over it.
fn room(ui: &Ui) -> Rect {
    let mut room = ui.clip_rect();
    room.max.y -= crate::page_bar::BAR_H;
    let below_bar = ui.ctx().content_rect().top() + crate::theme::TITLE_BAR_H;
    room.min.y = room.min.y.max(below_bar);
    room
}

/// Every stop on the shown pages, in no order yet.
fn gather(on: &On<'_>, pane: Id) -> Vec<Entry> {
    let pages = on.pages;
    let sheet_of = |r: Rect| {
        let at = r.center();
        let found = pages.sheets.iter().position(|s| s.rect.contains(at));
        found.unwrap_or(usize::MAX)
    };
    let mut out = Vec::new();
    for (n, sheet) in pages.sheets.iter().enumerate() {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        for (i, link) in layer.links.iter().enumerate() {
            out.push(Entry {
                sheet: n,
                rect: sheet.to_screen(link.bounds),
                id: pane.with(("stop-link", pages.doc, sheet.page, i)),
                stop: Stop::Link(link.target.clone()),
            });
        }
    }
    // A mark across two pages is placed once on each.
    let mut seen: Vec<MarkId> = Vec::new();
    for (i, m) in on.placed.iter().enumerate() {
        let again = seen.iter().filter(|id| **id == m.id).count();
        seen.push(m.id);
        let rect = m.bounds();
        out.push(Entry {
            sheet: sheet_of(rect),
            rect,
            id: pane.with(("stop-mark", m.id, again)),
            stop: Stop::Mark(i),
        });
    }
    for (i, w) in on.shaded.iter().enumerate().filter(|_| on.words) {
        let rect = w.bounds();
        out.push(Entry {
            sheet: sheet_of(rect),
            rect,
            id: pane.with(("stop-word", pages.doc, w.page, w.chars.start)),
            stop: Stop::Word(i),
        });
    }
    out
}

/// Page by page, then line by line, then left to right.
fn reading_order(a: &Entry, b: &Entry) -> Ordering {
    // Words on one line sit a little apart; a band keeps them together.
    let line = |e: &Entry| (e.rect.center().y / 6.0).floor();
    a.sheet
        .cmp(&b.sheet)
        .then(line(a).total_cmp(&line(b)))
        .then(a.rect.left().total_cmp(&b.rect.left()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    fn entry(sheet: usize, x: f32, y: f32) -> Entry {
        Entry {
            sheet,
            rect: Rect::from_min_size(pos2(x, y), vec2(20.0, 10.0)),
            id: Id::new((sheet, x as i32, y as i32)),
            stop: Stop::Word(0),
        }
    }

    #[test]
    fn stops_read_by_page_then_line_then_across() {
        let mut all = [
            entry(1, 0.0, 0.0),
            entry(0, 50.0, 41.0),
            entry(0, 10.0, 40.0),
            entry(0, 90.0, 10.0),
        ];
        all.sort_by(reading_order);
        let at: Vec<(usize, f32)> = all.iter().map(|e| (e.sheet, e.rect.left())).collect();
        assert_eq!(at, [(0, 90.0), (0, 10.0), (0, 50.0), (1, 0.0)]);
    }
}
