//! A card's two faces, painted flat before the card turns them.
//!
//! The front holds the word, as large as the card allows. The back holds
//! all an AI wrote: the meaning, the word class, the synonyms and an
//! example. When they crowd a small card, the text steps down in size and
//! moves under the corner pills; past the smallest step it scrolls with the
//! wheel, soft edges showing there is more. The band sits in the top right
//! corner and, when the deck has no header, the time left in the top left
//! and which card it is between.

use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontId, Galley, Painter, Pos2, Rect, Shape, pos2, vec2};

use super::{Cards, Faces};
use crate::theme::{self, Tokens};

/// The back's steps down: the meaning's size, the notes' change, the gap.
const STEPS: [(f32, f32, f32); 5] = [
    (20.0, 1.0, 10.0),
    (17.0, 0.0, 8.0),
    (15.0, 0.0, 6.0),
    (14.0, -1.0, 5.0),
    (13.0, -1.5, 4.0),
];
/// A card narrower than this keeps smaller margins.
const NARROW: f32 = 320.0;
/// How far a long back's soft edges reach.
const FADE: f32 = 18.0;

/// A back too long for its card: the text to clip to `window`, the soft
/// edges over it, and how far it scrolls.
pub(super) struct Long {
    pub text: Vec<Shape>,
    pub fade: Vec<Shape>,
    pub window: Rect,
    pub over: f32,
}

/// One face's lines, each with its colour.
type Lines = Vec<(Arc<Galley>, Color32)>;

/// The step a card's height is planned for, so its back reads easily.
const EASY: (f32, f32, f32) = (17.0, 0.0, 8.0);

/// How tall a card `width` wide wants to be, for `faces`' back to read easily.
pub fn need(painter: &Painter, t: &Tokens, faces: &Faces, width: f32) -> f32 {
    let (inset, side) = margins(width);
    let (size, notes, gap) = EASY;
    let lines = back_lines(painter, t, faces, (size, notes), width - 2.0 * side);
    tall(&lines, gap) + band_room(t, faces, width) + inset
}

/// The corner pills' inset, and the text's side margin, on a card `width` wide.
fn margins(width: f32) -> (f32, f32) {
    match width < NARROW {
        true => (10.0, 14.0),
        false => (16.0, 28.0),
    }
}

/// How far down the corner pills reach, and a little air below them.
fn band_room(t: &Tokens, faces: &Faces, width: f32) -> f32 {
    let (inset, _) = margins(width);
    let corners = [&faces.badge, &faces.clock, &faces.count];
    let empty = corners.iter().all(|s| s.is_empty());
    let pill = t.text_small * 1.3 + 6.0;
    if empty { inset } else { inset + pill + 4.0 }
}

/// The back's lines: the meaning `size` big, the notes `notes` off.
fn back_lines(painter: &Painter, t: &Tokens, faces: &Faces, pt: (f32, f32), width: f32) -> Lines {
    let (size, notes) = pt;
    let small = FontId::proportional(t.text_small + notes);
    let lines = [
        (&faces.meaning, theme::medium(size), t.ink, false),
        (&faces.class, small.clone(), t.muted, false),
        (&faces.synonyms, small.clone(), t.muted, false),
        (&faces.usage, small, t.muted, true),
    ];
    (lines.into_iter())
        .filter(|(text, ..)| !text.is_empty())
        .map(|(text, font, color, italics)| {
            (styled(painter, text, (font, color, italics), width), color)
        })
        .collect()
}

impl Cards {
    pub(super) fn front(&self, faces: &Faces, out: &mut Vec<Shape>) {
        let r = self.local();
        let (_, side) = margins(r.width());
        let room = r.width() - 2.0 * side - 8.0;
        let high = r.height() - 2.0 * band_room(&self.t, faces, r.width());
        let ink = self.t.ink;
        let mut word = self.text(&faces.word, theme::medium(46.0), ink, f32::INFINITY);
        let fit = (room / word.rect.width()).min(high.max(20.0) / word.rect.height());
        if fit < 1.0 {
            let size = (46.0 * fit).max(14.0);
            word = self.text(&faces.word, theme::medium(size), ink, room + 1.0);
        }
        let at = pos2(0.0, -word.rect.height() / 2.0);
        out.push(centred(at, word, ink));
        self.pills(faces, out);
    }

