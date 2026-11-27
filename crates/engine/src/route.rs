//! Which page fills the window, and which commands each one takes.

use mareader_settings::Bar;

use crate::command::Command;

/// The page in the window: the panes being read, the library, the
/// dictionary, or Space's studied words.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Route {
    #[default]
    Reader,
    Library,
    Dictionary,
    Space,
}

impl Route {
    /// The title bar it shows, each with its own pin; Space shares the
    /// library's.
    pub fn bar(self) -> Bar {
        match self {
            Route::Reader => Bar::Reader,
            Route::Library | Route::Space => Bar::Library,
            Route::Dictionary => Bar::Dictionary,
        }
    }

    /// The rail serves reading; the library has none.
    pub fn has_rail(self) -> bool {
        self == Route::Reader
    }
}

impl Route {
    /// Whether `command` means anything on this page. The rest are dropped.
    ///
    /// `reads` is exhaustive over `Command`, so a variant added there has to
    /// say which pages it belongs to.
    pub fn takes(self, intent: &Command) -> bool {
        self == Route::Reader || !self.reads(intent)
    }

    /// Whether `command` acts on what only the reader shows: panes, pages,
    /// a book being read, or the pixels of one.
    fn reads(self, intent: &Command) -> bool {
        use Command as I;
        match intent {
            I::Zoom(_) | I::View(_) | I::Arrow(_) | I::Page { .. } => true,
            I::FirstPage | I::LastPage | I::AutoScroll => true,
            I::GoTo { .. } | I::Turn { .. } | I::CopyPages { .. } => true,
            I::WordList { .. } | I::WordListWindow { .. } => true,
            I::StudyMode(_) => true,
            I::Split(_) | I::FocusPane(_) | I::ClosePane(_) => true,
            I::MovePane { .. } | I::SwapPanes { .. } => true,
            I::SetRatio(..) | I::CyclePane { .. } | I::PlaceBook { .. } => true,
            I::SetIndependence(_) | I::SetInkContrast(_) | I::SetQuickText(_) => true,
            I::Explain(_) | I::Highlight { .. } | I::SelectText { .. } => true,
            I::ClearSelection(_) | I::Search { .. } => true,
            I::LookUp(_) | I::Note(_) | I::Bookmark(_) | I::PageAi(_) | I::Skill(_) => true,
            I::ReadAloud { .. } | I::ToggleReadAloud | I::StopReadAloud => true,
            I::PlaceReadMarker { .. } => true,
            // Settings, voices and cards open over any page.
            I::ReadAloudAct(_) => false,
            I::OpenSettings(_) | I::SetSettings(_) | I::SetField { .. } => false,
            I::TryVoice | I::TryVoiceOf(_) | I::StopVoiceTry | I::VoiceModel(..) => false,
            I::SayWord(_) | I::SayWordWith { .. } | I::StopSaying => false,
            I::RevealVoice(..) | I::Study(_) | I::Pronunciation(_) => false,
            I::Dict(_) | I::SetDictionary(_) | I::Vocab(_) => false,
            // The library's own, and what finishes wherever the window went.
            I::OpenBook(_)
            | I::OpenMark(_)
            | I::OpenAiSelection(_)
            | I::SetLibraryView(_)
            | I::Library(_) => false,
            I::ForgetPositions { .. } | I::Open(_) | I::Export { .. } => false,
            I::SetAppearance(_) | I::SavePreset { .. } | I::DeletePreset(_) => false,
            I::SetTitlebarPinned(_) | I::SetSidebarOverlay(_) | I::ShowRoute(_) => false,
            I::Batch(_) | I::Update(_) | I::Space(_) | I::ShowSpace(_) => false,
            #[cfg(not(target_arch = "wasm32"))]
            I::Ai(_) | I::AttachVocab(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_drops_what_acts_on_panes() {
        assert!(!Route::Library.takes(&Command::View(crate::pane::ViewMode::Single)));
        assert!(!Route::Library.takes(&Command::FirstPage));
        assert!(Route::Library.takes(&Command::OpenSettings(crate::settings::Tab::Panes)));
        assert!(Route::Reader.takes(&Command::FirstPage));
    }

    #[test]
    fn the_dictionary_keeps_its_settings_and_drops_reading() {
        assert!(!Route::Dictionary.takes(&Command::Bookmark(None)));
        assert!(Route::Dictionary.takes(&Command::OpenBook(mareader_library::BookId(1))));
        assert!(!Route::Dictionary.has_rail());
    }

    #[test]
    fn space_acts_on_words_and_drops_reading() {
        let forget = Command::Space(crate::space::Action::Forget(Vec::new()));
        assert!(Route::Space.takes(&forget));
        assert!(Route::Reader.takes(&forget));
        assert!(!Route::Space.takes(&Command::FirstPage));
        assert!(!Route::Space.has_rail());
        assert_eq!(Route::Space.bar(), Bar::Library);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn key_and_model_work_finishes_on_any_page() {
        let job = || Command::Ai(mareader_settings::Job::Probe);
        assert!(Route::Library.takes(&job()));
        assert!(Route::Dictionary.takes(&job()));
    }
}
