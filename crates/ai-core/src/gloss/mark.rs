//! The persisted gloss mark and the format-specific anchor behind it.

use serde::{Deserialize, Serialize};

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
/// The field names are the serde schema persisted under
/// `mareader.gloss.v2` — do not rename.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlossMark {
    pub id: String,
    pub word: String,
    pub context: String,
    /// Persisted identity. Flattened so the schema keeps its top-level
    /// `page` and `rect` fields rather than nesting them under an `anchor`.
    #[serde(flatten)]
    pub anchor: PageAnchor,
}

impl GlossMark {
    /// Whether two marks denote the same glossed spot: the same word, at the
    /// same anchor spot. The single identity definition shared by
    /// capture-time dedup and re-click toggle-to-close.
    pub fn same_spot(&self, other: &Self) -> bool {
        self.word == other.word && self.anchor.same_spot(&other.anchor)
    }
}

impl std::ops::Deref for GlossMark {
    type Target = PageAnchor;

    fn deref(&self) -> &Self::Target {
        &self.anchor
    }
}

/// The id of a mark captured on `page` at `stamp_ms`.
///
/// The scheme lives here rather than at the capture sites because an id is
/// load-bearing twice over: the key a mark is persisted under
/// (`mareader.gloss.v2`) and the key a re-click on its stroke toggles by.
/// Three call sites used to format it identically — three chances for one to
/// drift and a mark to become unreachable by the code that saved it.
///
/// The page is in the id for a human reading storage; the stamp keeps two
/// marks on one page apart. The clock is NOT read here — this crate's gloss
/// half is pure and host-tested, so the caller supplies the stamp (the app's
/// `components::ai::anchor::captured_mark` is the one place that takes it).
pub fn mark_id(page: u32, stamp_ms: u64) -> String {
    format!("g{page}-{stamp_ms}")
}

/// Where a gloss mark sits in the document: a page number plus a rect in
/// *page* space (unscaled page coordinates). Unlike a screen rect it survives
/// scroll, zoom and view-mode flips — the live screen box is re-derived from
/// the page host element whenever anything moves (the app's
/// `components::ai::anchor`). Shared by the selection Explain pill and the
/// gloss card so both glue to the page without inventing their own coordinate
/// systems.
///
/// The reflowable formats have no anchor of their own on the wire: a spot
/// there is a block index and a character range riding in
/// [`GlossMark::context`] as a tagged envelope ([`ReflowSpot`]), because the
/// pages under it are re-cut whenever the typography moves. They still carry
/// a `PageAnchor` — the page the block happened to land on — so the schema
/// has exactly one shape.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PageAnchor {
    pub page: u32,
    pub rect: GlossBox,
}

impl PageAnchor {
    /// Whether two anchors denote the same logical spot, tolerant of
    /// sub-pixel drift: a re-capture of the same spot is the same spot even
    /// when the layout shifted a fraction of a pixel.
    pub fn same_spot(&self, other: &Self) -> bool {
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

/// The reflowable formats' *durable* identity for a spot: a block index plus
/// a character range inside that block.
///
/// A page number and a rect are the right answer for a PDF's fixed pixels and
/// the wrong one for a document that re-lays itself out: a font-size change,
/// a resize or a re-measure settling all re-cut the pages, and a page-space
/// rect then points at whatever moved under it. What survives every re-flow
/// is the block the words live in and how far into it they start.
///
/// Offsets are in the RENDERED text of the block (for Markdown the source
/// syntax is not part of them), counted in CHARACTERS — Unicode code points,
/// not the UTF-16 units a DOM `Range` speaks. One character is one character
/// on both sides of the wire; the conversion to code units happens once, at
/// the projection's `set_start`/`set_end` boundary. Pixels are never stored:
/// the format's own projection re-derives them from the live DOM at watch
/// time (the app's `components::ai::reflow_anchor`), which is also what lets
/// one mark follow its words onto another page.
///
/// Payload only: it travels inside [`GlossMark::context`] as a tagged
/// envelope rather than as a second anchor type on the wire, so the
/// persisted schema stays the one [`PageAnchor`] shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ReflowSpot {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

impl ReflowSpot {
    /// The spot covering `text` at the start of `block` — the shape a capture
    /// builds before it knows any better, and the one the tests use.
    pub fn new(block: usize, start: usize, end: usize) -> Self {
        Self { block, start, end: end.max(start) }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the spot covers no characters at all (a collapsed or clamped
    /// range), which makes it unprojectable and worth dropping.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_id_is_its_page_and_its_stamp() {
        // Pinned because it is a storage key: marks already saved under this
        // scheme are addressed by it, so a change here is a migration, not a
        // formatting preference.
        assert_eq!(mark_id(12, 1_700_000_000_123), "g12-1700000000123");
        assert_eq!(mark_id(1, 0), "g1-0");
    }

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

    #[test]
    fn a_reflow_spot_is_its_characters_and_clamps_into_a_shorter_block() {
        let spot = ReflowSpot::new(7, 12, 24);
        assert_eq!(spot.len(), 12);
        assert!(!spot.is_empty());
        // A block that shrank under the mark is clamped where the mark is
        // PROJECTED (`formats::reflow::spot::clamp_span`), against the text
        // that is actually on the page rather than a char count guessed here.
        // An end before the start is a collapsed range, not a backwards one.
        assert!(ReflowSpot::new(1, 9, 3).is_empty());
    }

    #[test]
    fn a_reflow_spot_survives_json_and_ignores_an_absent_field() {
        // The envelope the app writes into `GlossMark.context`: it must come
        // back byte-identical, and a payload without it must still parse so a
        // PDF-era mark is simply a mark with no spot.
        let spot = ReflowSpot::new(11, 2, 8);
        let json = serde_json::to_string(&spot).expect("serialize");
        assert_eq!(serde_json::from_str::<ReflowSpot>(&json).expect("round trip"), spot);

        #[derive(Deserialize)]
        struct Holder {
            #[serde(default)]
            spot: Option<ReflowSpot>,
        }
        let empty: Holder = serde_json::from_str("{}").expect("absent field");
        assert!(empty.spot.is_none());
    }
}
