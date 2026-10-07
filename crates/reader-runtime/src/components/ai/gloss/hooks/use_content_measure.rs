//! The card-content measurement twin, a wrapper over `use_content_size`.

use std::sync::Arc;

use ai_core::types::WordInfo;
use leptos::html;
use leptos::prelude::*;

use super::use_content_size::use_content_size;

/// The measure twin's node ref plus the live height signal it feeds.
pub fn use_content_measure(
    word: RwSignal<String>,
    word_info: RwSignal<Option<Arc<WordInfo>>>,
) -> (NodeRef<html::Div>, RwSignal<f64>) {
    let measure_ref: NodeRef<html::Div> = NodeRef::new();
    let content_height = use_content_size(measure_ref, move || {
        let _ = word_info.get();
        let _ = word.get(); // title wrap can change height independently of body
    });
    (measure_ref, content_height)
}
