//! Reattach saved selections to a book and its current layout.

use std::path::PathBuf;
use std::sync::Arc;

use mareader_library::{AiSelection, BookId, Library, Spot};
use mareader_session::{Anchor as BookAnchor, Caret, Document, Selection};
use mareader_settings::Skill;
use mareader_workspace::PaneId;

use super::{Anchor, Answer, Ask, AskId, State};
use crate::study::words;

pub(crate) struct View {
    pub pane: PaneId,
    pub doc: u64,
    pub book: Option<BookId>,
    pub path: Option<PathBuf>,
    pub document: Arc<Document>,
}

impl State {
    /// Attach each selection once to a reading view and its current layout.
    pub(crate) fn refresh(&mut self, views: Vec<View>, lib: &Library) -> bool {
        let mut changed = false;
        for view in &views {
            if let Some(book) = view.book {
                for saved in lib.ai_selections(book) {
                    if !self.asks.iter().any(|a| a.pinned == Some(saved.id))
                        && let Some(ask) = restored(saved, view, AskId(self.next))
                    {
                        self.next = self.next.saturating_add(1);
                        self.asks.push(ask);
                        changed = true;
                    }
                }
            }
        }
        for ask in &mut self.asks {
            if !matches!(ask.anchor, Anchor::Text(_)) {
                continue;
            }
            let current = views
                .iter()
                .find(|v| v.pane == ask.pane && v.doc == ask.doc);
            let same = |v: &&View| {
                (ask.book.is_some() && ask.book == v.book)
                    || (ask.path.is_some() && ask.path == v.path)
            };
            let Some(view) = current.or_else(|| views.iter().find(same)) else {
                continue;
            };
            let same_layout = ask
                .layout
                .as_ref()
                .and_then(|w| w.upgrade())
                .is_some_and(|d| Arc::ptr_eq(&d, &view.document));
            if same_layout && ask.pane == view.pane && ask.doc == view.doc {
                continue;
            }
            if let Some(spot) = ask.spot {
                let spans = resolve(&view.document, spot, &ask.text).unwrap_or_default();
                ask.anchor = Anchor::Text(spans);
            }
            ask.pane = view.pane;
            ask.doc = view.doc;
            ask.book = ask.book.or(view.book);
            ask.layout = Some(Arc::downgrade(&view.document));
            changed = true;
        }
        changed
    }
}

fn restored(saved: &AiSelection, view: &View, id: AskId) -> Option<Ask> {
    Some(Ask {
        id,
        pane: view.pane,
        doc: view.doc,
        skill: Skill::parse(&saved.skill)?,
        name: saved.name.clone(),
        anchor: Anchor::Text(Vec::new()),
        book: Some(saved.book.value),
        spot: Some(saved.spot),
        open: false,
        pinned: Some(saved.id),
        pin_status: crate::library::SaveStatus::Saved,
        auto_pin: false,
        answer: if saved.pending {
            Answer::Interrupted
        } else {
            Answer::Ready(saved.result.clone())
        },
        by: saved.by.clone(),
        text: saved.source.clone(),
        path: view.path.clone(),
        layout: None,
        job: None,
    })
}

/// Resolve only bounded spans, and reject text changed under the saved spot.
fn resolve(
    doc: &Document,
    spot: Spot,
    source: &str,
) -> Option<Vec<(usize, std::ops::Range<usize>)>> {
    let (first, last) = match spot {
        Spot::Chars { from, to } => (from[0], to[0]),
        Spot::Blocks { from, to } => (
            doc.locate(BookAnchor {
                block: from[0],
                offset: from[1],
            })?,
            doc.locate(BookAnchor {
                block: to[0],
                offset: to[1],
            })?,
        ),
        _ => return None,
    };
    if last < first || last.saturating_sub(first) > 255 || last >= doc.page_count() {
        return None;
    }
    let mut spans = Vec::new();
    let mut found = String::new();
    for page in first..=last {
        let layer = doc.text_layer(page).ok()?;
        let range = match spot {
            Spot::Chars { from, to } => {
                let caret = |[page, index]: [usize; 2]| Caret { page, index };
                Selection {
                    anchor: caret(from),
                    focus: caret(to),
                }
                .on_page(page, layer.text.len())
            }
            Spot::Blocks { from, to } => words::between(&layer, (from[0], from[1]), (to[0], to[1])),
            _ => None,
        };
        if let Some(range) = range {
            found.push_str(&layer.text.text(range.clone()));
            spans.push((page, range));
        }
    }
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    (!spans.is_empty() && compact(&found) == compact(source)).then_some(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_reflow::Typography;

    fn document(bytes: Vec<u8>, size: f32) -> Arc<Document> {
        let typo = Typography { font_size: size, ..Typography::default() };
        Arc::new(Document::open(Arc::new(bytes), &typo).unwrap())
    }

    fn selected(doc: &Document) -> (Spot, String) {
        for page in 0..doc.page_count() {
            let layer = Arc::new(doc.text_layer(page).unwrap());
            if layer.text.len() < 30 { continue }
            let start = if layer.places.is_empty() {
                Some(0)
            } else {
                (0..layer.text.len().saturating_sub(25))
                    .find(|&i| (i..i + 25).all(|n| layer.place(n).is_some()))
            };
            let Some(start) = start else { continue };
            let selection = Selection::within(page, start..start + 25);
            let Some(spot) = words::span(selection, |_| Some(Arc::clone(&layer))) else { continue };
            let source = selection.text(|_| Some(Arc::clone(&layer)));
            if !source.trim().is_empty() { return (spot, source) }
        }
        panic!("sample has no selectable text")
    }

    #[test]
    fn pdf_selection_resolves_and_changed_text_is_rejected() {
        let doc = document(mareader_pdf::sample::pdf(2, "Sample"), 16.0);
        let (spot, text) = selected(&doc);
        assert!(matches!(spot, Spot::Chars { .. }));
        assert!(resolve(&doc, spot, &text).is_some());
        assert!(resolve(&doc, spot, "different text").is_none());
    }

    #[test]
    fn epub_selection_resolves_after_the_text_reflows() {
        let bytes = mareader_epub::sample::epub(3, "Book");
        let doc = document(bytes.clone(), 16.0);
        let (spot, text) = selected(&doc);
        assert!(matches!(spot, Spot::Blocks { .. }));
        let resized = document(bytes, 24.0);
        assert!(resolve(&resized, spot, &text).is_some());
    }
}
