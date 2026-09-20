//! The persistent library's value-only endpoint. Files are opened by readers.
use leptos::prelude::*;
use serde_json::json;
use library_core::book::{self, Book, Fingerprint, Origin, ReadPoint, Row};
use crate::state::{AppState, Toast};
use super::{emit, ReaderConfig};

pub fn request_open(state: AppState, book_id: Option<String>, path: String) {
    if !reader_core::format::is_supported_path(&path) {
        state.ui.toast.set(Some(Toast::new("Unsupported document format")));
        return;
    }
    let format = reader_core::format::format_of(&path);
    let mut books = state.library.books.get_untracked();
    let id = book_id.or_else(|| book::book_rows(&books)
        .find(|b| b.path() == path && !b.independent).map(|b| b.id.clone()))
        .unwrap_or_else(|| {
            let now = crate::time::now_ms();
            let book = Book::new(library_core::id::next_id(now), Fingerprint::placeholder(&path),
                format, Origin::Linked { src: path.clone() }, now);
            book::add_book(&mut books, book)
        });
    let (resume_page, resume_fraction) = book::resume_point(&books, Some(&id), &path);
    let title = book::find_by_id(&books, &id).map(Book::title);
    state.library.books.set(books);
    crate::storage::persist_library(state.library);
    let cover = state.library.covers.with_untracked(|covers| covers.get(&path).map(|c| c.data_url.clone()));
    let format = match format {
        reader_core::format::Format::Pdf => "pdf",
        reader_core::format::Format::Text => "txt",
        reader_core::format::Format::Markdown => "md",
    }.to_string();
    emit(json!({"type":"open-request", "config":ReaderConfig {
        book_id:id, path, format, title, cover, resume_page, resume_fraction,
        settings:state.settings.get_untracked(),
    }}));
}

pub fn install(state: AppState) {
    super::listen(move |message| {
        match message["type"].as_str() {
            Some("dispose") => {
                crate::storage::persist_library(state.library);
                crate::runtime::unmount();
                emit(json!({"type":"disposed"}));
            }
            Some("reload-library") => {
                let blob = crate::storage::load_library();
                state.library.books.set(blob.books);
                state.library.shelves.set(blob.shelves);
                state.library.folders.set(blob.folders);
                state.library.view.set(blob.view);
            }
            Some("open-book") => {
                if let Some(id) = message["bookId"].as_str() {
                    crate::services::document::open::open_book(state, id.to_string());
                }
            }
            Some("open-path") => {
                if let Some(path) = message["path"].as_str() { request_open(state, None, path.to_string()); }
            }
            Some("progress" | "metadata") => receive_read(state, &message),
            Some("cover") => {
                if let (Some(path), Some(url), Some(width), Some(height)) = (
                    message["path"].as_str(), message["dataUrl"].as_str(),
                    message["width"].as_f64(), message["height"].as_f64(),
                ) {
                    if super::is_workspace() { state.library.covers.set(crate::storage::load_covers()); }
                    crate::services::library::covers::file_cover(state, path.to_string(), url.to_string(), width, height);
                    crate::services::library::covers::prune_now(state);
                    crate::storage::persist_covers(state.library);
                    if super::is_workspace() { state.library.covers.set(Default::default()); }
                }
            }
            Some("settings") => {
                if let Ok(mut settings) = serde_json::from_value(message["settings"].clone()) {
                    reader_core::settings::sanitize(&mut settings);
                    if let Err(error) = crate::storage::save_settings(&settings) { error.report(); }
                    state.settings.set(settings);
                }
            }
            Some("error") => {
                if let Some(message) = message["message"].as_str() {
                    state.ui.toast.set(Some(Toast::new(message)));
                }
            }
            _ => {}
        }
    });
    emit(json!({"type":"library-ready"}));
}

fn receive_read(state: AppState, message: &serde_json::Value) {
    let (Some(id), Some(path), Ok(point)) = (
        message["bookId"].as_str(), message["path"].as_str(),
        serde_json::from_value::<super::ReadPoint>(message.clone()),
    ) else { return; };
    let Some(book) = state.library.books.with_untracked(|rows| book::find_by_id(rows, id).cloned()) else { return; };
    if book.path() != path { return; }
    let point = ReadPoint { page:point.page, num_pages:point.num_pages,
        fraction:point.fraction.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0)) }.settled();
    state.library.books.update(|rows| {
        if message["type"] == "metadata" {
            book::record_read_row(rows, id, path,
                message["title"].as_str().map(str::to_string),
                message["author"].as_str().map(str::to_string), point, crate::time::now_ms());
        } else {
            for i in book::rows_for_read(rows, Some(id), path) {
                if let Some(book) = rows.get_mut(i).and_then(Row::as_book_mut) {
                    book.page = point.page;
                    book.fraction = point.fraction;
                }
            }
        }
    });
    crate::storage::persist_library(state.library);
    if message["type"] == "metadata" {
        crate::services::library::verify_one(state, path.to_string());
    }
}
