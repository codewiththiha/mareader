//! The params a script sends, read against the registry: every shape a key
//! can hold builds the command its row names, and what a key holds that it
//! cannot says what it wanted.

use std::path::PathBuf;
use std::sync::Arc;

use mareader_appearance::Rgb;
use mareader_engine::appearance_menu::QuickText;
use mareader_engine::command::{Command, Independence};
use mareader_engine::command_spec::by_id;
use mareader_engine::dict::{Action as DictAction, Download};
use mareader_engine::documents::{OpenRequest, Placement};
use mareader_engine::export::Format;
use mareader_engine::host::Fake;
use mareader_engine::library::Action as LibraryAction;
use mareader_engine::page_ai::Action as PageAiAction;
use mareader_engine::pane::{Arrow, Fit, ViewMode, ZoomCommand};
use mareader_engine::read_aloud::ReadFrom;
use mareader_engine::read_aloud::player::Act as ReadAct;
use mareader_engine::route::Route;
use mareader_engine::session::Session;
use mareader_engine::settings::Tab;
use mareader_engine::skills::{Action as SkillAction, AskId};
use mareader_engine::space::Action as SpaceAction;
use mareader_engine::study::Action as StudyAction;
use mareader_engine::study_mode::{Action as ModeAction, Ask};
use mareader_library::{BookId, MarkId, Options, Pen, WordId};
use mareader_session::Anchor;
use mareader_settings::{PageTask, Skill, SkillId};
use mareader_speech::{Item, KOKORO, Model, Spot};
use mareader_study::mode::{Key, Mode, Pos};
use mareader_workspace::{Bounds, Edge, PaneId, SplitAxis};
use serde_json::Value;

/// A session with nothing open, which is what a script's params are read
/// against: a `pane` a script leaves out is the one this has in focus.
fn session() -> Session {
    Session::new(Arc::new(Fake::default()), Arc::new(|| {}))
}

#[test]
fn the_params_a_script_sends_build_the_command_it_names() {
    let at = session();
    for (id, params, want) in cases(&at) {
        let Some(spec) = by_id(id) else {
            panic!("no row for {id}");
        };
        let held: Value = serde_json::from_str(&params).expect("the case is JSON");
        assert_eq!((spec.parse)(&at, &held), Ok(want), "{id} from {params}");
    }
}

#[test]
fn a_key_holding_the_wrong_thing_says_what_it_wanted() {
    let at = session();
    let spec = by_id("page.go").expect("a row");
    let Err(why) = (spec.parse)(&at, &serde_json::json!({ "page": "three" })) else {
        panic!("a page number in words built a command");
    };
    assert!(
        why.contains("page") && why.contains("whole number"),
        "{why}"
    );
}

#[test]
fn an_ask_a_script_cannot_send_names_the_ones_it_can() {
    let at = session();
    let spec = by_id("library.act").expect("a row");
    let held = serde_json::json!({ "action": "unknown_ask" });
    let Err(why) = (spec.parse)(&at, &held) else {
        panic!("an ask no script can send built a command");
    };
    assert!(
        why.contains("library.act") && why.contains("show_add"),
        "{why}"
    );
}

#[test]
fn export_write_renders_active_pane_when_contents_omitted() {
    let at = session();
    let spec = by_id("export.write").expect("a row");
    let held = serde_json::json!({ "format": "markdown" });
    let Ok(Command::Export {
        name,
        format,
        contents,
    }) = (spec.parse)(&at, &held)
    else {
        panic!("export.write without contents failed to parse");
    };
    assert_eq!(format, Format::Markdown);
    assert!(name.ends_with(".md"), "{name}");
    assert!(contents.contains("# notes"), "{contents}");
}

