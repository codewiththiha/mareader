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
pub mod command;
pub mod crossfade;
pub mod dial_deck;
pub mod dict;
pub mod documents;
pub mod export;
pub mod files;
pub mod find_bar;
pub mod floating_label;
pub mod focus;
pub mod fold;
pub mod footnote;
pub mod gpu;
pub mod icons;
pub mod image_viewer;
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
pub mod page_copy;
pub mod pages;
pub mod pane_texts;
pub mod panes;
pub mod paper;
pub mod park;
pub mod pdf_view;
pub mod platform;
pub mod play;
pub mod reach;
pub mod read_aloud;
pub mod reading_place;
pub mod resize;
pub mod reveal;
pub mod route;
pub mod scripts;
pub mod search;
pub mod selection;
pub mod settings_window;
pub mod shaped;
pub mod shortcuts;
pub mod sidebar;
pub mod sounds;
pub mod stable_hash;
pub mod study;
pub mod texts;
pub mod theme;
pub mod thumbnails;
pub mod titlebar;
pub mod toast;
pub mod transcript;
pub mod trigger;
pub mod view_menu;
pub mod vocab;
pub mod watch;
pub mod widgets;
mod word_card;
pub mod word_list;
pub mod zoom;

pub use app::MareaderApp;
pub use documents::OpenRequest;

#[cfg(test)]
mod testing {
    use mareader_session::{DocId, Session};

    /// A real `DocId` comes only from opening; a tiny sample will do.
    pub fn doc() -> Result<DocId, &'static str> {
        let mut s = Session::new(0, || {});
        let bytes = std::sync::Arc::new(mareader_pdf::sample::pdf(1, "t"));
        s.open(
            mareader_session::Source::Bytes(bytes),
            None,
            Default::default(),
        );
        let budget = mareader_session::Budget {
            items: 1,
            bytes: usize::MAX,
        };
        s.poll(budget)
            .into_iter()
            .find_map(|e| match e {
                mareader_session::Event::Opened { result: Ok(id), .. } => Some(id),
                _ => None,
            })
            .ok_or("the sample didn't open")
    }

    /// A sample of `pages` read at `current` in pane 1.
    pub fn reading(pages: usize, current: usize) -> Result<crate::documents::Reading, String> {
        let bytes = std::sync::Arc::new(mareader_pdf::sample::pdf(pages, "Sample"));
        let typo = mareader_session::Typography::default();
        let document = mareader_session::Document::open(bytes, &typo);
        let document = document.map_err(|e| e.to_string())?;
        Ok(crate::documents::Reading {
            pane: mareader_workspace::PaneId(1),
            doc: doc()?,
            document: std::sync::Arc::new(document),
            current,
            mode: crate::layout::ViewMode::Vertical,
            progress: 0.0,
            name: "sample.pdf".into(),
            page: None,
        })
    }
}
