//! Each settings tab's icon in the window's sidebar.

use mareader_engine::settings::Tab;

use crate::icons::Icon;

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
            Tab::Models => Icon::Chip,
            Tab::Panes => Icon::SplitRight,
            Tab::Motion => Icon::Motion,
            Tab::Updates => Icon::Download,
            Tab::Study => Icon::Key,
            Tab::Model(model) => Icon::engine(model.id).unwrap_or(Icon::VoiceModel),
        }
    }
}
