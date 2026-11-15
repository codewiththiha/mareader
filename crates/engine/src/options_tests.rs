//! The keys the engine owns, and the ones it leaves to a front end.

use std::sync::Arc;

use super::*;
use crate::host::Fake;
use crate::library::{Action, BookAction, SearchAction, State};
use crate::session::Session;
use mareader_appearance::{BaseMode, NoiseMode};
use mareader_library::{CoverFit, Home, Layout, SortKey};
use mareader_session::Source;
use mareader_settings::{
    AppearanceMenuStyle, Bar, FloatingLabelStyle, LibraryClick, Provider, Settings,
};
use mareader_workspace::{Bounds, Edge, SplitAxis};

/// A session on a host with no machine, so its settings are the defaults.
fn bare() -> Session {
    Session::new(Arc::new(Fake::default()), Arc::new(|| {}))
}

/// The command a key asks for, or why the question has no answer.
fn parsed(key: &str, value: &str) -> Result<Vec<Command>, String> {
    parsed_in(&bare(), key, value)
}

/// The same, against a session holding what the key reads.
fn parsed_in(at: &Session, key: &str, value: &str) -> Result<Vec<Command>, String> {
    match parse(key, value, at) {
        None => Err(format!("{key} is a front end's own")),
        Some(found) => found,
    }
}

/// A session with the key carried out, so a setting can be looked at.
fn written(key: &str, value: &str) -> Result<Session, String> {
    carried(bare(), key, value)
}

/// The same, from a session that already holds what the key reads.
fn carried(mut session: Session, key: &str, value: &str) -> Result<Session, String> {
    for command in parsed_in(&session, key, value)? {
        let done = session.apply(command);
        if let Some(why) = done.refused {
            return Err(why);
        }
    }
    Ok(session)
}

#[test]
fn a_number_reaches_the_setting_it_names() -> Result<(), String> {
    let session = written("text_size", "21")?;
    assert_eq!(session.settings().text.font_size, 21.0);
    let session = written("pane_gap", "12.5")?;
    assert_eq!(session.settings().workspace.look.gap, 12.5);
    Ok(())
}

#[test]
fn a_flag_reaches_the_setting_it_names() -> Result<(), String> {
    let session = written("glass", "true")?;
    assert!(session.settings().chrome.glass);
    // `gap` is the space between pages, so the setting is its opposite.
    let session = written("gap", "false")?;
    assert!(session.settings().layout.no_gap);
    Ok(())
}

#[test]
fn a_choice_is_read_ignoring_case() -> Result<(), String> {
    let session = written("grain", "ANIMATED")?;
    assert_eq!(session.settings().appearance.noise, NoiseMode::Animated);
    let session = written("appearance_menu", "dial-deck")?;
    let style = session.settings().chrome.appearance_menu;
    assert_eq!(style, AppearanceMenuStyle::DialDeck);
    Ok(())
}

#[test]
fn a_theme_and_a_preset_both_land() -> Result<(), String> {
    let session = written("theme", "dim")?;
    assert_eq!(session.settings().appearance.base, BaseMode::Dim);
    let session = written("preset", "sepia")?;
    let look = session.settings().appearance;
    assert_ne!(look, Settings::default().appearance);
    Ok(())
}

#[test]
fn any_setting_by_its_path_is_one_key() -> Result<(), String> {
    let session = written("set", "layout.page_indicator:false")?;
    assert!(!session.settings().layout.page_indicator);
    let missing = parsed("set", "layout.page_indicator");
    assert_eq!(missing.err().as_deref(), Some("set: path:json"));
    Ok(())
}

#[test]
fn a_bad_value_says_what_the_key_takes() -> Result<(), String> {
    let hint = |key: &str, value: &str| parsed(key, value).err();
    assert_eq!(hint("text_size", "big").as_deref(), Some("text_size: px"));
    let grain = "grain: off, static or animated";
    assert_eq!(hint("grain", "loud").as_deref(), Some(grain));
    let theme = "theme: light, dark, dim or black";
    assert_eq!(hint("theme", "mauve").as_deref(), Some(theme));
    let preset = "preset: sepia, green, night, parchment, cinema";
    assert_eq!(hint("preset", "noir").as_deref(), Some(preset));
    Ok(())
}

