//! A cloth binding for a book without art: its tone, spine and type.

use std::ops::RangeInclusive;

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Align2, Color32, CornerRadius, Painter, Pos2, Rect, Shape, Stroke, TextFormat, pos2,
    vec2,
};
use mareader_library::Book;

use super::face::{across, clip, quad, rounded, shade};
use crate::theme::{self, Tokens};

/// Cloth tones for books without art. Gilt type reads on each at 4.5:1.
const CLOTH: [u32; 10] = [
    0x1E3354, // navy
    0x27493F, // forest
    0x5E2A2C, // oxblood
    0x6E4A1C, // ochre
    0x38424F, // slate
    0x4A3450, // plum
    0x1D4E55, // teal
    0x4C5230, // olive
    0x74402A, // rust
    0x2C3768, // indigo
];
/// The gilt of a cloth cover's type and frame.
const GILT: Color32 = Color32::from_rgb(0xF3, 0xEE, 0xE3);

/// A steady tone per book from the cloth list; deeper on a dark page.
pub(super) fn paper(book: &Book, t: &Tokens) -> Color32 {
    let mixed = book
        .content
        .0
        .wrapping_mul(0x9E37_79B9_7F4A_7C15_F39C_C060_5CED_C835);
    let at = ((mixed >> 96) % CLOTH.len() as u128) as usize;
    let hex = CLOTH.get(at).copied().unwrap_or(0x38424F);
    let tone = Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    match t.dark {
        true => tone.lerp_to_gamma(Color32::BLACK, 0.18),
        false => tone,
    }
}

/// A cloth binding: its tone lit from above, a darker spine, and type.
pub(super) fn cloth(
    p: &Painter,
    rect: Rect,
    book: &Book,
    (round, small): (CornerRadius, bool),
    fade: f32,
    t: &Tokens,
) {
    p.rect_filled(rect, round, paper(book, t).gamma_multiply(fade));
    let window = rounded(rect, round);
    let (top, h) = (rect.top(), rect.height().max(1.0));
    let light = (Color32::from_white_alpha(20), Color32::from_black_alpha(38));
    shade(p, &window, |at| {
        let k = ((at.y - top) / h).clamp(0.0, 1.0);
        light.0.lerp_to_gamma(light.1, k).gamma_multiply(fade)
    });
    let gilt = |share: f32| GILT.gamma_multiply(share * fade);
    if small {
        let size = rect.width() * 0.55;
        let at = rect.center() + vec2(rect.width() * 0.03, 0.0);
        let font = theme::serif(size);
        p.text(at, Align2::CENTER_CENTER, initial(book), font, gilt(0.92));
        return;
    }
    spine(p, rect, &window, fade);
    // Rules pressed into the cloth, a little in from head and foot.
    let left = hinge(rect) + rect.width() * 0.07;
    let right = rect.right() - rect.width() * 0.07;
    let head = top + h * 0.075;
    let foot = rect.bottom() - h * 0.075;
    for y in [head, foot] {
        blind(p, left..=right, y, fade);
    }
    let column = Rect::from_x_y_ranges(left + 4.0..=right - 4.0, head..=foot);
    let below = title(p, book, column, gilt(0.94));
    let size = (rect.width() / 19.0).clamp(8.5, 11.5);
    let mark = pos2(column.center().x, below + size * 1.4);
    ornament(p, mark, size, gilt(0.6));
    // Who wrote it, in spaced capitals above the foot; else its format.
    let author = book.author.value.as_deref().map(str::trim);
    let by = match author.filter(|a| !a.is_empty()) {
        Some(a) => a.to_uppercase(),
        None => book.format.label().to_owned(),
    };
    let mut job = LayoutJob::single_section(by, spaced(size, gilt(0.78)));
    job.wrap = TextWrapping::truncate_at_width(column.width());
    let imprint = p.layout_job(job);
    let at = pos2(
        column.center().x - imprint.size().x / 2.0,
        foot - h * 0.05 - imprint.size().y,
    );
    p.galley(at, imprint, gilt(0.78));
}

/// Where a full cover's hinge is pressed, near its spine.
pub(super) fn hinge(rect: Rect) -> f32 {
    rect.left() + (rect.width() * 0.045).max(5.0)
}

/// The first letter or digit of a book's title, as a small cover shows it.
fn initial(book: &Book) -> String {
    let first = book.title.value.chars().find(|c| c.is_alphanumeric());
    let upper = |c: char| c.to_uppercase().collect::<String>();
    first.map(upper).unwrap_or_default()
}

