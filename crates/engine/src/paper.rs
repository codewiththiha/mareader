//! The blend backdrop: the open documents' own paper, page by page.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use mareader_appearance::Rgb;
use mareader_pdf::{Palette, Paper};
use mareader_session::{DocId, RenderError};
use mareader_settings::PaperArea;

/// Pages read ahead of those shown, so a scroll finds them known.
const AHEAD: usize = 2;

/// Each open document's paper, read both ways.
#[derive(Debug, Default)]
pub struct Papers {
    docs: BTreeMap<DocId, Pages>,
}

#[derive(Debug, Default)]
struct Pages {
    whole: Palette,
    edges: Palette,
    /// Pages read, with or without a paper, so none is asked twice.
    read: BTreeSet<usize>,
}

impl Papers {
    pub fn insert(&mut self, doc: DocId, page: usize, result: Result<Paper, RenderError>) {
        let pages = self.docs.entry(doc).or_default();
        pages.read.insert(page);
        let Ok(paper) = result else { return };
        if let Some(c) = paper.whole {
            pages.whole.set(page, c);
        }
        if let Some(c) = paper.edges {
            pages.edges.set(page, c);
        }
    }

    pub fn forget(&mut self, doc: DocId) {
        self.docs.remove(&doc);
    }

    /// `doc`'s paper at fractional page `at`, as `area` reads it.
    pub fn at(&self, doc: DocId, area: PaperArea, at: f64) -> Option<Rgb> {
        let pages = self.docs.get(&doc)?;
        match area {
            PaperArea::WholePage => pages.whole.at(at),
            PaperArea::Edges => pages.edges.at(at),
        }
    }

    /// Unread pages among those `shown`, one before and a few after.
    pub fn wanted(&self, doc: DocId, shown: Range<usize>, count: usize) -> Vec<usize> {
        let read = self.docs.get(&doc).map(|p| &p.read);
        let near = shown.start.saturating_sub(1)..(shown.end + AHEAD).min(count);
        near.filter(|page| read.is_none_or(|r| !r.contains(page)))
            .collect()
    }

    /// Documents with any paper read, for meters and tests.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREAM: Rgb = Rgb(250, 244, 232);
    const SLATE: Rgb = Rgb(120, 135, 153);

    type Out = Result<(), &'static str>;

    fn paper(whole: Rgb, edges: Rgb) -> Result<Paper, RenderError> {
        Ok(Paper {
            whole: Some(whole),
            edges: Some(edges),
        })
    }

    #[test]
    fn each_area_reads_its_own_colour() -> Out {
        let (mut p, d) = (Papers::default(), crate::sample::doc()?);
        p.insert(d, 0, paper(SLATE, CREAM));
        assert_eq!(p.at(d, PaperArea::WholePage, 0.0), Some(SLATE));
        assert_eq!(p.at(d, PaperArea::Edges, 0.0), Some(CREAM));
        Ok(())
    }

    #[test]
    fn pages_read_are_never_wanted_again() -> Out {
        let (mut p, d) = (Papers::default(), crate::sample::doc()?);
        assert_eq!(p.wanted(d, 3..5, 10), [2, 3, 4, 5, 6]);
        p.insert(d, 3, paper(CREAM, CREAM));
        // A failed page counts as read: it won't draw next time either.
        p.insert(d, 5, Err(RenderError::Failed));
        assert_eq!(p.wanted(d, 3..5, 10), [2, 4, 6]);
        assert_eq!(p.wanted(d, 0..1, 2), [0, 1]);
        assert_eq!(p.at(d, PaperArea::Edges, 5.0), Some(CREAM));
        Ok(())
    }

    #[test]
    fn a_closed_document_forgets_its_paper() -> Out {
        let (mut p, d) = (Papers::default(), crate::sample::doc()?);
        p.insert(d, 1, paper(CREAM, CREAM));
        assert_eq!(p.len(), 1);
        p.forget(d);
        assert!(p.is_empty());
        assert_eq!(p.at(d, PaperArea::WholePage, 1.0), None);
        Ok(())
    }
}
