//! The settings window's sidebar: a group's heading, and a tab's row with
//! its icon.

use egui::{Color32, Rect, RichText, Sense, Ui, Vec2, pos2, vec2};
use mareader_engine::settings::Tab;

use crate::icons::Icon;
use crate::icons::anim::{self, Act};
use crate::theme::{self, Tokens};

/// A small muted heading over a group of tabs.
pub(super) fn nav_heading(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        let text = RichText::new(text)
            .font(theme::medium(t.text_small))
            .color(t.muted);
        ui.label(text);
    });
    ui.add_space(4.0);
}

/// A tab in the sidebar: its icon and name, filled while shown.
pub(super) fn nav_row(ui: &mut Ui, tab: Tab, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let size = vec2(ui.available_width(), super::NAV_ROW_H);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let label = tab.label();
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, label));
    let fill = if on {
        t.selected
    } else if r.hovered() {
        t.hover
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, t.radius, fill);
    let icon = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(16.0));
    let tint = if on { t.accent } else { t.muted };
    // Stable row names preserve runs while earlier groups fold.
    let _act = anim::announce(ui, Act::Click);
    ui.push_id(("settings-nav", label), |ui| {
        anim::paint(ui, icon, tab.icon(), tint, label, &r);
    });
    let font = if on {
        theme::medium(t.text)
    } else {
        egui::FontId::proportional(t.text)
    };
    let at = pos2(rect.left() + 38.0, rect.center().y);
    ui.painter()
        .text(at, egui::Align2::LEFT_CENTER, label, font, t.ink);
    r
}

/// A tab's icon: presentation, so it stays above the line.
pub(crate) trait TabIcon {
    fn icon(self) -> Icon;
}

impl TabIcon for Tab {
    fn icon(self) -> Icon {
        match self {
            Tab::Appearance => Icon::Appearance,
            Tab::Layout => Icon::Layout,
            Tab::ReadAloud => Icon::ReadAloud,
            Tab::Highlights => Icon::Highlight,
            Tab::Vocabulary => Icon::Levels,
            Tab::Dictionary => Icon::Dictionary,
            Tab::Ai => Icon::Ai,
            Tab::Skills => Icon::SelectSeveral,
            Tab::Models => Icon::Chip,
            Tab::Panes => Icon::SplitRight,
            Tab::Motion => Icon::Motion,
            Tab::Updates => Icon::Download,
            Tab::Study => Icon::Key,
            Tab::Model(model) => Icon::engine(model.id).unwrap_or(Icon::VoiceModel),
        }
    }
}
