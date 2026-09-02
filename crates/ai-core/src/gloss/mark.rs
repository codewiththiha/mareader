//! The persisted gloss mark and the format-specific anchor behind it.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

use super::geometry::GlossBox;

/// A persisted gloss highlight: the word that was explained, plus WHERE it is
/// in the document in a form that survives everything the viewport does to it.
///
/// The rect is deliberately in **document space** — for the PDF format,
/// unscaled CSS px measured from the `.pdf-page` host's origin — not in
/// viewport space. A native `Selection` cannot be persisted (it is cleared
/// when the card opens, it dies with the text-layer spans the virtualizer
/// unmounts, and there is only ever one of it), so the mark is re-projected
/// onto the screen as `host_rect.origin + rect * display_scale` every time
/// the page mounts. That is what makes the highlight survive scroll, zoom,
/// remounts and sessions.
///
/// The field names are the serde schema persisted to localStorage
/// (`pdfreader.gloss.v1`) — do not rename.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlossMark<A = PageAnchor> {
    pub id: String,
    pub word: String,
    pub context: String,
    /// Format-specific persisted identity. Flattening keeps the PDF schema's
    /// existing top-level `page` and `rect` fields compatible.
    #[serde(flatten)]
    pub anchor: A,
}

impl<A: MarkAnchor> GlossMark<A> {
    /// Whether two marks denote the same glossed spot: the same word, and
    /// rects at the same anchor spot. The single identity definition shared
    /// by capture-time dedup and re-click toggle-to-close.
    pub fn same_spot(&self, other: &Self) -> bool {
        self.word == other.word && self.anchor.same_spot(&other.anchor)
    }
}

impl<A> std::ops::Deref for GlossMark<A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.anchor
    }
}

/// Where a gloss mark sits in the document — format-specific.
///
/// The trait owns the *identity* of a spot: given two anchors, is this the
/// same place in the document? Projection onto the screen is deliberately
/// NOT part of the trait — it needs live layout (the page host's current
/// position and scale, the display mode), which only the format's renderer
/// layer has, and each format projects its own anchors (for PDF, the app's
/// `components::ai::anchor`).
///
/// [`PageAnchor`] is the PDF implementation. A future format (epub, plain
/// text, ...) implements this with its own notion of a spot — a line and
/// character offset, a block id — and the rest of the AI feature (the card,
/// the cache, the settings) reuses it unchanged.
pub trait MarkAnchor: Clone + Debug + PartialEq + Serialize + DeserializeOwned {
    /// Whether two anchors denote the same logical spot in the document.
    ///
    /// Tolerant of sub-pixel drift: a re-capture of the same spot is the same
    /// spot even when the layout shifted a fraction of a pixel.
    fn same_spot(&self, other: &Self) -> bool;
}

/// The PDF implementation of [`MarkAnchor`]: a page number plus a rect in
/// *page* space (unscaled page coordinates).
///
/// Unlike a screen rect it survives scroll, zoom and view-mode flips: the
/// live screen box is re-derived from the page host element whenever anything
/// moves. Shared by the selection Info pill and the gloss card so both glue
/// to the page without each inventing its own coordinate system.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PageAnchor {
    pub page: u32,
    pub rect: GlossBox,
}

impl MarkAnchor for PageAnchor {
    fn same_spot(&self, other: &Self) -> bool {
        self.page == other.page
            && (self.rect.x - other.rect.x).abs() < 1.0
            && (self.rect.y - other.rect.y).abs() < 1.0
    }
}

impl PageAnchor {
    pub fn from_mark(m: &GlossMark) -> Self {
        m.anchor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_spot_tolerates_sub_pixel_drift_but_not_a_new_word() {
        let base = GlossMark {
            id: "g1".into(),
            word: "palimpsest".into(),
            context: String::new(),
            anchor: PageAnchor {
                page: 1,
                rect: GlossBox { x: 100.0, y: 40.0, w: 60.0, h: 12.0, r: 0.0 },
            },
        };

        let mut drifted = base.clone();
        drifted.id = "g2".into();
        drifted.anchor.rect.x += 0.4;
        assert!(base.same_spot(&drifted), "sub-pixel drift is the same spot");

        let mut other_word = base.clone();
        other_word.word = "palimpsests".into();
        assert!(!base.same_spot(&other_word));

        let mut other_page = base.clone();
        other_page.anchor.page = 2;
        assert!(!base.same_spot(&other_page));

        let mut moved = base.clone();
        moved.anchor.rect.y += 2.0;
        assert!(!base.same_spot(&moved));
    }

    #[test]
    fn a_page_anchor_is_the_same_spot_across_sub_pixel_drift() {
        let a = PageAnchor {
            page: 2,
            rect: GlossBox { x: 10.0, y: 20.0, w: 5.0, h: 5.0, r: 0.0 },
        };
        let mut b = a;
        b.rect.x += 0.9;
        b.rect.y += 0.9;
        assert!(a.same_spot(&b));
        let mut c = a;
        c.rect.x += 1.0;
        assert!(!a.same_spot(&c));
        let mut d = a;
        d.page = 3;
        assert!(!a.same_spot(&d));
    }

    #[test]
    fn a_gloss_mark_round_trips_through_json() {
        // The persistence contract: what localStorage holds must come back
        // byte-identical, because the rect IS the anchor. A silently dropped
        // field would put the highlight on the wrong word after a restart.
        let mark = GlossMark {
            id: "g3-1700000000000".to_string(),
            word: "palimpsest".to_string(),
            context: "a manuscript page, a palimpsest, scraped clean".to_string(),
            anchor: PageAnchor {
                page: 3,
                rect: GlossBox { x: 120.5, y: 44.25, w: 62.0, h: 13.5, r: 0.0 },
            },
        };
        let json = serde_json::to_string(&mark).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json value");
        assert_eq!(value["page"], 3);
        assert_eq!(value["rect"]["x"], 120.5);
        assert!(
            value.get("anchor").is_none(),
            "PDF anchor must remain flattened"
        );
        let back: GlossMark = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(mark, back);
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TextAnchor {
        block: String,
        offset: usize,
    }

    impl MarkAnchor for TextAnchor {
        fn same_spot(&self, other: &Self) -> bool {
            self.block == other.block && self.offset == other.offset
        }
    }

    #[test]
    fn a_non_pdf_anchor_defines_mark_identity_and_persists() {
        let mark = GlossMark {
            id: "text-1".to_string(),
            word: "palimpsest".to_string(),
            context: String::new(),
            anchor: TextAnchor {
                block: "chapter-2".to_string(),
                offset: 14,
            },
        };
        let mut same = mark.clone();
        same.id = "text-2".to_string();
        assert!(mark.same_spot(&same));
        same.anchor.offset += 1;
        assert!(!mark.same_spot(&same));

        let json = serde_json::to_string(&mark).expect("serialize");
        let back: GlossMark<TextAnchor> = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(mark, back);
    }
}
