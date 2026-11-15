//! The egui front end: the only crate that draws (`docs/architecture.md`).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod ai;
pub mod app;
pub mod appearance_menu;
pub mod backdrop;
pub mod bookmarks;
pub mod bubble;
pub mod crossfade;
pub mod dial_deck;
pub mod dict;
pub mod documents;
pub mod files;
pub mod find_bar;
pub mod floating_label;
pub mod focus;
pub mod fold;
pub mod footnote;
pub mod gpu;
pub mod icons;
pub mod image_viewer;
pub mod import;
pub mod keymap;
pub mod layout;
pub mod library;
pub mod library_panel;
pub mod lift;
pub mod loading_mark;
pub mod machine_fonts;
pub mod modal;
pub mod motion;
pub mod notes_panel;
pub mod outline_panel;
pub mod page_ai;
pub mod page_bar;
pub mod pages;
pub mod panes;
pub mod park;
pub mod pdf_view;
pub mod platform;
pub mod reach;
pub mod read_aloud;
pub mod repaint;
pub mod resize;
pub mod scripts;
pub mod search;
pub mod selection;
pub mod settings_window;
pub mod shaped;
pub mod shortcuts;
pub mod sidebar;
pub mod sounds;
pub mod study;
pub mod theme;
pub mod thumbnails;
pub mod titlebar;
pub mod toast;
pub mod ui_command;
pub mod view_menu;
pub mod vocab;
pub mod widgets;
pub mod word_list;
pub mod zoom;

pub use app::MareaderApp;

#[cfg(test)]
mod testing {
    /// The sample handle the engine makes; `Sample` is its one owner.
    pub fn doc() -> Result<mareader_session::DocId, &'static str> {
        mareader_engine::sample::doc()
    }

    /// A sample of `pages` read at `current` in pane 1.
    pub fn reading(pages: usize, current: usize) -> Result<crate::documents::Reading, String> {
        let bytes = std::sync::Arc::new(mareader_pdf::sample::pdf(pages, "Sample"));
        let typo = mareader_session::Typography::default();
        let document = mareader_session::Document::open(bytes, &typo).map_err(|e| e.to_string())?;
        Ok(crate::documents::Reading {
            pane: mareader_workspace::PaneId(1),
            doc: doc()?,
            document: std::sync::Arc::new(document),
            current,
            mode: mareader_engine::pane::ViewMode::Vertical,
            progress: 0.0,
            name: "sample.pdf".into(),
            page: None,
        })
    }
}
