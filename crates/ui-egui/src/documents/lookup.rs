//! Resolve explicit word-card anchors in the visible document.

use egui::Rect;
use mareader_appearance::Rgb;
use mareader_workspace::PaneId;

use super::DocumentsView;

impl DocumentsView<'_> {
    /// A retypeset book receives a new DocId but keeps its source identity.
    pub fn reflow_source(&self, pane: PaneId) -> Option<(Option<std::path::PathBuf>, String)> {
        self.reflowable(pane).then(|| {
            (
                self.path(pane).map(std::path::Path::to_owned),
                self.title(pane).unwrap_or_default().to_owned(),
            )
        })
    }

    /// Resolves a lookup against visible sheets. None means layout/text is
    /// not ready; Some(None) means its text is outside the visible sheets.
    pub fn lookup_anchor(
        &self,
        pane: PaneId,
        anchor: &mut crate::word_card::TextAnchor,
    ) -> Option<Option<Rect>> {
        let doc = self.state.place(pane)?.doc;
        if anchor.doc.is_some_and(|id| id != doc) {
            if anchor.reflow_source.is_some() && anchor.reflow_source == self.reflow_source(pane) {
                anchor.doc = Some(doc);
                return None;
            }
            return Some(None);
        }
        let sheets = self.sheets(pane);
        if sheets.is_empty() {
            return None;
        }
        let missing = sheets
            .iter()
            .any(|s| self.state.texts.get(doc, s.page).is_none());
        let found = if let Some(spot) = anchor.spot {
            let mark = crate::study::marks::Shown {
                id: mareader_library::MarkId(0),
                spot,
                ink: crate::study::marks::Ink {
                    pen: mareader_library::Pen::Full,
                    color: Rgb(0, 0, 0),
                    ai: false,
                },
                noted: false,
            };
            let pages = crate::selection::Pages {
                doc,
                sheets,
                texts: &self.state.texts,
                sideways: false,
                lift: false,
            };
            crate::study::marks::place(&[mark], &pages)
                .iter()
                .map(|p| p.bounds())
                .reduce(|a, b| a.union(b))
        } else {
            anchor
                .spans
                .iter()
                .filter_map(|(page, range)| {
                    let sheet = sheets.iter().find(|s| s.page == *page)?;
                    let rect = self
                        .hit_box(pane, *page, range.clone())?
                        .intersect(sheet.shown);
                    rect.is_positive().then_some(rect)
                })
                .reduce(|a, b| a.union(b))
        };
        if found.is_none() && missing {
            None
        } else {
            Some(found)
        }
    }
}
