//! Paints the spoken-word highlight over visible pages.

use egui::{Color32, CornerRadius, Ui};
use mareader_engine::read_aloud::lit::{Lit, Span};
use mareader_session::Anchor;

use crate::selection::Pages;
use crate::theme::Tokens;

/// The words read on the pages shown, in the accent's tint.
pub fn paint(ui: &Ui, pages: &Pages<'_>, lit: &Lit, t: &Tokens) {
    let fill = fill(t);
    let painter = ui.painter();
    for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let mut runs = Vec::new();
        match lit.span {
            Span::Anchors { start, end } => {
                let inside = |at: Option<(u32, u32)>| {
                    at.is_some_and(|(b, i)| {
                        let at = Anchor {
                            block: b as usize,
                            offset: i as usize,
                        };
                        start <= at && at < end
                    })
                };
                let mut from = None;
                let count = layer.places.len();
                for i in 0..=count {
                    let on = i < count && inside(layer.places.get(i).copied().flatten());
                    match (on, from) {
                        (true, None) => from = Some(i),
                        (false, Some(f)) => {
                            runs.push(f..i);
                            from = None;
                        }
                        _ => {}
                    }
                }
            }
            Span::Chars { start, end } => {
                let page = sheet.page;
                if (start.page..=end.page).contains(&page) {
                    let from = if page == start.page { start.char } else { 0 };
                    let to = if page == end.page {
                        end.char
                    } else {
                        layer.text.len()
                    };
                    runs.push(from..to.min(layer.text.len()));
                }
            }
        }
        for run in runs.into_iter().filter(|r| !r.is_empty()) {
            for b in layer.text.line_boxes(run) {
                let r = sheet.to_screen(b).intersect(sheet.shown);
                if r.is_positive() {
                    painter.rect_filled(r, CornerRadius::same(2), fill);
                }
            }
        }
    }
}

/// The accent toned to the paper so ink remains readable.
fn fill(t: &Tokens) -> Color32 {
    let paper = t.book.paper.map(|c| c.clamp(0.0, 1.0) * 255.0);
    tint(paper, [t.accent.r(), t.accent.g(), t.accent.b()])
}

/// A faint share of `colour` over `paper`, fainter on a dark page.
fn tint(paper: [f32; 3], colour: [u8; 3]) -> Color32 {
    let light = paper.iter().sum::<f32>() > 3.0 * 128.0;
    let share = if light { 0.30 } else { 0.26 };
    let target = std::array::from_fn(|i| {
        let p = paper.get(i).copied().unwrap_or(255.0);
        p + (f32::from(colour.get(i).copied().unwrap_or(255)) - p) * share
    });
    over(paper, target)
}

/// The darkest fill that turns `paper` into `target`, so ink stays dark.
fn over(paper: [f32; 3], target: [f32; 3]) -> Color32 {
    let pairs = || paper.into_iter().zip(target);
    let darker = |(p, t): (f32, f32)| (p - t).max(0.0) / p.max(1.0);
    let lighter = |(p, t): (f32, f32)| (t - p).max(0.0) / (255.0 - p).max(1.0);
    // Darkening sets the strength; lightening only when nothing darkens.
    let dark = pairs().map(darker).fold(0.0_f32, f32::max);
    let need = match dark > 0.01 {
        true => dark,
        false => pairs().map(lighter).fold(0.0_f32, f32::max),
    };
    let alpha = need.clamp(0.12, 1.0);
    let channel = |(p, t): (f32, f32)| ((t - (1.0 - alpha) * p) / alpha).clamp(0.0, 255.0);
    let [r, g, b] = {
        let mut out = [0u8; 3];
        for (slot, pair) in out.iter_mut().zip(pairs()) {
            *slot = channel(pair).round() as u8;
        }
        out
    };
    Color32::from_rgba_unmultiplied(r, g, b, (alpha * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `fill` over `under` shows, as egui blends it.
    fn shown(fill: Color32, under: f32) -> [f32; 3] {
        let a = f32::from(fill.a()) / 255.0;
        let c = [fill.r(), fill.g(), fill.b()];
        c.map(|c| f32::from(c) + under * (1.0 - a))
    }

    #[test]
    fn the_tint_on_a_tinted_page_leaves_the_ink_readable() {
        // A pink page: a book's pure yellow once covered the words here.
        let pink = [255.0, 214.0, 232.0];
        let fill = tint(pink, [0x25, 0x63, 0xeb]);
        assert!(fill.a() <= 80, "{fill:?}");
        let ink = shown(fill, 31.0);
        assert!(ink.iter().all(|c| *c < 110.0), "{ink:?}");
    }

    #[test]
    fn the_fill_turns_paper_to_its_target_and_keeps_ink_dark() {
        let fill = over([250.0; 3], [255.0, 255.0, 187.0]);
        let paper = shown(fill, 250.0);
        assert!(
            (paper[2] - 187.0).abs() < 2.0 && paper[0] > 248.0,
            "{paper:?}"
        );
        let ink = shown(fill, 0.0);
        assert!(ink.iter().all(|c| *c < 90.0), "{ink:?}");
        // On a dark page the tint lightens, and is faint.
        let fill = over([30.0; 3], [60.0, 60.0, 40.0]);
        assert!(fill.a() < 64, "{fill:?}");
        assert!((shown(fill, 30.0)[0] - 60.0).abs() < 2.0);
    }
}