/// One case per shape a param can take: a name, a number, a flag, a list, an
/// object, a path, a colour, a spot, a model's own `&'static` tables, and a
/// pane a script leaves to the session.
fn cases(at: &Session) -> Vec<(&'static str, String, Command)> {
    let pane = at.active();
    // A const, so the borrow is 'static as `RevealVoice` and `Tab` need.
    let model: &'static Model = &KOKORO;
    let mut dials = QuickText::of(&at.settings().text);
    dials.size_pct = 120.0;
    let mut opened = OpenRequest::path(PathBuf::from("/a.pdf"));
    opened.page = 2;
    vec![
        (
            "pane.split",
            r#"{"axis":"vertical"}"#,
            Command::Split(SplitAxis::Vertical),
        ),
        ("pane.focus", r#"{"pane":4}"#, Command::FocusPane(PaneId(4))),
        (
            "pane.cycle",
            r#"{"forward":false}"#,
            Command::CyclePane { forward: false },
        ),
        (
            "pane.swap",
            r#"{"other":2}"#,
            Command::SwapPanes {
                pane,
                other: PaneId(2),
            },
        ),
        (
            "pane.move",
            r#"{"other":2,"edge":"top","area":{"x":1.0,"y":2.0,"w":3.0,"h":4.0}}"#,
            Command::MovePane {
                pane,
                other: PaneId(2),
                edge: Edge::Top,
                area: Bounds::new(1.0, 2.0, 3.0, 4.0),
            },
        ),
        (
            "pane.ratio",
            r#"{"path":[true,false],"ratio":0.25}"#,
            Command::SetRatio(vec![true, false], 0.25),
        ),
        (
            "pane.view",
            r#"{"mode":"spread"}"#,
            Command::View(ViewMode::Spread),
        ),
        (
            "pane.zoom",
            r#"{"zoom":"fit-page"}"#,
            Command::Zoom(ZoomCommand::Fit(Fit::Page)),
        ),
        ("page.arrow", r#"{"arrow":"up"}"#, Command::Arrow(Arrow::Up)),
        ("page.go", r#"{"page":3}"#, Command::GoTo { pane, page: 3 }),
        (
            "page.turn",
            r#"{"forward":true}"#,
            Command::Turn {
                pane,
                forward: true,
            },
        ),
        (
            "page.copy",
            r#"{"pages":[1,2]}"#,
            Command::CopyPages {
                pane,
                pages: vec![1, 2],
            },
        ),
        ("page.first", "{}", Command::FirstPage),
        (
            "word.list-window",
            "{}",
            Command::WordListWindow {
                pane,
                pages: vec![],
            },
        ),
        (
            "reader.open",
            r#"{"path":"/a.pdf","page":2}"#,
            Command::Open(opened),
        ),
        (
            "reader.open",
            r#"{"bytes":[1,2],"name":"a.pdf"}"#,
            Command::Open(OpenRequest::bytes(vec![1, 2], "a.pdf")),
        ),
        (
            "reader.open-book",
            r#"{"book":7}"#,
            Command::OpenBook(BookId(7)),
        ),
        (
            "reader.open-mark",
            r#"{"mark":9}"#,
            Command::OpenMark(mareader_library::MarkId(9)),
        ),
        (
            "space.show",
            r#"{"room":"notes"}"#,
            Command::ShowSpace(mareader_engine::space::Room::Notes),
        ),
        (
            "reader.place-book",
            r#"{"book":7,"placement":"beside"}"#,
            Command::PlaceBook {
                book: BookId(7),
                pane: Some(pane),
                placement: Placement::Beside,
            },
        ),
        (
            "library.forget",
            r#"{"books":[7],"paths":["/a"]}"#,
            Command::ForgetPositions {
                books: vec![BookId(7)],
                paths: vec![PathBuf::from("/a")],
            },
        ),
        ("mark.bookmark", "{}", Command::Bookmark(None)),
        (
            "mark.highlight",
            r#"{"pen":"underline","color":[1,2,3]}"#,
            Command::Highlight {
                pane,
                pen: Pen::Underline,
                color: Some(Rgb(1, 2, 3)),
            },
        ),
        (
            "mark.select",
            r#"{"page":2,"text":"a"}"#,
            Command::SelectText {
                pane,
                page: 2,
                text: "a".into(),
            },
        ),
        (
            "read.marker",
            r#"{"at":{"page":4,"char":9}}"#,
            Command::PlaceReadMarker {
                pane,
                spot: Spot { page: 4, char: 9 },
            },
        ),
        (
            "read.start",
            r#"{"from":{"block":1,"offset":2},"until":{"page":3,"char":4}}"#,
            Command::ReadAloud {
                pane,
                from: ReadFrom {
                    anchor: Some(Anchor {
                        block: 1,
                        offset: 2,
                    }),
                    at: None,
                    until: Some(Spot { page: 3, char: 4 }),
                },
            },
        ),
        (
            "word.say-with",
            r#"{"word":"a","voice":"v"}"#,
            Command::SayWordWith {
                word: "a".into(),
                voice: "v".into(),
            },
        ),
        (
            "voice.model",
            r#"{"model":"kokoro","act":"start"}"#,
            Command::VoiceModel(Item::Model("kokoro"), Download::Start),
        ),
        (
            "voice.model",
            r#"{"model":"kokoro","voice":"af_alloy","act":"remove"}"#,
            Command::VoiceModel(Item::Voice("kokoro", "af_alloy"), Download::Remove),
        ),
        (
            "voice.reveal",
            r#"{"model":"kokoro","language":"en-US"}"#,
            Command::RevealVoice(model, "en-US"),
        ),
        (
            "vocab.download",
            r#"{"act":"pause"}"#,
            Command::Vocab(Download::Pause),
        ),
        (
            "settings.open",
            r#"{"tab":"panes"}"#,
            Command::OpenSettings(Tab::Panes),
        ),
        (
            "settings.open",
            r#"{"model":"kokoro"}"#,
            Command::OpenSettings(Tab::Model(model)),
        ),
        (
            "settings.set",
            r#"{"path":"chrome.glass","value":true}"#,
            Command::SetField {
                path: "chrome.glass".into(),
                value: "true".into(),
            },
        ),
        (
            "settings.ink",
            r#"{"ink":80}"#,
            Command::SetInkContrast(80.0),
        ),
        (
            "settings.independence",
            r#"{"themes":true}"#,
            Command::SetIndependence(Independence {
                themes: true,
                textures: false,
            }),
        ),
        (
            "settings.sidebar-overlay",
            r#"{"overlay":true}"#,
            Command::SetSidebarOverlay(true),
        ),
        (
            "route.show",
            r#"{"route":"library"}"#,
            Command::ShowRoute(Route::Library),
        ),
        (
            "settings.preset-delete",
            r#"{"name":"Night"}"#,
            Command::DeletePreset("Night".into()),
        ),
        (
            "settings.quick-text",
            r#"{"size_pct":120}"#,
            Command::SetQuickText(dials),
        ),
        (
            "export.write",
            r#"{"name":"notes","format":"text","contents":"a"}"#,
            Command::Export {
                name: "notes".into(),
                format: Format::Text,
                contents: "a".into(),
            },
        ),
        (
            "dict.act",
            r#"{"action":"pursue","word":"a"}"#,
            Command::Dict(DictAction::Pursue("a".into())),
        ),
        (
            "study.act",
            r#"{"action":"pick_all"}"#,
            Command::Study(StudyAction::PickAll),
        ),
        (
            "study.act",
            r#"{"action":"set_note","mark":42,"text":"key passage"}"#,
            Command::Study(StudyAction::SetNote(MarkId(42), "key passage".into())),
        ),
        (
            "study.mode",
            r#"{"action":"toggle","pane":1,"mode":"proactive"}"#,
            Command::StudyMode(ModeAction::Toggle {
                pane: PaneId(1),
                mode: Mode::Proactive,
            }),
        ),
        (
            "study.mode",
            r#"{"action":"reopen","pane":1}"#,
            Command::StudyMode(ModeAction::Reopen(PaneId(1))),
        ),
        (
            "study.mode",
            r#"{"action":"known","pane":1,"word":"arcane|adjective"}"#,
            Command::StudyMode(ModeAction::Known {
                pane: PaneId(1),
                key: Key::new("arcane", Some(Pos::Adjective)),
            }),
        ),
        (
            "study.mode",
            r#"{"action":"recall","pane":1,"ask":"open","page":2,"from":4,"to":9}"#,
            Command::StudyMode(ModeAction::Recall {
                pane: PaneId(1),
                ask: Ask::Open {
                    page: 2,
                    chars: 4..9,
                },
            }),
        ),
        (
            "space.word",
            r#"{"action":"forget","word":"lintel|noun"}"#,
            Command::Space(SpaceAction::Forget(vec![WordId::of(
                "lintel",
                Some("noun"),
            )])),
        ),
        (
            "library.act",
            r#"{"action":"rescan"}"#,
            Command::Library(LibraryAction::Rescan),
        ),
        (
            "library.act",
            r#"{"action":"import_folder","root":"/books","min_size":0,"groups":false}"#,
            Command::Library(LibraryAction::ImportFolder {
                root: PathBuf::from("/books"),
                opts: Options {
                    min_size: 0,
                    groups: false,
                    ..Options::default()
                },
            }),
        ),
        (
            "ai.page",
            r#"{"action":"stop_pages"}"#,
            Command::PageAi(PageAiAction::StopPages(pane)),
        ),
        (
            "ai.skill",
            r#"{"action":"run","skill":"custom-3"}"#,
            Command::Skill(SkillAction::Run {
                pane,
                skill: Skill::Custom(SkillId(3)),
            }),
        ),
        (
            "ai.skill",
            r#"{"action":"run_page","page":2,"x":10,"skill":"summarize"}"#,
            Command::Skill(SkillAction::RunPage {
                pane,
                page: 2,
                x: 10.0,
                y: 0.0,
                skill: Skill::Builtin(PageTask::Summarize),
            }),
        ),
        (
            "ai.skill",
            r#"{"action":"close","ask":4}"#,
            Command::Skill(SkillAction::Close(AskId(4))),
        ),
        ("mark.clear", r#"{}"#, Command::ClearSelection(pane)),
        (
            "page.search",
            r#"{"query":"fox"}"#,
            Command::Search {
                pane,
                query: "fox".into(),
            case_sensitive: false,
            },
        ),
        (
            "read.act",
            r#"{"action":"seek","share":0.5}"#,
            Command::ReadAloudAct(ReadAct::Seek(0.5)),
        ),
    ]
    .into_iter()
    .map(|(id, params, want)| (id, params.to_owned(), want))
    .collect()
}