#[test]
fn a_key_the_front_end_keeps_is_not_here() {
    // These name the window's own state.
    let kept = [
        "chromeless",
        "menu",
        "reduced",
        // These name the front end's own widgets.
        "platform",
        "sidebar",
        "find",
        "workers",
        // This one is shared: its popover values stay a front end's.
        "open",
        "toast",
        "toasts",
        "page_ai",
        "page_ai_hold",
        "dictionaries",
        "explain",
        "selection",
    ];
    for key in kept {
        assert!(parse(key, "true", &bare()).is_none(), "{key} moved");
    }
    assert!(parse("no_such_key", "1", &bare()).is_none());
}

#[test]
fn the_shelf_keys_answer_library_actions() -> Result<(), String> {
    let found = parsed("books", "3")?;
    assert_eq!(found, vec![Command::Library(Action::Seed(3))]);
    let found = parsed("shelves", "demo")?;
    assert_eq!(found, vec![Command::Library(Action::SeedShelves)]);
    let found = parsed("copies", "2")?;
    assert_eq!(found, vec![Command::Library(Action::SeedCopies(2))]);
    let found = parsed("shelf", "Fiction")?;
    let named = Action::OpenShelf("Fiction".to_owned());
    assert_eq!(found, vec![Command::Library(named)]);
    let found = parsed("runs", "two")?;
    assert_eq!(found, vec![Command::Library(Action::RunsDemo)]);
    let found = parsed("organize", "review")?;
    assert_eq!(found, vec![Command::Library(Action::OrganizeDemo(true))]);
    let hint = parsed("organize", "sideways");
    let want = "organize: running, review, paused or ready";
    assert_eq!(hint.err().as_deref(), Some(want));
    let hint = parsed("books", "many");
    assert_eq!(hint.err().as_deref(), Some("books: a count"));
    Ok(())
}

#[test]
fn the_picking_keys_ask_the_shelf_where_they_land() -> Result<(), String> {
    // Nothing is read here: a shot seeds the shelf in the same batch.
    let found = parsed("select", "2")?;
    assert_eq!(found, vec![Command::Library(Action::PickRecent(2))]);
    let found = parsed("remove", "2")?;
    assert_eq!(found, vec![Command::Library(Action::AskRemoveRecent(2))]);
    let found = parsed("remove", "shelf:Fiction")?;
    let named = Action::AskRemoveShelf("Fiction".to_owned());
    assert_eq!(found, vec![Command::Library(named)]);
    let hint = parsed("remove", "some");
    let want = "remove: a count or shelf:Name";
    assert_eq!(hint.err().as_deref(), Some(want));
    Ok(())
}

