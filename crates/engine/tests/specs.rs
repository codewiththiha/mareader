//! The registry as a ledger cites it: ids that are dotted and unique, a row
//! for every command there is, and rows that agree with the pages.

use mareader_appearance::Appearance;
use mareader_engine::appearance_menu::QuickText;
use mareader_engine::command::{Command, Independence};
use mareader_engine::command_spec::SPECS;
use mareader_engine::dict::{Action as DictAction, Download};
use mareader_engine::export::Format;
use mareader_engine::page_ai::Action as PageAiAction;
use mareader_engine::pane::{Arrow, Step, ViewMode, ZoomCommand};
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::read_aloud::player::Act as ReadAct;
use mareader_engine::route::Route;
use mareader_engine::settings::Tab;
use mareader_engine::space::Action as SpaceAction;
use mareader_engine::study::Action as StudyAction;
use mareader_engine::study_mode::Action as ModeAction;
use mareader_engine::updates::Action as UpdateAction;
use mareader_library::{BookId, Pen, View};
use mareader_speech::{Item, KOKORO, Spot};
use mareader_workspace::{Bounds, Edge, PaneId, SplitAxis};

#[test]
fn ids_are_unique_dotted_and_labels_say_something() {
    let mut seen = std::collections::BTreeSet::new();
    for spec in SPECS {
        assert!(seen.insert(spec.id), "duplicate id {}", spec.id);
        assert!(!spec.label.trim().is_empty(), "{} has no label", spec.id);
        let parts: Vec<&str> = spec.id.split('.').collect();
        assert!(parts.len() >= 2, "{} is not dotted", spec.id);
        let named = |part: &str| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        };
        assert!(parts.iter().all(|p| named(p)), "{} is not dotted", spec.id);
    }
}

/// One of every variant, so a row `id` never returns is caught here.
///
/// `id` is an exhaustive match, so a new variant cannot build without a row;
/// what that cannot show is a row the match never names. Building each
/// variant and asking for its spec does.
#[test]
fn every_variant_has_a_row() {
    let intents = every_variant();
    assert_eq!(intents.len(), SPECS.len(), "a variant has no row");
    for intent in &intents {
        let Some(spec) = intent.spec() else {
            panic!("{} has no row", intent.id());
        };
        assert!(!spec.title.is_empty(), "{} has no title", spec.id);
    }
}

/// One of every variant, in the five families the rows are written in.
fn every_variant() -> Vec<Command> {
    let mut intents = pane_intents();
    intents.extend(page_intents());
    intents.extend(mark_intents());
    intents.extend(reading_intents());
    intents.extend(settings_intents());
    intents
}

/// The rows that read no key at all: a script sends them an empty object.
const BARE: &[&str] = &[
    "page.autoscroll",
    "page.first",
    "page.last",
    "read.stop",
    "read.toggle",
    "voice.stop-try",
    "voice.try",
    "word.stop",
];

#[test]
fn a_row_says_which_keys_it_reads_and_no_more() {
    for spec in SPECS {
        assert_eq!(
            spec.params.is_empty(),
            BARE.contains(&spec.id),
            "{} reads {} key(s)",
            spec.id,
            spec.params.len()
        );
        for (key, kind) in spec.params {
            assert!(
                !key.is_empty() && !kind.is_empty(),
                "{} has a blank",
                spec.id
            );
        }
    }
}

/// A row's `enabled` is `Route::takes` asked of the row, so the two agree on
/// every command and every page: a script can ask before it builds.
#[test]
fn a_row_says_which_page_takes_its_command() {
    let routes = [
        Route::Reader,
        Route::Library,
        Route::Dictionary,
        Route::Space,
    ];
    for intent in every_variant() {
        let Some(spec) = intent.spec() else {
            panic!("{} has no row", intent.id());
        };
        for route in routes {
            assert_eq!(
                (spec.enabled)(route),
                route.takes(&intent),
                "{} on {route:?}",
                spec.id
            );
        }
    }
}

