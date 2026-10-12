//! A walk's memo: what it depends on, and where its boxes land.

use leptos::prelude::*;

use super::layer::CefrBox;

/// What a walk's result depends on; equality means skip the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WalkKey {
    /// Layout and content epoch (reflow); zero on a PDF page.
    pub fp: u64,
    /// The committed zoom (reflow only; PDF boxes are page-space).
    pub zoom: u64,
    /// The assembled text's fingerprint.
    pub text: u64,
    /// The cache fill clock.
    pub generation: u64,
    /// The suppression set's fingerprint.
    pub marks: u64,
    /// The band threshold the walk ran against.
    pub threshold: u8,
}

/// The last walk a component ran, held in its own scope and never
/// outliving it.
#[derive(Clone)]
pub(crate) struct WalkMemo {
    pub key: WalkKey,
    /// The scheduled frame has measured and filled `boxes`.
    pub measured: bool,
    pub boxes: Vec<CefrBox>,
}

/// Where a scheduled measurement writes back. All handles, all `Copy`.
#[derive(Clone, Copy)]
pub(crate) struct Sink {
    pub key: WalkKey,
    pub boxes: RwSignal<Vec<CefrBox>>,
    pub memo: StoredValue<Option<WalkMemo>, LocalStorage>,
}

impl Sink {
    /// True when the layer currently holds painted boxes from an earlier walk.
    pub(crate) fn has_previous(&self) -> bool {
        self.boxes
            .try_with_untracked(|b| !b.is_empty())
            .unwrap_or(false)
    }
}

/// Publish a walk, keeping stale boxes if a live row measured empty.
pub(crate) fn publish(sink: &Sink, painted: Vec<CefrBox>) {
    publish_checked(sink, painted, true)
}

/// Stale-while-revalidate publish with explicit block liveness.
pub(crate) fn publish_checked(sink: &Sink, painted: Vec<CefrBox>, block_alive: bool) {
    if sink.boxes.try_get_untracked().is_none() {
        return;
    }
    if !block_alive {
        if sink.has_previous() {
            sink.boxes.set(Vec::new());
        }
        return;
    }
    if painted.is_empty() && sink.has_previous() {
        // Keep stale boxes when a live row measured zero rects.
        return;
    }
    if sink.boxes.get_untracked() != painted {
        sink.boxes.set(painted.clone());
    }
    sink.memo.try_update_value(|slot| {
        if let Some(slot) = slot
            && slot.key == sink.key
            && !slot.measured
        {
            slot.measured = true;
            slot.boxes = painted;
        }
    });
}

/// Fold mark ids into one suppression fingerprint; any edit moves it.
pub(crate) fn marks_fingerprint<'a>(ids: impl Iterator<Item = &'a str>) -> u64 {
    let mut fold: u64 = 0xcbf2_9ce4_8422_2325;
    for id in ids {
        for byte in id.bytes() {
            fold ^= u64::from(byte);
            fold = fold.wrapping_mul(0x0000_0100_0000_01b3);
        }
        fold ^= 0xff;
        fold = fold.wrapping_mul(0x0000_0100_0000_01b3);
    }
    fold
}

/// The text's fingerprint: the same FNV over the assembled characters.
pub(crate) fn text_fingerprint(text: &str) -> u64 {
    let mut fold: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        fold ^= u64::from(byte);
        fold = fold.wrapping_mul(0x0000_0100_0000_01b3);
    }
    fold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mark_fold_moves_on_any_id_change() {
        let a = marks_fingerprint(["m1", "m2"].into_iter());
        let b = marks_fingerprint(["m1", "m3"].into_iter());
        let c = marks_fingerprint(["m2", "m1"].into_iter());
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, marks_fingerprint(["m1", "m2"].into_iter()));
    }

    #[test]
    fn the_text_fold_is_stable_and_order_sensitive() {
        assert_eq!(text_fingerprint("abc"), text_fingerprint("abc"));
        assert_ne!(text_fingerprint("abc"), text_fingerprint("acb"));
        assert_ne!(text_fingerprint(""), text_fingerprint("a"));
    }

    #[test]
    fn cefr_keeps_stale_boxes_while_row_blank() {
        use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};

        let owner = Owner::new();
        owner.with(|| {
            let key = WalkKey {
                fp: 1,
                zoom: 1,
                text: text_fingerprint("ubiquitous"),
                generation: 1,
                marks: 0,
                threshold: 4,
            };
            let sample = CefrBox {
                x: 10.0,
                y: 20.0,
                w: 64.0,
                h: 16.0,
                mark: GlossMark {
                    id: "m1".to_string(),
                    word: "ubiquitous".to_string(),
                    context: "ubiquitous".to_string(),
                    anchor: PageAnchor {
                        page: 1,
                        rect: GlossBox::default(),
                    },
                },
            };
            let boxes = RwSignal::new(vec![sample.clone()]);
            let memo = StoredValue::new_local(Some(WalkMemo {
                key,
                measured: false,
                boxes: Vec::new(),
            }));
            let sink = Sink { key, boxes, memo };
            assert!(sink.has_previous());

            // Live blank row: keep stale boxes and leave memo unmeasured.
            publish_checked(&sink, Vec::new(), true);
            assert_eq!(boxes.get_untracked(), vec![sample]);
            assert!(
                !memo.get_value().unwrap().measured,
                "unmeasured memo schedules a rescan on settle"
            );

            // Once genuinely unmounted (`!block_alive`), boxes clear.
            publish_checked(&sink, Vec::new(), false);
            assert!(boxes.get_untracked().is_empty());
        });
    }
}