    /// The back; one too long for the card even at the smallest step
    /// scrolls by `scroll` under the pills, its text kept apart to clip.
    pub(super) fn back(&self, faces: &Faces, out: &mut Vec<Shape>, scroll: f32) -> Option<Long> {
        let r = self.local();
        let top = band_room(&self.t, faces, r.width());
        let (inset, side) = margins(r.width());
        let room = r.height() - top - inset;
        let mut chosen = (Vec::new(), 0.0, 0.0);
        for &(size, notes, gap) in &STEPS {
            let width = r.width() - 2.0 * side;
            let lines = back_lines(&self.painter, &self.t, faces, (size, notes), width);
            let height = tall(&lines, gap);
            chosen = (lines, gap, height);
            if height <= room {
                break;
            }
        }
        self.pills(faces, out);
        let (lines, gap, height) = chosen;
        let over = height - room;
        // Centred when it clears the pills both ways, else just under them.
        let mut y = match (height <= r.height() - 2.0 * top, over > 0.0) {
            (true, _) => -height / 2.0,
            (false, false) => r.top() + top + (room - height) / 2.0,
            (false, true) => r.top() + top - scroll.clamp(0.0, over),
        };
        let mut text = Vec::new();
        for (galley, color) in lines {
            let height = galley.rect.height();
            text.push(centred(pos2(0.0, y), galley, color));
            y += height + gap;
        }
        if over <= 0.0 {
            out.extend(text);
            return None;
        }
        let from = pos2(r.left(), r.top() + top);
        let window = Rect::from_min_max(from, pos2(r.right(), r.bottom() - inset));
        let fade = self.fades(window, scroll.clamp(0.0, over), over);
        Some(Long {
            text,
            fade,
            window,
            over,
        })
    }

    /// Soft edges where a long back's text runs on past `window`.
    fn fades(&self, window: Rect, scroll: f32, over: f32) -> Vec<Shape> {
        let paper = self.t.surface;
        let clear = paper.gamma_multiply(0.0);
        let mut out = Vec::new();
        let low = window.bottom() - FADE;
        let edges = [
            (scroll > 0.5, window.top(), FADE, paper, clear),
            (scroll < over - 0.5, low, FADE, clear, paper),
        ];
        for (shown, y, reach, from, to) in edges {
            if !shown {
                continue;
            }
            let rect = Rect::from_min_size(pos2(window.left(), y), vec2(window.width(), reach));
            let mut mesh = egui::Mesh::default();
            let (low, high) = (rect.left_bottom(), rect.right_bottom());
            let corners = [rect.left_top(), rect.right_top(), high, low];
            for (i, p) in corners.into_iter().enumerate() {
                mesh.colored_vertex(p, if i < 2 { from } else { to });
            }
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            out.push(Shape::mesh(mesh));
        }
        out
    }

    /// The band top right; the time left top left, which card it is between
    /// while there is room for it.
    fn pills(&self, faces: &Faces, out: &mut Vec<Shape>) {
        let r = self.local();
        let (inset, _) = margins(r.width());
        let t = &self.t;
        let font = theme::medium(t.text_small);
        let right = r.right_top() + vec2(-inset, inset);
        let left = r.left_top() + vec2(inset, inset);
        let corners = [
            (&faces.badge, t.accent, right, true),
            (&faces.clock, t.ink, left, false),
        ];
        let mut taken = 2.0 * inset;
        for (text, color, corner, on_right) in corners {
            if text.is_empty() {
                continue;
            }
            let galley = self.text(text, font.clone(), color, f32::INFINITY);
            let pill = galley.rect.size() + vec2(14.0, 6.0);
            taken += pill.x;
            let x = corner.x - if on_right { pill.x } else { 0.0 };
            let rect = Rect::from_min_size(pos2(x, corner.y), pill);
            out.push(Shape::rect_filled(rect, pill.y / 2.0, t.track));
            let at = pos2(rect.center().x, rect.top() + 3.0);
            out.push(centred(at, galley, color));
        }
        if faces.count.is_empty() {
            return;
        }
        let galley = self.text(&faces.count, font, t.muted, f32::INFINITY);
        if taken + galley.rect.width() + 16.0 <= r.width() {
            out.push(centred(pos2(0.0, r.top() + inset + 3.0), galley, t.muted));
        }
    }

    fn text(&self, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
        styled(&self.painter, text, (font, color, false), width)
    }
}

/// `text` laid out centred, `width` at most, in its font, colour and slant.
fn styled(
    painter: &Painter,
    text: &str,
    (font_id, color, italics): (FontId, Color32, bool),
    width: f32,
) -> Arc<Galley> {
    let format = TextFormat {
        font_id,
        color,
        italics,
        ..TextFormat::default()
    };
    crate::machine_fonts::note(painter.ctx(), text);
    let mut job = LayoutJob::single_section(text.to_owned(), format);
    job.wrap.max_width = width;
    job.halign = Align::Center;
    painter.layout_job(job)
}

/// The lines' height, `gap` apart.
fn tall(lines: &[(Arc<Galley>, Color32)], gap: f32) -> f32 {
    let heights: f32 = lines.iter().map(|(g, _)| g.rect.height()).sum();
    heights + gap * lines.len().saturating_sub(1) as f32
}

/// `galley` with its top middle at `top`.
fn centred(top: Pos2, galley: Arc<Galley>, color: Color32) -> Shape {
    let at = pos2(top.x - galley.rect.center().x, top.y - galley.rect.top());
    Shape::galley(at, galley, color)
}
