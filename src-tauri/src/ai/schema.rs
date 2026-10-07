//! The provider-facing contract: the `WordInfo` payload providers stream.

use serde::{Deserialize, Serialize};

#[cfg(all(feature = "ai", target_os = "macos", target_arch = "aarch64"))]
use fm_bridge::{Schema, SchemaProperty};

/// The exact data structure we want the AI to return.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WordInfo {
    pub pos: String,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usages: Vec<String>,
}

/// Forces the model into constrained decoding, matching the WordInfo shape.
pub fn word_info_schema() -> Schema {
    Schema::new(
        "WordInfo",
        vec![
            SchemaProperty::string("pos")
                .description("The part of speech of the word (e.g., noun, verb, adjective)."),
            SchemaProperty::string("meaning").description(
                "A simplified, easy-to-understand meaning of the word in the given context.",
            ),
            SchemaProperty::array("synonyms", SchemaProperty::string("word"))
                .description("A list of 2 to 5 synonyms.")
                .count(2, 5),
            SchemaProperty::array("usages", SchemaProperty::string("sentence"))
                .description("A list of 2 to 3 example sentences using the word in context.")
                .count(2, 3),
        ],
    )
}
