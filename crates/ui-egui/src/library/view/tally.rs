//! A book's "3 notes · 12 words" under its title: each opens Space on it.

use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{
    CursorIcon, FontId, Galley, Id, Pos2, Rect, Sense, Stroke, Ui, WidgetInfo, WidgetType, pos2,
};
use mareader_engine::library::counted;
use mareader_engine::space::{Room, Tally, tally};
use mareader_library::{BookId, Library};

use super::Cx;
use crate::ui_command::{Sent, UiCommand};

const SIZE: f32 = 12.0;
const DOT: &str = " · ";

/// Every book's counts, read once a frame however many cards ask.
fn counts(ui: &Ui, lib: &Library) -> Arc<BTreeMap<BookId, Tally>> {
    let id = Id::new("library-tally");
    let pass = ui.ctx().cumulative_pass_nr();
    let kept: Option<(u64, Arc<BTreeMap<BookId, Tally>>)> = ui.data(|d| d.get_temp(id));
    if let Some((_, counts)) = kept.filter(|(at, _)| *at == pass) {
        return counts;
    }
    let counts = Arc::new(tally(lib));
    ui.data_mut(|d| d.insert_temp(id, (pass, Arc::clone(&counts))));
    counts
}

/// `book`'s counts, when it has a note or a word.
pub(super) fn of(ui: &Ui, lib: &Library, book: BookId) -> Option<Tally> {
    let n = counts(ui, lib).get(&book).copied()?;
    (!n.is_empty()).then_some(n)
}

/// The parts said, and the room each opens.
fn parts(n: Tally) -> Vec<(String, Room)> {
    let notes = (n.notes, ("note", "notes"), Room::Notes);
    let words = (n.words, ("word", "words"), Room::Words);
    let shown = [notes, words].into_iter().filter(|(count, ..)| *count > 0);
    let said = |(count, (one, many), room)| (counted(count, one, many), room);
    shown.map(said).collect()
}

/// The line at `at`, from its left, or its right when `right`; within
/// `width`. Each part is a link while no cards are being picked.
pub(super) fn line(ui: &Ui, (at, width, right): (Pos2, f32, bool), book: BookId, cx: &mut Cx<'_>) {
    let Some(n) = of(ui, cx.model, book) else {
        return;
    };
    let t = cx.t;
    let font = FontId::proportional(SIZE);
    let p = ui.painter();
    let dot = p.layout_no_wrap(DOT.into(), font.clone(), t.muted);
    let mut laid = Vec::new();
    for (said, room) in parts(n) {
        let galley = p.layout_no_wrap(said.clone(), font.clone(), t.muted);
        laid.push((galley, said, room));
    }
    let span = |laid: &[(Arc<Galley>, String, Room)]| {
        let gaps = dot.size().x * laid.len().saturating_sub(1) as f32;
        laid.iter().map(|(g, ..)| g.size().x).sum::<f32>() + gaps
    };
    // Too narrow for both: the first says enough.
    if span(&laid) > width {
        laid.truncate(1);
    }
    let mut x = if right { at.x - span(&laid) } else { at.x };
    let live = !cx.picks.active();
    for (i, (galley, said, room)) in laid.into_iter().enumerate() {
        if i > 0 {
            p.galley(pos2(x, at.y), dot.clone(), t.muted);
            x += dot.size().x;
        }
        let rect = Rect::from_min_size(pos2(x, at.y), galley.size());
        x = rect.right();
        let id = Id::new(("library-tally", book, room.name()));
        let sense = if live { Sense::click() } else { Sense::hover() };
        let resp = ui.interact(rect, id, sense);
        resp.widget_info(|| WidgetInfo::labeled(WidgetType::Link, live, &said));
        let lit = live && resp.hovered();
        let ink = if lit { t.accent } else { t.muted };
        p.galley_with_override_text_color(rect.min, galley, ink);
        if lit {
            let under = rect.bottom() - 1.0;
            p.hline(rect.x_range(), under, Stroke::new(1.0, ink));
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if live && resp.clicked() {
            cx.out.push(Sent::Ui(UiCommand::SpaceFor { room, book }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_what_a_book_holds_is_said_and_one_is_singular() {
        let said = |notes, words| -> Vec<String> {
            let parts = parts(Tally { notes, words });
            parts.into_iter().map(|(s, _)| s).collect()
        };
        assert_eq!(said(1, 12), ["1 note", "12 words"]);
        assert_eq!(said(0, 3), ["3 words"]);
        assert_eq!(said(2, 0), ["2 notes"]);
    }
}
