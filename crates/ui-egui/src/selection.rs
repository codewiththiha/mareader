//! Selecting text across a pane's pages, and the links among it.

use std::sync::Arc;

use egui::style::ScrollAnimation;
use egui::{Color32, CursorIcon, Pos2, Rect, Response, Sense, Ui, Vec2, vec2};
use mareader_engine::texts::TextLayers;
use mareader_pdf::{Bounds, LinkTarget};
use mareader_session::{Caret, DocId, Selection, TextLayer};

mod grab;

/// Fastest a drag past the pane's edge scrolls, in points per frame.
const EDGE_SCROLL: f32 = 24.0;

/// A page on screen: its rect and points-to-screen scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sheet {
    pub page: usize,
    pub rect: Rect,
    /// What of `rect` is on screen; a stream shows only a band of it.
    pub shown: Rect,
    pub scale: f32,
}

impl Sheet {
    /// A screen point in page points, origin top left.
    pub fn to_page(self, p: Pos2) -> (f32, f32) {
        let at = (p - self.rect.min) / self.scale;
        (at.x, at.y)
    }

    /// A box in page points, on screen.
    pub fn to_screen(self, b: Bounds) -> Rect {
        let at = |x: f32, y: f32| self.rect.min + vec2(x, y) * self.scale;
        Rect::from_min_max(at(b.x0, b.y0), at(b.x1, b.y1))
    }

    fn distance(self, p: Pos2) -> f32 {
        self.shown.distance_to_pos(p)
    }
}

/// What the pointer did over a pane's pages this frame.
#[derive(Debug, Default)]
pub struct Outcome {
    pub follow: Option<LinkTarget>,
    /// Where a plain click landed.
    pub click: Option<Pos2>,
    /// Where a right-click landed.
    pub context: Option<Pos2>,
    /// Where a double click found no word, maybe on a picture.
    pub double: Option<Pos2>,
    /// Where a hold on empty space lifted the pane.
    pub lift: Option<Pos2>,
}

/// The pages' texts as one pane sees them.
pub struct Pages<'a> {
    pub doc: DocId,
    pub sheets: &'a [Sheet],
    pub texts: &'a TextLayers,
    /// The strip scrolls only sideways, so the wheel turns that way.
    pub sideways: bool,
    /// The pane shares the window, so a hold on empty space lifts it.
    pub lift: bool,
}

impl Pages<'_> {
    fn under(&self, p: Pos2) -> Option<(Sheet, Arc<TextLayer>)> {
        let sheet = *self.sheets.iter().find(|s| s.shown.contains(p))?;
        Some((sheet, self.texts.get(self.doc, sheet.page)?))
    }

    /// Off the pages, or away from text and links; unloaded text counts.
    fn empty(&self, p: Pos2) -> bool {
        let Some(sheet) = self.sheets.iter().find(|s| s.shown.contains(p)) else {
            return true;
        };
        let Some(layer) = self.texts.get(self.doc, sheet.page) else {
            return false;
        };
        let (x, y) = sheet.to_page(p);
        !layer.text.on_line(x, y) && layer.link_at(x, y).is_none()
    }

    /// The caret a drag at `p` means: on the nearest page, held to it.
    fn caret(&self, p: Pos2) -> Option<Caret> {
        let sheet = self
            .sheets
            .iter()
            .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))?;
        let layer = self.texts.get(self.doc, sheet.page)?;
        let (x, y) = sheet.to_page(p.clamp(sheet.shown.min, sheet.shown.max));
        Some(Caret {
            page: sheet.page,
            index: layer.text.caret_at(x, y),
        })
    }

    /// A double click's word, or a triple click's line.
    fn unit(&self, p: Pos2, line: bool) -> Option<Selection> {
        let (sheet, layer) = self.under(p)?;
        let (x, y) = sheet.to_page(p);
        let at = layer.text.char_at(x, y)?;
        let range = if line {
            layer.text.line_at(at)
        } else {
            layer.text.word_at(at)
        };
        Some(Selection::within(sheet.page, range))
    }
}

/// Drags select, clicks pick words or lines, a link click is returned.
pub fn interact(
    ui: &Ui,
    area: Rect,
    pages: &Pages<'_>,
    selection: &mut Option<Selection>,
) -> Outcome {
    let response = ui.interact(area, ui.id().with("text"), Sense::CLICK | Sense::DRAG);
    let mut out = Outcome::default();
    let hovered = response
        .hover_pos()
        .and_then(|p| pages.under(p).map(|u| (p, u)));
    let link = hovered.as_ref().and_then(|(p, (sheet, layer))| {
        let (x, y) = sheet.to_page(*p);
        let over_text = layer.text.char_at(x, y).is_some();
        if over_text && !response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Text);
        }
        layer.link_at(x, y).map(|l| l.target.clone())
    });
    let origin = ui.input(|i| i.pointer.press_origin());
    let empty = origin.is_some_and(|p| pages.empty(p));
    let grabbed = grab::pan(ui, &response, empty, pages.lift);
    out.lift = grabbed.lift;
    if grabbed.busy {
        return out;
    }
    if response.hover_pos().is_some_and(|p| pages.empty(p)) {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    if response.drag_started() {
        *selection = origin.and_then(|p| pages.caret(p)).map(|c| Selection {
            anchor: c,
            focus: c,
        });
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Text);
        if let Some(p) = response.interact_pointer_pos() {
            if let (Some(sel), Some(c)) = (selection.as_mut(), pages.caret(p)) {
                sel.focus = c;
            }
            edge_scroll(ui, area, p);
        }
        wheel(ui, pages.sideways);
    } else if let Some(target) = &link {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        tooltip(&response, target);
    }
    if response.clicked() {
        let p = response.interact_pointer_pos();
        let unit = |line| p.and_then(|p| pages.unit(p, line));
        if response.triple_clicked() {
            *selection = unit(true);
        } else if response.double_clicked() {
            *selection = unit(false);
            out.double = p.filter(|_| selection.is_none());
        } else {
            *selection = None;
            out.follow = link;
            out.click = p;
        }
    }
    if response.secondary_clicked() {
        out.context = response.interact_pointer_pos();
    }
    out
}

