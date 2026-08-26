//! Gloss's card-content measurement: a thin domain wrapper over the generic
//! [`use_content_size`] hook. The twin rules (pixel-exact replica of the
//! surface's scroll column) live here; the effect mechanics (defer one frame,
//! squash jitter under 2 px) live in the primitive.

use leptos::{html, prelude::*};

use crate::components::ai::types::WordInfo;
use crate::components::primitives::hooks::use_content_size::use_content_size;

/// Returns the node ref for the invisible measure twin plus the live height
/// signal it feeds.
pub fn use_content_measure(
    word: RwSignal<String>,
    word_info: RwSignal<Option<WordInfo>>,
) -> (NodeRef<html::Div>, RwSignal<f64>) {
    let measure_ref: NodeRef<html::Div> = NodeRef::new();
    let content_height = use_content_size(
        measure_ref,
        move || {
            let _ = word_info.get();
            let _ = word.get(); // title wrap can change height independently of body
        },
    );
    (measure_ref, content_height)
}
