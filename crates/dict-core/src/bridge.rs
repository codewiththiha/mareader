//! The route a translation takes: direct, reversed, or through
//! a hub language between the shores.

use serde::{Deserialize, Serialize};

/// One hop in a plan: a pack, and which of its columns is the door.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hop {
    pub pack: String,
    /// `false`: ask `word` for `definition`. `true`: ask the other way.
    pub reverse: bool,
}

/// One way to answer `from -> to`, best plans first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub from: String,
    pub to: String,
    pub hops: Vec<Hop>,
    /// The middle language a bridge plan rides through.
    pub via: Option<String>,
}

/// A pack the planner may use: its two languages and its id.
#[derive(Debug, Clone, Copy)]
pub struct PackRef {
    pub id: &'static str,
    pub source: &'static str,
    pub target: &'static str,
}

/// Every plan that carries `from -> to`, best first: direct,
/// reversed, then bridged.
pub fn plans(from: &str, to: &str, packs: &[PackRef]) -> Vec<Plan> {
    let mut out: Vec<Plan> = Vec::new();
    for pack in packs {
        if pack.source == from && pack.target == to {
            out.push(Plan {
                from: from.into(),
                to: to.into(),
                hops: vec![Hop {
                    pack: pack.id.to_string(),
                    reverse: false,
                }],
                via: None,
            });
        }
    }
    for pack in packs {
        if pack.source == to && pack.target == from {
            out.push(Plan {
                from: from.into(),
                to: to.into(),
                hops: vec![Hop {
                    pack: pack.id.to_string(),
                    reverse: true,
                }],
                via: None,
            });
        }
    }
    // A bridge is two shores and a hub between them.
    for hub in ["en"] {
        if hub == from || hub == to {
            continue;
        }
        let first = plans(from, hub, packs);
        let second = plans(hub, to, packs);
        for a in &first {
            for b in &second {
                let mut hops = a.hops.clone();
                hops.extend(b.hops.iter().copied());
                out.push(Plan {
                    from: from.into(),
                    to: to.into(),
                    hops,
                    via: Some(hub.into()),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKS: &[PackRef] = &[
        PackRef {
            id: "mcfnlp-en-my",
            source: "en",
            target: "my",
        },
        PackRef {
            id: "jmdict-en-jp",
            source: "en",
            target: "jp",
        },
        PackRef {
            id: "wiktionary-en-fr",
            source: "en",
            target: "fr",
        },
    ];

    #[test]
    fn a_direct_pack_is_the_first_plan() {
        let out = plans("en", "jp", PACKS);
        assert_eq!(out[0].hops.len(), 1);
        assert!(!out[0].hops[0].reverse);
        assert_eq!(out[0].hops[0].pack, "jmdict-en-jp");
        assert_eq!(out[0].via, None);
    }

    #[test]
    fn a_reversed_pack_carries_the_way_home() {
        let out = plans("my", "en", PACKS);
        assert_eq!(out[0].hops[0].pack, "mcfnlp-en-my");
        assert!(out[0].hops[0].reverse);
    }

    #[test]
    fn no_pack_between_the_shores_rides_through_english() {
        let out = plans("my", "jp", PACKS);
        let bridge = out
            .iter()
            .find(|plan| plan.via.as_deref() == Some("en"))
            .expect("a my-jp bridge through en");
        assert_eq!(bridge.hops.len(), 2);
        assert!(
            bridge.hops[0].reverse,
            "my is asked through its pack's door"
        );
        assert_eq!(bridge.hops[1].pack, "jmdict-en-jp");
    }

    #[test]
    fn a_bridge_needs_both_shores() {
        // No pack carries my -> de: no plan at all.
        assert!(plans("my", "de", PACKS).is_empty());
    }
}
