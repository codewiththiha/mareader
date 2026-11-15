//! A pane's document name or chapter, in its top-left corner.

use egui::{Context, Id, Label, LayerId, Order, Rect, RichText, WidgetInfo, WidgetType, pos2};
use mareader_pdf::{OutlineEntry, active_entry};
use mareader_settings::{FloatingLabelStyle, LayoutSettings};

use crate::documents::Reading;
use crate::motion;
use crate::theme::Tokens;
use crate::titlebar::FADE;

/// Inset from the pane's corner.
const INSET: f32 = 12.0;

/// The chrome the label makes way for. A floating sidebar just covers it.
#[derive(Debug, Clone, Copy)]
pub struct LabelChrome {
    /// The title bar's bottom while shown; the label stays below it.
    pub bar: Option<f32>,
}

/// The file's name, or the chapter read and else the name.
pub fn text(reading: &Reading, style: FloatingLabelStyle) -> String {
    match style {
        FloatingLabelStyle::Chapter => {
            chapter(reading.document.outline(), reading.current, &reading.name)
        }
        FloatingLabelStyle::FileName => reading.name.clone(),
    }
}

fn chapter(outline: &[OutlineEntry], page: usize, name: &str) -> String {
    active_entry(outline, page)
        .and_then(|i| outline.get(i))
        .map_or_else(|| name.to_owned(), |e| e.title.clone())
}

/// Room for the label: the page's blank side and a quarter of the page.
pub fn budget(pane: Rect, page: Option<Rect>) -> f32 {
    let Some(page) = page else {
        return 0.0;
    };
    let side = (page.left() - pane.left()).max(0.0);
    let over = if side > 1.0 { page.width() * 0.25 } else { 0.0 };
    side + over - 8.0
}

/// Shown while the title bar is away, if it fits beside the page.
pub fn show(
    ctx: &Context,
    pane: Rect,
    reading: &Reading,
    layout: &LayoutSettings,
    chrome: LabelChrome,
) {
    let id = Id::new(("floating-label", reading.pane));
    let text = text(reading, layout.floating_label_style);
    let t = Tokens::get(ctx);
    let font = egui::FontId::proportional(t.text_small);
    let painter = ctx.layer_painter(LayerId::new(Order::Middle, id));
    let width = painter.layout_no_wrap(text.clone(), font, t.muted).size().x;
    let limit = budget(pane, reading.page) * layout.floating_label_max_pct / 100.0;
    let persist = layout.floating_label_persist;
    let wanted = layout.floating_label && (persist || (chrome.bar.is_none() && width <= limit));
    let fade = ctx.animate_bool_with_time(id, wanted, motion::secs(ctx, FADE));
    if fade <= 0.0 {
        return;
    }
    let top = chrome.bar.map_or(pane.top(), |b| b.max(pane.top()));
    // Under the floating sidebar's Foreground, so it slides over the label.
    egui::Area::new(id)
        .fade_in(false)
        .order(Order::Middle)
        .fixed_pos(pos2(pane.left() + INSET, top + INSET * 0.75))
        .constrain(false)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(fade);
            ui.set_clip_rect(pane);
            ui.set_max_width((pane.width() - 2.0 * INSET).max(0.0));
            let rich = RichText::new(text.as_str())
                .size(t.text_small)
                .color(t.muted);
            let shown = ui.add(Label::new(rich).truncate());
            // Named apart from the outline entry that reads the same.
            let name = format!("Pane label: {text}");
            shown.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &name));
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_chapter_is_the_last_entry_begun_else_the_name() -> Result<(), String> {
        let r = crate::testing::reading(12, 6)?;
        assert_eq!(text(&r, FloatingLabelStyle::Chapter), "2.1 Scrolling");
        assert_eq!(text(&r, FloatingLabelStyle::FileName), "sample.pdf");
        assert_eq!(chapter(&[], 6, "sample.pdf"), "sample.pdf");
        Ok(())
    }

    #[test]
    fn the_budget_is_the_blank_side_and_a_quarter_page() {
        let pane = Rect::from_min_size(pos2(100.0, 0.0), vec2(1000.0, 800.0));
        let page = Rect::from_min_size(pos2(300.0, 20.0), vec2(600.0, 700.0));
        assert_eq!(budget(pane, Some(page)), 200.0 + 150.0 - 8.0);
        // Edge to edge, there is no side to borrow from.
        let full = Rect::from_min_size(pane.min, pane.size());
        assert!(budget(pane, Some(full)) < 0.0);
        assert!(budget(pane, None) <= 0.0);
    }
}
