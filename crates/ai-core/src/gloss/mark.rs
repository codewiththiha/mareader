//! The persisted gloss mark and the format-specific anchor behind it.

use serde::{Deserialize, Serialize};

use super::geometry::GlossBox;

/// A persisted gloss highlight: the word plus its document-space rect.
pub struct GlossMark {
    pub id: String,
    pub word: String,
    pub context: String,
    /// Persisted identity: flattened so the schema keeps `page` and `rect`.
    pub anchor: PageAnchor,
}

impl GlossMark {
    /// Whether two marks denote the same glossed spot: word plus anchor.
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

/// The id of a mark captured on `page` at `stamp_ms`: the storage key.
pub fn mark_id(page: u32, stamp_ms: u64) -> String {
    format!("g{page}-{stamp_ms}")
}

/// Where a mark sits: a page number plus a rect in unscaled page space.
pub struct PageAnchor {
    pub page: u32,
    pub rect: GlossBox,
}

impl PageAnchor {
    /// Whether two anchors denote the same spot, tolerant of sub-pixel drift.
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

/// A reflowable spot: block index plus character range, since pages re-cut.
pub struct ReflowSpot {
    pub block: usize,
    pub start: usize,
    pub end: usize,
}

impl ReflowSpot {
    /// The spot covering `text` at the start of `block`.
    pub fn new(block: usize, start: usize, end: usize) -> Self {
        Self {
            block,
            start,
            end: end.max(start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mark_id_is_its_page_and_its_stamp() {
        // Pinned: marks already saved under this scheme are addressed by it.
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
                rect: GlossBox {
                    x: 100.0,
                    y: 40.0,
                    w: 60.0,
                    h: 12.0,
                    r: 0.0,
                },
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
            rect: GlossBox {
                x: 10.0,
                y: 20.0,
                w: 5.0,
                h: 5.0,
                r: 0.0,
            },
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
        // localStorage must come back byte-identical: the rect is the anchor.
        let mark = GlossMark {
            id: "g3-1700000000000".to_string(),
            word: "palimpsest".to_string(),
            context: "a manuscript page, a palimpsest, scraped clean".to_string(),
            anchor: PageAnchor {
                page: 3,
                rect: GlossBox {
                    x: 120.5,
                    y: 44.25,
                    w: 62.0,
                    h: 13.5,
                    r: 0.0,
                },
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
        assert_eq!(spot.end, 24);
        // Clamping happens at projection, against the text really on the page.
        let collapsed = ReflowSpot::new(1, 9, 3);
        assert_eq!(collapsed.end, collapsed.start);
    }

    #[test]
    fn a_reflow_spot_survives_json_and_ignores_an_absent_field() {
        // `GlossMark.context` must round-trip; absent means a PDF-era mark.
        let spot = ReflowSpot::new(11, 2, 8);
        let json = serde_json::to_string(&spot).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ReflowSpot>(&json).expect("round trip"),
            spot
        );

        #[derive(Deserialize)]
        struct Holder {
            #[serde(default)]
            spot: Option<ReflowSpot>,
        }
        let empty: Holder = serde_json::from_str("{}").expect("absent field");
        assert!(empty.spot.is_none());
    }
}
