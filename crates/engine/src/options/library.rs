//! The shelf's keys: what a folder does, and how the books are laid out.
//!
//! What a front end keeps of these moves its page or brings up a popover:
//! `library` goes to the shelf, and `open` answers for its panels here and for
//! the window's own popovers there. The rest read the books as they stand, or
//! fill a fixture, and each answers as an ask the library carries out when it
//! lands, so a shot that seeds a shelf and then counts it counts what it
//! seeded.

use std::path::PathBuf;

use mareader_library::{CoverFit, Home, Layout, Options, SortKey};

use super::{field, number};
use crate::command::Command;
use crate::library::{Action, BookAction, SearchAction};
use crate::session::Session;

/// The folder a fixture's import sheet asks about.
const FOLDER: &str = "/home/reader/Books";

/// The shelf's keys; `None` for a key that is not one of them.
pub(super) fn set(key: &str, value: &str, _at: &Session) -> Option<Result<Vec<Command>, String>> {
    let parsed = match key {
        "import" => ask_folder(value),
        "folder" => read_folder(value),
        "search" => Ok(vec![Command::Library(Action::Search(SearchAction::Text(
            value.to_owned(),
        )))]),
        "layout" => layout(value),
        "columns" => field("library.columns", value.parse::<usize>().ok()),
        "cover" => cover(value),
        "sort" => sort(value),
        "home" => home(value),
        "select" => select(value),
        "remove" => remove(value),
        "books" => books(value),
        "shelves" => Ok(vec![Command::Library(Action::SeedShelves)]),
        "copies" => copies(value),
        "organize" => organize(value),
        "shelf" => Ok(vec![Command::Library(Action::OpenShelf(value.to_owned()))]),
        "runs" => Ok(vec![Command::Library(Action::RunsDemo)]),
        "open" => return open(value),
        _ => return None,
    };
    Some(parsed)
}

/// The first books by last read, picked as the page's bar picks them.
fn select(value: &str) -> Result<Vec<Command>, String> {
    let count = number(value, "select: a count of books")?;
    Ok(vec![Command::Library(Action::PickRecent(count))])
}

/// Asks to remove the first books by last read, or a shelf by name.
fn remove(value: &str) -> Result<Vec<Command>, String> {
    let hint = "remove: a count or shelf:Name";
    let ask = match value.strip_prefix("shelf:") {
        Some(name) => Action::AskRemoveShelf(name.to_owned()),
        None => Action::AskRemoveRecent(number(value, hint)?),
    };
    Ok(vec![Command::Library(ask)])
}

/// A fixture's shelf: the count of books a shot asks for.
fn books(value: &str) -> Result<Vec<Command>, String> {
    let count = number(value, "books: a count")?;
    Ok(vec![Command::Library(Action::Seed(count))])
}

/// A fixture's twins: the count of copies of one book.
fn copies(value: &str) -> Result<Vec<Command>, String> {
    let count = number(value, "copies: a count")?;
    Ok(vec![Command::Library(Action::SeedCopies(count))])
}

/// A made-up AI run over the loose copies, or the plan it came to.
fn organize(value: &str) -> Result<Vec<Command>, String> {
    let action = match value {
        "running" => Action::OrganizeDemo(false),
        "review" => Action::OrganizeDemo(true),
        "paused" => Action::OrganizePaused(false),
        "ready" => Action::OrganizePaused(true),
        _ => return Err("organize: running, review, paused or ready".to_owned()),
    };
    Ok(vec![Command::Library(action)])
}

/// The library's own panels; `None` for a popover, which a front end brings up.
fn open(value: &str) -> Option<Result<Vec<Command>, String>> {
    let found = match value {
        "search" => Action::Search(SearchAction::Focus),
        "add" => Action::ShowAdd,
        "add_folder" => Action::AskFolder(PathBuf::from(FOLDER)),
        // The removal sheet for whatever the page has picked.
        "remove" => Action::Book(BookAction::AskRemovePicked),
        "rename" => Action::AskRenameLast,
        "find_again" => Action::FindAgainLost,
        _ => return None,
    };
    Some(Ok(vec![Command::Library(found)]))
}

/// The import sheet for a folder, as Add ▸ Folder… asks.
fn ask_folder(value: &str) -> Result<Vec<Command>, String> {
    let ask = Action::AskFolder(PathBuf::from(value));
    Ok(vec![Command::Library(ask)])
}

/// A folder read in place: watched, every file counted, so its books show.
fn read_folder(value: &str) -> Result<Vec<Command>, String> {
    let opts = Options {
        watch: true,
        min_size: 0,
        ..Options::default()
    };
    let import = Action::ImportFolder {
        root: PathBuf::from(value),
        opts,
    };
    Ok(vec![Command::Library(import)])
}

fn layout(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "list" => Layout::List,
        _ => Layout::Grid,
    };
    field("library.layout", some)
}

fn cover(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "crop" => CoverFit::Crop,
        _ => CoverFit::Fit,
    };
    field("library.cover", some)
}

fn home(value: &str) -> Result<Vec<Command>, String> {
    let some = match value {
        "unshelved" => Home::Unshelved,
        _ => Home::Everything,
    };
    field("library.home", some)
}

/// The key and the way it runs, which the settings hold apart.
fn sort(value: &str) -> Result<Vec<Command>, String> {
    let hint = "sort: arranged, last_read, title, author or added";
    let (key, way) = value.split_once(':').unwrap_or((value, ""));
    let flat = key.replace('_', "");
    let key = SortKey::ALL
        .into_iter()
        .find(|k| format!("{k:?}").eq_ignore_ascii_case(&flat))
        .ok_or(hint)?;
    let some = match way {
        "asc" => true,
        "desc" => false,
        _ => key.natural_ascending(),
    };
    let mut commands = field("library.sort", key)?;
    commands.extend(field("library.ascending", some)?);
    Ok(commands)
}
