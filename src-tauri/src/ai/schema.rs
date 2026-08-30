//! The provider-facing output contract: the `WordInfo` payload every
//! provider streams back, and — on Apple Silicon builds — the fm-bridge
//! schema that forces the on-device model into constrained decoding for
//! exactly that shape.

use serde::{Deserialize, Serialize};

#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
use fm_bridge::{Schema, SchemaProperty};

/// The exact data structure we want the AI to return.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WordInfo {
    pub pos: String,       // Part of Speech (e.g., "noun", "verb")
    pub meaning: String,   // Simplified explanation
    pub synonyms: Vec<String>,
    pub usages: Vec<String>, // Example sentences
}

/// Forces the Apple Intelligence model into constrained decoding.
/// It will ONLY generate valid JSON that matches this exact shape.
///
/// Gated to the same targets as the `fm-bridge` dependency itself: schema
/// construction is only ever needed by the Apple provider, and importing
/// the crate elsewhere would break fallback builds.
#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
pub fn word_info_schema() -> Schema {
    Schema::new(
        "WordInfo",
        vec![
            SchemaProperty::string("pos")
                .description("The part of speech of the word (e.g., noun, verb, adjective)."),
            SchemaProperty::string("meaning")
                .description("A simplified, easy-to-understand meaning of the word in the given context."),
            SchemaProperty::array("synonyms", SchemaProperty::string("word"))
                .description("A list of 2 to 5 synonyms.")
                .count(2, 5),
            SchemaProperty::array("usages", SchemaProperty::string("sentence"))
                .description("A list of 2 to 3 example sentences using the word in context.")
                .count(2, 3),
        ],
    )
}