/// The spine: a deeper band to the hinge, its rounded back catching light.
fn spine(p: &Painter, rect: Rect, window: &[Pos2], fade: f32) {
    let band = Rect::from_x_y_ranges(rect.left()..=hinge(rect), rect.y_range());
    let deep = Color32::from_black_alpha(44).gamma_multiply(fade);
    let soft = Color32::from_black_alpha(22).gamma_multiply(fade);
    across(p, &clip(quad(band), window), band, (soft, deep));
    let x = rect.left() + 1.5;
    let lit = Color32::from_white_alpha(22).gamma_multiply(fade);
    let ends = [pos2(x, rect.top() + 2.0), pos2(x, rect.bottom() - 2.0)];
    p.line_segment(ends, Stroke::new(1.0, lit));
    // The board turns away from the light toward its fore-edge.
    let board = Rect::from_x_y_ranges(rect.center().x..=rect.right(), rect.y_range());
    let dim = Color32::from_black_alpha(18).gamma_multiply(fade);
    let fall = (Color32::TRANSPARENT, dim);
    across(p, &clip(quad(board), window), board, fall);
}

/// A blind-stamped rule: a dark line pressed in, lit along its lower lip.
fn blind(p: &Painter, x: RangeInclusive<f32>, y: f32, fade: f32) {
    let y = y.round() + 0.5;
    let x = x.start().round()..=x.end().round();
    let pressed = Color32::from_black_alpha(70).gamma_multiply(fade);
    let lip = Color32::from_white_alpha(26).gamma_multiply(fade);
    p.hline(x.clone(), y, Stroke::new(1.0, pressed));
    p.hline(x, y + 1.0, Stroke::new(1.0, lip));
}

/// The title set in the serif, centred, up to four lines; returns its foot.
fn title(p: &Painter, book: &Book, column: Rect, ink: Color32) -> f32 {
    let text = book.title.value.trim();
    let size = (column.width() * 0.13).clamp(12.0, 26.0);
    let top = column.top() + column.height() * 0.21;
    let middle = column.center().x;
    let shaped = crate::shaped::block(p.ctx(), text, size, column.width(), 4);
    if let Some(shaped) = shaped {
        let at = pos2(middle - shaped.size.x / 2.0, top);
        shaped.paint(p, at, ink);
        return top + shaped.size.y;
    }
    let format = TextFormat {
        font_id: theme::serif(size),
        color: ink,
        line_height: Some(size * 1.18),
        ..Default::default()
    };
    let mut job = LayoutJob::single_section(text.to_owned(), format);
    job.halign = Align::Center;
    job.wrap = TextWrapping {
        max_width: column.width(),
        max_rows: 4,
        break_anywhere: false,
        overflow_character: Some('…'),
    };
    let laid = p.layout_job(job);
    let rect = laid.rect;
    let at = pos2(middle - rect.center().x, top - rect.top());
    p.galley(at, laid, ink);
    top + rect.height()
}

/// A short rule each side of a small diamond, centred on `at`.
fn ornament(p: &Painter, at: Pos2, size: f32, ink: Color32) {
    let (r, reach, gap) = (size * 0.24, size * 1.5, size * 0.55);
    let diamond = vec![
        at + vec2(0.0, -r),
        at + vec2(r, 0.0),
        at + vec2(0.0, r),
        at + vec2(-r, 0.0),
    ];
    p.add(Shape::convex_polygon(diamond, ink, Stroke::NONE));
    let y = at.y.round() + 0.5;
    let stroke = Stroke::new(1.0, ink);
    p.hline(at.x - gap - reach..=at.x - gap, y, stroke);
    p.hline(at.x + gap..=at.x + gap + reach, y, stroke);
}

/// Small medium type, letter-spaced as an imprint is.
fn spaced(size: f32, color: Color32) -> TextFormat {
    TextFormat {
        font_id: theme::medium(size),
        color,
        extra_letter_spacing: size * 0.14,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_appearance::Rgb;

    #[test]
    fn every_cloth_tone_carries_gilt_type() {
        for hex in CLOTH {
            let tone = Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
            // The imprint: gilt at 80 % over the cloth.
            let mix = |g: u8, c: u8| (f32::from(g) * 0.8 + f32::from(c) * 0.2).round() as u8;
            let seen = Rgb(
                mix(GILT.r(), tone.0),
                mix(GILT.g(), tone.1),
                mix(GILT.b(), tone.2),
            );
            let ratio = seen.contrast(tone);
            assert!(ratio >= 4.5, "{hex:06x}: {ratio}");
        }
    }
}
