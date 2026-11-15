//! Document search drawn: every hit on the pages shown, the current amber.

use egui::{Color32, CornerRadius, Stroke, StrokeKind, Ui};
pub(crate) use mareader_engine::search::{Found, Lit, Search};

use crate::selection::Pages;
use crate::theme::Tokens;

/// mareader's current-match amber, deliberately not the accent.
const AMBER: Color32 = Color32::from_rgb(0xf5, 0x9e, 0x0b);
/// Lifted for dark pages.
const AMBER_DARK: Color32 = Color32::from_rgb(0xfb, 0xbf, 0x24);

/// Every hit on the pages shown in the accent; the current one amber.
pub fn paint(ui: &Ui, pages: &Pages<'_>, lit: &Lit, t: &Tokens) {
    let (all, amber, strength) = if t.dark {
        (t.accent.gamma_multiply(0.26), AMBER_DARK, 0.46)
    } else {
        (t.accent.gamma_multiply(0.30), AMBER, 0.58)
    };
    let painter = ui.painter();
    for sheet in pages.sheets.iter().filter(|s| s.shown.is_positive()) {
        let Some(layer) = pages.texts.get(pages.doc, sheet.page) else {
            continue;
        };
        let from = lit.hits.partition_point(|h| h.page < sheet.page);
        let on_page = lit.hits.iter().enumerate().skip(from);
        for (i, hit) in on_page.take_while(|(_, h)| h.page == sheet.page) {
            let on = lit.active == Some(i);
            for b in layer.text.line_boxes(hit.chars.clone()) {
                let r = sheet.to_screen(b).intersect(sheet.shown);
                if !r.is_positive() {
                    continue;
                }
                if on {
                    let ring = Stroke::new(1.5, amber.gamma_multiply(0.85));
                    let fill = amber.gamma_multiply(strength);
                    painter.rect(r, CornerRadius::same(3), fill, ring, StrokeKind::Outside);
                } else {
                    painter.rect_filled(r, CornerRadius::same(2), all);
                }
            }
        }
    }
}