fn tooltip(response: &Response, target: &LinkTarget) {
    let text = match target {
        LinkTarget::Page(page) => format!("Go to page {}", page + 1),
        LinkTarget::Web(url) => url.clone(),
        LinkTarget::Note { .. } => "Show the note".to_owned(),
        LinkTarget::Media { video: true, .. } => "Play the video".to_owned(),
        LinkTarget::Media { video: false, .. } => "Play the audio".to_owned(),
        LinkTarget::Transcript { track: None, .. } => "Show or hide the transcript".to_owned(),
        LinkTarget::Transcript { .. } => "Show the transcript in this language".to_owned(),
        LinkTarget::Trigger { what, .. } => what.clone(),
        LinkTarget::Script { .. } => "Interactive: click to use".to_owned(),
    };
    response.clone().on_hover_text_at_pointer(text);
}

/// egui drops the wheel during drags, so a selection scrolls itself.
pub(crate) fn wheel(ui: &Ui, sideways: bool) {
    let delta = ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta));
    let delta = if sideways {
        Vec2::new(delta.x + delta.y, 0.0)
    } else {
        delta
    };
    // egui has smoothed it already; animating again would drop most of it.
    if delta != Vec2::ZERO {
        ui.scroll_with_delta_animation(delta, ScrollAnimation::none());
        ui.ctx().request_repaint();
    }
}

/// Dragging past an edge scrolls on, faster the further out.
pub(crate) fn edge_scroll(ui: &Ui, area: Rect, p: Pos2) {
    let past = |v: f32, lo: f32, hi: f32| {
        let d = if v < lo {
            lo - v
        } else if v > hi {
            hi - v
        } else {
            0.0
        };
        d.clamp(-EDGE_SCROLL, EDGE_SCROLL)
    };
    let delta = Vec2::new(
        past(p.x, area.left(), area.right()),
        past(p.y, area.top(), area.bottom()),
    );
    // Each frame's step lands now, or egui's glide would swallow it.
    if delta != Vec2::ZERO {
        ui.scroll_with_delta_animation(delta, ScrollAnimation::none());
        ui.ctx().request_repaint();
    }
}

/// Tints the selected characters, one band per line.
pub fn paint(ui: &Ui, pages: &Pages<'_>, selection: Selection, tint: Color32) {
    let painter = ui.painter();
    for sheet in pages.sheets {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let Some(range) = selection.on_page(sheet.page, layer.text.len()) else {
            continue;
        };
        for b in layer.text.line_boxes(range) {
            painter.rect_filled(sheet.to_screen(b).intersect(sheet.shown), 1.0, tint);
        }
    }
}

#[cfg(test)]
mod tests {
    use mareader_pdf::PageText;

    use super::*;

    fn caret(page: usize, index: usize) -> Caret {
        Caret { page, index }
    }

    #[test]
    fn a_selection_spans_pages_whichever_way_it_was_dragged() {
        let up = Selection {
            anchor: caret(3, 5),
            focus: caret(1, 10),
        };
        assert_eq!(up.ordered(), (caret(1, 10), caret(3, 5)));
        assert_eq!(up.pages(), 1..4);
        assert_eq!(up.on_page(0, 50), None);
        assert_eq!(up.on_page(1, 50), Some(10..50));
        assert_eq!(up.on_page(2, 50), Some(0..50));
        assert_eq!(up.on_page(3, 50), Some(0..5));
        // A caret past a page's text holds to its end.
        assert_eq!(up.on_page(1, 4), None);
        assert!(Selection::within(2, 4..4).is_empty());
    }

    fn page(wraps: bool, words: &str) -> Arc<TextLayer> {
        let at = |i: usize| {
            let x0 = i as f32 * 10.0;
            let (x1, y1) = (x0 + 10.0, 20.0);
            Bounds {
                x0,
                y0: 0.0,
                x1,
                y1,
            }
        };
        let line = words.chars().enumerate().map(|(i, c)| (c, at(i)));
        let text = PageText::from_lines([(wraps, line.collect())]);
        Arc::new(TextLayer {
            text,
            ..TextLayer::default()
        })
    }

    #[test]
    fn a_page_going_on_from_the_last_copies_unbroken() {
        let pages = [
            page(false, "the boats "),
            page(true, "unloaded"),
            page(false, "End"),
        ];
        let all = Selection {
            anchor: caret(0, 4),
            focus: caret(2, 3),
        };
        let text = all.text(|p| pages.get(p).cloned());
        assert_eq!(text, "boats unloaded\nEnd");
    }
}
