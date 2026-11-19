//! What the settings window is showing: a tab's name, not its drawing.

/// Which page of the settings window is open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    /// Colours, a book's text and a PDF's paper.
    #[default]
    Appearance,
    Layout,
    /// The engine, its voice and how books are read aloud.
    ReadAloud,
    /// The pens, the selection menu and the AI's reserved look.
    Highlights,
    Vocabulary,
    Dictionary,
    StudyMode,
    /// What the AI does: the AI Dictionary, the library, pages.
    Ai,
    /// The models tasks ask, and their providers' keys.
    Models,
    Panes,
    /// The master switch, then each motion.
    Motion,
    /// Software update checks, auto-update, and version status.
    Updates,
    /// A downloaded voice model's voices; shown once it is ready.
    Model(&'static mareader_speech::Model),
}

impl Tab {
    pub const ALL: [Tab; 12] = [
        Tab::Appearance,
        Tab::Layout,
        Tab::ReadAloud,
        Tab::Highlights,
        Tab::Vocabulary,
        Tab::Dictionary,
        Tab::StudyMode,
        Tab::Ai,
        Tab::Models,
        Tab::Panes,
        Tab::Motion,
        Tab::Updates,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tab::Appearance => "Appearance",
            Tab::Layout => "Layout",
            Tab::ReadAloud => "Read aloud",
            Tab::Highlights => "Highlights",
            Tab::Vocabulary => "Vocabulary",
            Tab::Dictionary => "Dictionary",
            Tab::StudyMode => "Study mode",
            Tab::Ai => "AI",
            Tab::Models => "AI models",
            Tab::Panes => "Panes",
            Tab::Motion => "Motion",
            Tab::Updates => "Updates",
            Tab::Model(m) => m.name,
        }
    }

    /// A tab by its name; old names, as Workspace, still work.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.to_ascii_lowercase();
        match s.as_str() {
            "theme" | "fonts" => Some(Tab::Appearance),
            "animations" => Some(Tab::Motion),
            "workspace" => Some(Tab::Panes),
            "models" => Some(Tab::Models),
            "voices" | "read_aloud" | "speech" => Some(Tab::ReadAloud),
            "update" | "updates" => Some(Tab::Updates),
            _ => Self::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(&s)),
        }
    }

    /// The heading the sidebar groups it under.
    pub fn group(self) -> &'static str {
        match self {
            Tab::Appearance | Tab::Layout | Tab::ReadAloud | Tab::Model(_) => "Reading",
            Tab::Highlights | Tab::Vocabulary | Tab::Dictionary | Tab::StudyMode | Tab::Ai => {
                "Study"
            }
            Tab::Models | Tab::Panes | Tab::Motion | Tab::Updates => "App",
        }
    }

    /// Shown away from the books too, in the library and the dictionary.
    pub fn beyond_books(self) -> bool {
        use Tab as T;
        let study = matches!(
            self,
            T::Highlights | T::Vocabulary | T::Dictionary | T::StudyMode
        );
        let voices = matches!(self, T::ReadAloud | T::Model(_));
        study || voices || matches!(self, T::Ai | T::Models | T::Updates)
    }

    /// Every tab always shows, so the list never shifts under the pointer.
    pub fn available(self) -> bool {
        !matches!(self, Tab::Ai | Tab::Models | Tab::Updates) || cfg!(not(target_arch = "wasm32"))
    }
}
