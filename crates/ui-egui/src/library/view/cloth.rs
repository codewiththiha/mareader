//! A cloth binding for a book without art: its tone, gilt and imprint.

use egui::text::LayoutJob;
use egui::{
    Align2, Color32, CornerRadius, Painter, Rect, Stroke, StrokeKind, TextFormat, pos2, vec2,
};
use mareader_library::Book;

use super::face::{rounded, shade};
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
/// A cloth cover's frame: in this share of its width, more by the spine.
const FRAME: f32 = 0.08;
const SPINE: f32 = 0.04;
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

/// A cloth binding: its tone lit from above, a gilt frame and initial.
pub(super) fn cloth(
    p: &Painter,
    rect: Rect,
    book: &Book,
    (round, small): (CornerRadius, bool),
    fade: f32,
    t: &Tokens,
) {
    p.rect_filled(rect, round, paper(book, t).gamma_multiply(fade));
    let light = (Color32::from_white_alpha(18), Color32::from_black_alpha(34));
    let window = rounded(rect, round);
    let (top, h) = (rect.top(), rect.height().max(1.0));
    shade(p, &window, |at| {
        let k = ((at.y - top) / h).clamp(0.0, 1.0);
        light.0.lerp_to_gamma(light.1, k).gamma_multiply(fade)
    });
    let gilt = |share: f32| GILT.gamma_multiply(share * fade);
    // The caption under the card names the book, so the cover doesn't.
    let initial = book
        .title
        .value
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    if small {
        let size = rect.width() * 0.55;
        let at = rect.center() + vec2(rect.width() * 0.03, 0.0);
        p.text(
            at,
            Align2::CENTER_CENTER,
            initial,
            theme::serif(size),
            gilt(0.92),
        );
        return;
    }
    let inset = rect.width() * FRAME;
    let frame = Rect::from_min_max(
        rect.min + vec2(inset + rect.width() * SPINE, inset),
        rect.max - vec2(inset, inset),
    );
    p.rect_stroke(frame, 1.0, Stroke::new(1.0, gilt(0.3)), StrokeKind::Inside);
    let middle = frame.center().x;
    let size = (rect.width() * 0.36).clamp(16.0, 88.0);
    let at = pos2(middle, rect.top() + rect.height() * 0.4);
    let glyph = p.text(
        at,
        Align2::CENTER_CENTER,
        initial,
        theme::serif(size),
        gilt(0.92),
    );
    let rule = Rect::from_center_size(
        pos2(middle, glyph.bottom() + size * 0.18),
        vec2((rect.width() * 0.14).max(10.0), 1.0),
    );
    p.rect_filled(rule, 0.0, gilt(0.55));
    // The format, small and spaced, as an imprint at the foot.
    let size = (rect.width() / 17.0).clamp(8.5, 11.0);
    let format = TextFormat {
        font_id: theme::medium(size),
        color: gilt(0.8),
        extra_letter_spacing: size * 0.14,
        ..Default::default()
    };
    let job = LayoutJob::single_section(book.format.label().to_owned(), format);
    let imprint = p.layout_job(job);
    let foot = frame.bottom() - inset * 0.7 - imprint.size().y;
    p.galley(
        pos2(middle - imprint.size().x / 2.0, foot),
        imprint,
        gilt(0.8),
    );
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