/// What lands in a pane, what leaves one, and the library that fills them.
fn pane_intents() -> Vec<Command> {
    let pane = PaneId(1);
    let book = BookId(1);
    vec![
        Command::Split(SplitAxis::Horizontal),
        Command::FocusPane(pane),
        Command::SwapPanes {
            pane,
            other: PaneId(2),
        },
        Command::MovePane {
            pane,
            other: PaneId(2),
            edge: Edge::Right,
            area: Bounds::new(0.0, 0.0, 900.0, 700.0),
        },
        Command::CyclePane { forward: true },
        Command::ClosePane(pane),
        Command::SetRatio(vec![], 0.5),
        Command::Zoom(ZoomCommand::Step(Step::In)),
        Command::View(ViewMode::Single),
        Command::Open(mareader_engine::documents::OpenRequest::bytes(
            vec![],
            "a.pdf",
        )),
        Command::OpenBook(book),
        Command::OpenMark(mareader_library::MarkId(9)),
        Command::PlaceBook {
            book,
            pane: Some(pane),
            placement: mareader_engine::documents::Placement::Into,
        },
        Command::ForgetPositions {
            books: vec![],
            paths: vec![],
        },
        Command::CopyPages {
            pane,
            pages: vec![],
        },
        Command::Export {
            name: String::new(),
            format: Format::Markdown,
            contents: String::new(),
        },
        Command::Library(mareader_engine::library::Action::ShowAdd),
    ]
}

/// What moves the page a pane shows.
fn page_intents() -> Vec<Command> {
    let pane = PaneId(1);
    vec![
        Command::Arrow(Arrow::Left),
        Command::Page { forward: true },
        Command::AutoScroll,
        Command::FirstPage,
        Command::LastPage,
        Command::GoTo { pane, page: 0 },
        Command::Turn {
            pane,
            forward: true,
        },
        Command::Search {
            pane,
            query: String::new(),
        },
        Command::WordList {
            pane,
            pages: vec![],
        },
        Command::WordListWindow {
            pane,
            pages: vec![],
        },
    ]
}

/// What the reader marks, and what a marked word is asked.
fn mark_intents() -> Vec<Command> {
    let pane = PaneId(1);
    vec![
        Command::Bookmark(Some(pane)),
        Command::ClearSelection(pane),
        Command::Highlight {
            pane,
            pen: Pen::default(),
            color: None,
        },
        Command::Note(pane),
        Command::SelectText {
            pane,
            page: 0,
            text: String::new(),
        },
        Command::Explain(pane),
        Command::LookUp(pane),
        Command::Study(StudyAction::Close),
        Command::StudyMode(ModeAction::Off(pane)),
        Command::Space(SpaceAction::Forget(Vec::new())),
        Command::ShowSpace(mareader_engine::space::Room::Notes),
        Command::Dict(DictAction::CloseCard),
        Command::Vocab(Download::Start),
        #[cfg(not(target_arch = "wasm32"))]
        Command::AttachVocab(std::path::PathBuf::from("words")),
        Command::Pronunciation(Download::Start),
    ]
}

/// What is heard, and where the voice that says it comes from.
fn reading_intents() -> Vec<Command> {
    let pane = PaneId(1);
    // A const, so the borrow is 'static as `RevealVoice` needs.
    let model: &'static mareader_speech::Model = &KOKORO;
    vec![
        Command::ReadAloud {
            pane,
            from: ReadFrom::default(),
        },
        Command::ReadAloudAct(ReadAct::Toggle),
        Command::ToggleReadAloud,
        Command::StopReadAloud,
        Command::PlaceReadMarker {
            pane,
            spot: Spot::default(),
        },
        Command::SayWord(String::new()),
        Command::SayWordWith {
            word: String::new(),
            voice: String::new(),
        },
        Command::StopSaying,
        Command::TryVoice,
        Command::TryVoiceOf(String::new()),
        Command::StopVoiceTry,
        Command::RevealVoice(model, "en"),
        Command::VoiceModel(Item::Model("kokoro"), Download::Start),
    ]
}

/// What is kept between launches, and the AI's own asks.
fn settings_intents() -> Vec<Command> {
    let pane = PaneId(1);
    let quick = QuickText::of(&mareader_settings::TextSettings::default());
    vec![
        Command::Batch(Vec::new()),
        Command::OpenSettings(Tab::Appearance),
        Command::SetSettings(Box::default()),
        Command::SetField {
            path: String::new(),
            value: String::new(),
        },
        Command::SetAppearance(Appearance::default()),
        Command::SetQuickText(quick),
        Command::SetInkContrast(1.0),
        Command::SetIndependence(Independence {
            themes: false,
            textures: false,
        }),
        Command::SavePreset {
            name: String::new(),
            group: String::new(),
            look: Appearance::default(),
        },
        Command::DeletePreset(String::new()),
        Command::SetDictionary(Box::default()),
        Command::SetLibraryView(View::default()),
        Command::SetSidebarOverlay(false),
        Command::SetTitlebarPinned(false),
        Command::ShowRoute(Route::Reader),
        Command::PageAi(PageAiAction::StopFollow(pane)),
        Command::Update(UpdateAction::Check { manual: true }),
        #[cfg(not(target_arch = "wasm32"))]
        Command::Ai(mareader_settings::Job::Probe),
    ]
}