#[test]
fn a_shelf_key_counts_what_the_batch_seeded() -> Result<(), String> {
    let mut shelf = State::default();
    let mut out = Vec::new();
    shelf.act(Action::Seed(4), 0.0, &mut out);
    for command in parsed("select", "3")? {
        let action = match command {
            Command::Library(action) => action,
            other => return Err(format!("{other:?} is no library ask")),
        };
        shelf.act(action, 0.0, &mut out);
    }
    assert_eq!(shelf.picks.books().len(), 3);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_word_levels_key_asks_for_its_folder() -> Result<(), String> {
    let mut found = parsed("cefr", "/tmp/words")?.into_iter();
    let root = std::path::PathBuf::from("/tmp/words");
    assert_eq!(found.next(), Some(Command::AttachVocab(root)));
    let dial = match found.next() {
        Some(Command::SetField { path, value }) => (path, value),
        other => return Err(format!("{other:?} is no dial")),
    };
    assert_eq!(dial, ("study.cefr.enabled".to_owned(), "true".to_owned()));
    assert_eq!(found.next(), None);
    Ok(())
}

#[test]
fn the_open_key_is_shared_by_its_values() -> Result<(), String> {
    let found = parsed("open", "search")?;
    let focus = Action::Search(SearchAction::Focus);
    assert_eq!(found, vec![Command::Library(focus)]);
    let found = parsed("open", "add")?;
    assert_eq!(found, vec![Command::Library(Action::ShowAdd)]);
    let found = parsed("open", "add_folder")?;
    let folder = Action::AskFolder(std::path::PathBuf::from("/home/reader/Books"));
    assert_eq!(found, vec![Command::Library(folder)]);
    let found = parsed("open", "rename")?;
    assert_eq!(found, vec![Command::Library(Action::AskRenameLast)]);
    let found = parsed("open", "remove")?;
    let ask = Action::Book(BookAction::AskRemovePicked);
    assert_eq!(found, vec![Command::Library(ask)]);
    // A popover is a front end's, so the door falls through to it.
    assert!(parse("open", "appearance", &bare()).is_none());
    Ok(())
}

#[test]
fn a_path_the_settings_do_not_have_is_refused() {
    let mut session = bare();
    let done = session.apply(Command::SetField {
        path: "no.such.dial".into(),
        value: "1".into(),
    });
    assert!(done.refused.is_some(), "an unknown path is refused");
}

#[test]
fn a_folder_key_reaches_the_library() -> Result<(), String> {
    for key in ["import", "folder"] {
        let commands = parsed(key, "/home/reader/Books")?;
        let Some(Command::Library(ask)) = commands.first() else {
            return Err(format!("{key} did not ask the library"));
        };
        // Both ask, and neither opens a sheet of its own accord.
        assert!(matches!(
            ask,
            Action::AskFolder(_) | Action::ImportFolder { .. }
        ));
    }
    // A folder read in place is watched, and counts every file it finds.
    let commands = parsed("folder", "/home/reader/Books")?;
    let Some(Command::Library(Action::ImportFolder { opts, .. })) = commands.first() else {
        return Err("folder did not ask for an import".into());
    };
    assert!(opts.watch, "the folder is watched");
    assert_eq!(opts.min_size, 0, "every file counts");
    Ok(())
}

#[test]
fn a_sort_key_writes_the_key_and_its_way() -> Result<(), String> {
    let session = written("sort", "title:desc")?;
    assert_eq!(session.settings().library.sort, SortKey::Title);
    assert!(!session.settings().library.ascending);
    // Without a way, the key's own natural one.
    let session = written("sort", "last_read")?;
    assert_eq!(session.settings().library.sort, SortKey::LastRead);
    assert!(!session.settings().library.ascending);
    let hint = "sort: arranged, last_read, title, author or added";
    assert_eq!(parsed("sort", "colour").err().as_deref(), Some(hint));
    Ok(())
}

#[test]
fn the_shelf_s_view_keys_land() -> Result<(), String> {
    let session = written("layout", "list")?;
    assert_eq!(session.settings().library.layout, Layout::List);
    let session = written("cover", "crop")?;
    assert_eq!(session.settings().library.cover, CoverFit::Crop);
    let session = written("home", "unshelved")?;
    assert_eq!(session.settings().library.home, Home::Unshelved);
    let session = written("columns", "3")?;
    assert_eq!(session.settings().library.columns, Some(3));
    Ok(())
}

#[test]
fn a_sample_is_built_without_a_file_on_disk() -> Result<(), String> {
    let commands = parsed("pdf", "sample:4")?;
    let Some(Command::Open(request)) = commands.first() else {
        return Err("pdf=sample did not ask for an open".into());
    };
    assert_eq!(request.name.as_deref(), Some("sample.pdf"));
    // No pane named: which one is in front is a front end's to know.
    assert!(request.pane.is_none(), "the front end aims it");
    let commands = parsed("epub", "sample")?;
    let Some(Command::Open(request)) = commands.first() else {
        return Err("epub=sample did not ask for an open".into());
    };
    assert_eq!(request.name.as_deref(), Some("sample.epub"));
    Ok(())
}

#[test]
fn a_path_is_opened_as_the_file_it_names() -> Result<(), String> {
    let commands = parsed("pdf", "/home/reader/Books/one.pdf")?;
    let Some(Command::Open(request)) = commands.first() else {
        return Err("pdf did not ask for an open".into());
    };
    assert!(
        matches!(request.source, Source::Path(_)),
        "the path is kept"
    );
    Ok(())
}

#[test]
fn the_workspace_keys_land() -> Result<(), String> {
    let session = written("library_click", "split")?;
    let click = session.settings().workspace.library_click;
    assert_eq!(click, LibraryClick::Split);
    let session = written("independent_textures", "true")?;
    assert!(session.settings().workspace.independent_textures);
    let session = written("shared_base", "true")?;
    assert!(session.settings().workspace.shared_base_mode);
    let hint = "library_click: replace, split or drag_only";
    assert_eq!(
        parsed("library_click", "double").err().as_deref(),
        Some(hint)
    );
    Ok(())
}

#[test]
fn a_search_reaches_the_library_field() -> Result<(), String> {
    let commands = parsed("search", "dune")?;
    let Some(Command::Library(Action::Search(SearchAction::Text(text)))) = commands.first() else {
        return Err("search did not ask the library".into());
    };
    assert_eq!(text, "dune");
    Ok(())
}

#[test]
fn a_pin_key_writes_every_bar_at_once() -> Result<(), String> {
    let session = written("pinned", "true")?;
    for bar in Bar::ALL {
        assert!(session.settings().chrome.pinned(bar), "{bar:?} is free");
    }
    let session = written("pinned", "false")?;
    for bar in Bar::ALL {
        let pinned = session.settings().chrome.pinned(bar);
        assert!(!pinned, "{bar:?} stays pinned");
    }
    // One write per bar, so a bar added to `Bar::ALL` cannot be missed.
    assert_eq!(parsed("pinned", "true")?.len(), Bar::ALL.len());
    Ok(())
}

#[test]
fn a_label_key_writes_the_three_fields_it_names() -> Result<(), String> {
    let session = written("label", "chapter")?;
    let layout = &session.settings().layout;
    assert!(layout.floating_label);
    assert!(!layout.floating_label_persist);
    assert_eq!(layout.floating_label_style, FloatingLabelStyle::Chapter);
    let session = written("label", "persist")?;
    assert!(session.settings().layout.floating_label_persist);
    let session = written("label", "false")?;
    assert!(!session.settings().layout.floating_label);
    Ok(())
}

#[test]
fn a_preset_key_keeps_the_look_the_session_holds() -> Result<(), String> {
    let session = written("theme", "dark")?;
    let kept = carried(session, "save_preset", "Look/Page")?;
    let made = &kept.settings().user_presets;
    assert_eq!(made.len(), 1);
    assert_eq!(made.first().map(|p| p.name.as_str()), Some("Look"));
    assert_eq!(made.first().map(|p| p.group.as_str()), Some("Page"));
    let base = made.first().map(|p| p.appearance.base);
    assert_eq!(base, Some(BaseMode::Dark));
    Ok(())
}

#[test]
fn a_provider_key_points_one_task_at_one_preset() -> Result<(), String> {
    let session = written("provider", "openai")?;
    let ai = &session.settings().ai;
    assert_eq!(ai.presets.len(), 1);
    let made = ai.presets.first().map(|p| p.provider);
    assert_eq!(made, Some(Provider::OpenAi));
    let pointed = ai.tasks.explain.preset.as_ref();
    assert_eq!(pointed, ai.presets.first().map(|p| &p.id));
    let hint = "provider: e.g. openai or ollama";
    assert_eq!(parsed("provider", "nope").err().as_deref(), Some(hint));
    Ok(())
}

#[test]
fn a_focus_key_counts_panes_as_a_shot_does() -> Result<(), String> {
    let mut session = bare();
    let first = session.active();
    let area = Bounds::new(0.0, 0.0, 1600.0, 1000.0);
    let Some(second) = session.split_at(first, Edge::Right, area) else {
        return Err("the fixture area has room for two".into());
    };
    let aimed = match parsed_in(&session, "focus", "2")?.first() {
        Some(Command::FocusPane(pane)) => Some(*pane),
        _ => None,
    };
    assert_eq!(aimed, Some(second));
    let hint = "focus: no such pane";
    let found = parsed_in(&session, "focus", "3").err();
    assert_eq!(found.as_deref(), Some(hint));
    let count = "focus: a pane number";
    let found = parsed_in(&session, "focus", "x").err();
    assert_eq!(found.as_deref(), Some(count));
    Ok(())
}

#[test]
fn a_panes_key_answers_one_split_per_pane_it_adds() -> Result<(), String> {
    let made = parsed("panes", "3")?;
    let mut axes = Vec::new();
    for command in &made {
        if let Command::Split(axis) = command {
            axes.push(*axis);
        }
    }
    assert_eq!(axes, vec![SplitAxis::Horizontal, SplitAxis::Vertical]);
    assert!(parsed("panes", "1")?.is_empty());
    let hint = "panes: 1 to 4";
    let found = parsed("panes", "x").err();
    assert_eq!(found.as_deref(), Some(hint));
    Ok(())
}

#[test]
fn a_meaning_language_is_checked_before_it_is_written() -> Result<(), String> {
    let session = written("meaning", "my")?;
    let language = session.settings().ai.tasks.explain.language.as_deref();
    assert_eq!(language, Some("my"));
    let hint = "meaning: a tag, e.g. my";
    assert_eq!(parsed("meaning", "zz").err().as_deref(), Some(hint));
    Ok(())
}
