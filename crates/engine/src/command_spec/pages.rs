//! The forms a script sends for a page: what a pane shows, what opens in it.

use std::path::PathBuf;

use serde_json::Value;

use mareader_session::SpinePlace;

use crate::command::Command;
use crate::command_spec::{json, names};
use crate::documents::OpenRequest;
use crate::session::Session;

/// `page.arrow`: which way an arrow key goes.
pub(crate) fn arrow(_at: &Session, params: &Value) -> Result<Command, String> {
    let arrow = json::one_of(params, "arrow", names::ARROW)?;
    Ok(Command::Arrow(arrow))
}

/// `page.step`: a page key, forward or back.
pub(crate) fn step(_at: &Session, params: &Value) -> Result<Command, String> {
    let forward = json::flag(params, "forward")?;
    Ok(Command::Page { forward })
}

/// `page.autoscroll`: the active pane's scroll, turned over.
pub(crate) fn autoscroll(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::AutoScroll)
}

/// `page.first`: the document's first page.
pub(crate) fn first(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::FirstPage)
}

/// `page.last`: the document's last page.
pub(crate) fn last(_at: &Session, _params: &Value) -> Result<Command, String> {
    Ok(Command::LastPage)
}

/// `page.go`: a pane and the page number it shows.
pub(crate) fn go_to(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let page = json::count(params, "page")?;
    Ok(Command::GoTo { pane, page })
}

/// `page.turn`: a pane, and which way its page turns.
pub(crate) fn turn(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let forward = json::flag(params, "forward")?;
    Ok(Command::Turn { pane, forward })
}

/// `page.copy`: the pages whose text goes to the clipboard.
pub(crate) fn copy(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let pages = json::counts_or_none(params, "pages")?;
    Ok(Command::CopyPages { pane, pages })
}

/// `page.search`: searches the pane's document for `query`.
pub(crate) fn search(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let query = json::text(params, "query")?;
    Ok(Command::Search { pane, query })
}

/// `word.list`: the pages a word list is copied from.
pub(crate) fn word_list(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let pages = json::counts_or_none(params, "pages")?;
    Ok(Command::WordList { pane, pages })
}

/// `word.list-window`: the pages the word list window opens on.
pub(crate) fn word_list_window(at: &Session, params: &Value) -> Result<Command, String> {
    let pane = json::pane(at, params)?;
    let pages = json::counts_or_none(params, "pages")?;
    Ok(Command::WordListWindow { pane, pages })
}

/// `reader.open`: a file by path, or bytes beside the name they are called.
pub(crate) fn open(_at: &Session, params: &Value) -> Result<Command, String> {
    let mut ask = if let Some(path) = json::maybe_text(params, "path")? {
        OpenRequest::path(PathBuf::from(path))
    } else {
        let Some(bytes) = json::member(params, "bytes") else {
            let what = "a file, or \"bytes\" beside the \"name\" they carry";
            return Err(json::wants("path", what));
        };
        let name = json::text(params, "name")?;
        OpenRequest::bytes(octets(bytes)?, name)
    };
    ask.pane = json::maybe_pane(params, "pane")?;
    if let Some(placement) = json::maybe_one_of(params, "placement", names::PLACEMENT)? {
        ask.placement = placement;
    }
    if let Some(page) = json::maybe_count(params, "page")? {
        ask.page = page;
    }
    if let Some(pages) = json::maybe_count(params, "pages")? {
        ask.pages = pages;
    }
    if let Some(place) = json::maybe_object(params, "place") {
        ask.place = Some(json::anchor(place)?);
    }
    if let Some(spine) = json::maybe_object(params, "spine") {
        ask.spine = Some(spine_place(spine)?);
    }
    if json::member(params, "book").is_some() {
        ask.book = Some(json::book(params, "book")?);
    }
    Ok(Command::Open(ask))
}

/// `reader.open-book`: a library book, by its number.
pub(crate) fn open_book(_at: &Session, params: &Value) -> Result<Command, String> {
    let book = json::book(params, "book")?;
    Ok(Command::OpenBook(book))
}

/// `reader.place-book`: a book, the pane it goes to, and how it lands.
pub(crate) fn place_book(at: &Session, params: &Value) -> Result<Command, String> {
    let book = json::book(params, "book")?;
    let pane = json::maybe_pane(params, "pane")?.or(Some(at.active()));
    let placement = json::maybe_one_of(params, "placement", names::PLACEMENT)?;
    let placement = placement.unwrap_or_default();
    Ok(Command::PlaceBook {
        book,
        pane,
        placement,
    })
}

/// `library.forget`: the books and paths whose saved place goes.
pub(crate) fn forget(_at: &Session, params: &Value) -> Result<Command, String> {
    let books = json::books_or_none(params, "books")?;
    let paths = json::paths_or_none(params, "paths")?;
    Ok(Command::ForgetPositions { books, paths })
}

/// The bytes a script lists as numbers, since a JSON text cannot hold them.
fn octets(found: &Value) -> Result<Vec<u8>, String> {
    let list = found
        .as_array()
        .ok_or_else(|| json::wants("bytes", "a list of numbers, one to a byte"))?;
    list.iter()
        .map(|one| {
            one.as_u64()
                .and_then(|whole| u8::try_from(whole).ok())
                .ok_or_else(|| json::wants("bytes", "numbers of a byte each"))
        })
        .collect()
}

/// A place in an EPUB's spine, which is what a strip of pages is keyed by.
fn spine_place(found: &Value) -> Result<SpinePlace, String> {
    let href = json::text(found, "href")?;
    let item = json::count(found, "item")?;
    let block = json::count(found, "block")?;
    let offset = json::count(found, "offset")?;
    Ok(SpinePlace {
        href,
        item,
        block,
        offset,
    })
}
