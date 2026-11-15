//! Where a book stands while it is read, and the panes that show it.
//!
//! A reader below the front end names no window type, so the pane a book is
//! open in arrives as a value, and turning or lighting a pane is asked of
//! whatever the front end offers.

use std::ops::Range;
use std::sync::Arc;

use mareader_session::{DocId, Document};
use mareader_settings::ReadAloudSettings;
use mareader_workspace::PaneId;

/// The pane a book is open in, as of the frame just drawn.
#[derive(Clone, Debug)]
pub struct Stand {
    pub pane: PaneId,
    pub doc: DocId,
    pub document: Arc<Document>,
    /// The page showing in that pane.
    pub current: usize,
}

/// The panes a reader turns and lights, as the front end offers them.
pub trait Panes {
    /// Turns `pane` to `page`.
    fn go_to(&mut self, pane: PaneId, page: usize);

    /// Lights `chars` on `pane`'s `page`; true while that pane still moves.
    fn reveal_said(&mut self, pane: PaneId, page: usize, chars: Range<usize>, clear: f32) -> bool;
}

/// Turns `here`'s pane to `page` when it is new and following is on.
pub fn follow(
    panes: &mut dyn Panes,
    here: &Stand,
    settings: &ReadAloudSettings,
    page: Option<usize>,
    followed: &mut Option<usize>,
) {
    if settings.follow && page.is_some() && page != *followed {
        *followed = page;
        if let Some(page) = page.filter(|p| *p != here.current) {
            panes.go_to(here.pane, page);
        }
    }
}

/// "Page 12 · Chapter Three": where a page is, for the seek track.
pub fn place_name(document: &Document, page: usize) -> String {
    let outline = document.outline();
    let chapter = mareader_pdf::active_entry(outline, page).and_then(|i| outline.get(i));
    match chapter {
        Some(c) if !c.title.trim().is_empty() => format!("Page {} · {}", page + 1, c.title.trim()),
        _ => format!("Page {}", page + 1),
    }
}
