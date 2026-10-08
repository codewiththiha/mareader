//! Red ink over hard words; a click hands each one to the AI card.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use app_state::dom_contract::TEXT_LAYER_CLASS;

use self::layer::CefrBox;

pub mod layer;
pub mod pdf;
pub mod reflow;

/// One text scan per row, shared with the search painter.
pub(crate) mod scan;

/// Range measurement, batched into single animation frames.
pub(crate) mod measure;

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

/// Publish a measured walk; a stale task writes neither target.
pub(crate) fn publish(sink: &Sink, painted: Vec<CefrBox>) {
    if sink.boxes.try_get_untracked().is_none() {
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

/// One text layer's joined text, plus each span's element and its
/// character range.
pub(crate) struct LayerText {
    pub text: String,
    pub spans: Vec<(web_sys::Element, usize, usize)>,
}

/// Read a text layer once: the range walk must not index two parallel
/// arrays.
pub(crate) fn read_text_layer(host: &web_sys::Element) -> Option<LayerText> {
    let layer = host
        .query_selector(&format!(".{TEXT_LAYER_CLASS}"))
        .ok()
        .flatten()?;
    let list = layer.query_selector_all("span").ok()?;
    let mut text = String::new();
    let mut spans: Vec<(web_sys::Element, usize, usize)> =
        Vec::with_capacity(list.length() as usize);
    let mut chars = 0usize;
    for index in 0..list.length() {
        let Some(node) = list.get(index) else {
            continue;
        };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else {
            continue;
        };
        let piece = el.text_content().unwrap_or_default();
        let start = chars;
        chars += piece.chars().count();
        text.push_str(&piece);
        spans.push((el, start, chars));
    }
    if spans.is_empty() {
        return None;
    }
    Some(LayerText { text, spans })
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
}
