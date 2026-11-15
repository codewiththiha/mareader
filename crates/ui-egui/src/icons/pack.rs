//! Thin semantic-icon adapter for the generated animated pack.

use mareader_animated_icons::Pack;

use super::Icon;

pub(crate) fn of(icon: Icon) -> Option<&'static Pack> {
    let id = match icon {
        Icon::Sidebar => "panel-left",
        Icon::SidebarClose => "panel-left-close",
        Icon::Aside => "panel-right",
        Icon::SplitRight => "columns-2",
        Icon::Thumbnails => "layout-grid",
        Icon::Grid => "layout-grid",
        Icon::Layout => "layout-template",
        Icon::ScrollVertical => "gallery-vertical",
        Icon::ScrollHorizontal => "gallery-horizontal",
        Icon::Menu => "menu",
        Icon::Close => "x",
        Icon::WindowMinimize => "minus",
        Icon::Minus => "minus",
        Icon::Plus => "plus",
        Icon::WindowMaximize => "square",
        Icon::WindowRestore => "copy",
        Icon::Copy => "copy",
        Icon::Fullscreen => "maximize",
        Icon::ExitFullscreen => "minimize",
        Icon::Chromeless => "eye-off",
        Icon::Keyboard => "keyboard",
        Icon::ViewOptions => "sliders-horizontal",
        Icon::Pin => "pin",
        Icon::Settings => "settings",
        Icon::Book => "book-open",
        Icon::Spread => "book-open",
        Icon::Follow => "book-open-text",
        Icon::Dictionary => "book-a",
        Icon::LookUp => "book-search",
        Icon::Import => "book-plus",
        Icon::Bookmark => "bookmark",
        Icon::ZoomIn => "zoom-in",
        Icon::ZoomOut => "zoom-out",
        Icon::Search => "search",
        Icon::FitWidth => "move-horizontal",
        Icon::FitPage => "shrink",
        Icon::SinglePage => "rectangle-vertical",
        Icon::Type => "type",
        Icon::Figure => "image",
        Icon::Table => "table",
        Icon::Levels => "chart-column-increasing",
        Icon::ChevronLeft => "chevron-left",
        Icon::ChevronRight => "chevron-right",
        Icon::ChevronUp => "chevron-up",
        Icon::ChevronDown => "chevron-down",
        Icon::AutoScroll => "arrow-down",
        Icon::MoveDown => "arrow-down",
        Icon::MoveUp => "arrow-up",
        Icon::Sort => "arrow-down-up",
        Icon::Swap => "arrow-right-left",
        Icon::LeaveTable => "arrow-right-from-line",
        Icon::Restore => "undo-2",
        Icon::Rescan => "refresh-cw",
        Icon::Repeat => "repeat",
        Icon::Link => "link",
        Icon::List => "list",
        Icon::Outline => "list-tree",
        Icon::SelectSeveral => "list-checks",
        Icon::More => "ellipsis",
        Icon::Library => "library-big",
        Icon::Shelf => "square-library",
        Icon::Folder => "folder",
        Icon::Open => "folder-open",
        Icon::FolderPlus => "folder-plus",
        Icon::SearchFolder => "folder-search",
        Icon::Folders => "folders",
        Icon::ChooseFile => "file-search",
        Icon::Offline => "hard-drive",
        Icon::Download => "download",
        Icon::Remove => "trash-2",
        Icon::Resume => "play",
        Icon::Try => "play",
        Icon::Pause => "pause",
        Icon::SkipBack => "skip-back",
        Icon::SkipForward => "skip-forward",
        Icon::Stop => "circle-stop",
        Icon::ReadAloud => "headphones",
        Icon::VoiceModel => "audio-waveform",
        Icon::Speak => "volume-2",
        Icon::PageNumber => "hash",
        Icon::Light => "sun",
        Icon::Dark => "moon",
        Icon::Dim => "sun-moon",
        Icon::Black => "eclipse",
        Icon::Appearance => "palette",
        Icon::Motion => "wind",
        Icon::Show => "eye",
        Icon::Highlight => "highlighter",
        Icon::HalfHighlight => "baseline",
        Icon::Underline => "underline",
        Icon::Rename => "pencil",
        Icon::Note => "sticky-note",
        Icon::Ai => "ai-mark",
        Icon::Chip => "cpu",
        Icon::Server => "server",
        Icon::Key => "key-round",
        Icon::Ban => "ban",
        Icon::Translate => "languages",
        Icon::Simplify => "feather",
        Icon::Summarize => "scroll-text",
        Icon::Check => "check",
        Icon::Done => "circle-check",
        Icon::Alert => "circle-alert",
        Icon::Clock => "clock",
        _ => return voice(icon),
    };
    mareader_animated_icons::find(id)
}

fn voice(icon: Icon) -> Option<&'static Pack> {
    let id = match icon {
        Icon::Kokoro => "kokoro",
        Icon::Piper => "piper",
        Icon::Kitten => "kitten",
        Icon::Phonon => "phonon",
        Icon::SystemVoice => "system",
        Icon::SilentEngine => "silent",
        _ => return None,
    };
    mareader_animated_icons::find(id)
}

#[cfg(test)]
mod pack_tests {
    use super::*;

    #[test]
    fn ai_and_every_voice_use_generated_packs() {
        let expected = [
            (Icon::Ai, "ai-mark"),
            (Icon::Kokoro, "kokoro"),
            (Icon::Piper, "piper"),
            (Icon::Kitten, "kitten"),
            (Icon::Phonon, "phonon"),
            (Icon::SystemVoice, "system"),
            (Icon::SilentEngine, "silent"),
        ];
        for (icon, id) in expected {
            assert_eq!(of(icon).map(|pack| pack.id()), Some(id));
            assert!(icon.svg().is_none());
        }
    }
}
