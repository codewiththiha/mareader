//! The dataset's answers: word levels in batch, one word's role on a click.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use tauri::AppHandle;

use super::CefrManager;

/// The largest batch one lookup may carry; a page stays far below it.
const MAX_BATCH: usize = 4_000;

/// The dataset's POS verdict for one clicked word.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PosAnswer {
    /// The Penn Treebank tag at the word's position in its sentence.
    pub pos: String,
    /// The readable word class: noun, verb, adjective, ...
    pub kind: String,
    /// The dataset sense that answered, when one did.
    pub sense: Option<String>,
    /// The answering sense's level.
    pub level: Option<f64>,
    /// Every POS sense the dataset lists for the word.
    pub senses: Vec<String>,
}

impl CefrManager {
    /// Levels for `words`, aligned with the input.
    pub fn levels(&self, app: &AppHandle, words: &[String]) -> Result<Vec<Option<f64>>, String> {
        if words.len() > MAX_BATCH {
            return Err(format!("{} words is over a {MAX_BATCH} batch", words.len()));
        }
        let paths = Self::paths(app)?;
        // One normalize per word; a non-word keeps its place as `None`.
        let keys: Vec<Option<String>> = words
            .iter()
            .map(|word| cefr_core::is_english_ascii(word).then(|| cefr_core::normalize(word)))
            .collect();
        let mut seen: HashSet<&str> = HashSet::new();
        let unique: Vec<String> = keys
            .iter()
            .flatten()
            .map(String::as_str)
            .filter(|key| seen.insert(key))
            .map(str::to_string)
            .collect();
        let Some(found) = self.with_db(&paths.db, |db| {
            db.lookup_words(&unique).map_err(|e| format!("lookup: {e}"))
        })?
        else {
            // An all-`None` answer would cache a wrong band page-wide.
            return Err("the dataset is not ready".into());
        };
        Ok(keys
            .iter()
            .map(|key| key.as_ref().and_then(|key| found.get(key).copied()))
            .collect())
    }

    /// The dataset's POS for `word` in `sentence`; `Ok(None)` if unequipped.
    pub fn pos_of(
        &self,
        app: &AppHandle,
        word: &str,
        sentence: &str,
    ) -> Result<Option<PosAnswer>, String> {
        let Some(tagger) = self.tagger(app)? else {
            return Ok(None);
        };
        let Some(found) = tagger.pos_in_context(word, sentence) else {
            return Ok(None);
        };
        let paths = Self::paths(app)?;
        let tag = found.pos.clone();
        // The table's keys are the dataset's words: `Ephemeral` must
        // ask for `ephemeral`.
        let key = cefr_core::normalize(word);
        let Some((sense, all)) = self.with_db(&paths.db, |db| {
            let sense = db
                .sense_level(&key, &tag)
                .map_err(|e| format!("pos lookup: {e}"))?;
            let all = db.pos_senses(&key).map_err(|e| format!("senses: {e}"))?;
            Ok((sense, all))
        })?
        else {
            return Ok(None);
        };
        Ok(Some(PosAnswer {
            kind: cefr::tags::kind_of(&tag).to_string(),
            // The sense that answered may be a tag-family neighbour; the tag
            // stays the tagger's.
            pos: tag,
            sense: sense.as_ref().map(|answer| answer.pos.clone()),
            level: sense.map(|answer| answer.level),
            senses: all.into_iter().map(|(pos, _)| pos).collect(),
        }))
    }

    /// Run `read` against the opened dataset; `Ok(None)` means there is none.
    fn with_db<R>(
        &self,
        path: &Path,
        read: impl FnOnce(&cefr::db::CefrDb) -> Result<R, String>,
    ) -> Result<Option<R>, String> {
        let mut slot = self.db.lock().map_err(|_| "db lock poisoned")?;
        if slot.is_none() {
            if !path.is_file() {
                return Ok(None);
            }
            *slot = Some(cefr::db::CefrDb::open(path).map_err(|e| format!("dataset: {e}"))?);
        }
        let Some(db) = slot.as_ref() else {
            return Ok(None);
        };
        read(db).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pos_answer_uses_the_camel_case_wire() {
        let json = serde_json::to_string(&PosAnswer {
            pos: "VB".into(),
            kind: "verb".into(),
            sense: Some("VB".into()),
            level: Some(4.2),
            senses: vec!["NN".into(), "VB".into()],
        })
        .unwrap();
        assert!(json.contains("\"pos\":\"VB\""));
        assert!(json.contains("\"kind\":\"verb\""));
        assert!(json.contains("\"level\":4.2"));
        assert!(!json.contains("pos_tag"));
    }

    #[test]
    fn the_word_class_is_the_dataset_crate_s_not_a_restatement() {
        // One home for the mapping: `NN` is a noun in the CLI too.
        assert_eq!(cefr::tags::kind_of("NN"), "noun");
        assert_eq!(cefr::tags::kind_of("NNS"), "noun");
        assert_eq!(cefr::tags::kind_of("VBG"), "verb");
        assert_eq!(cefr::tags::kind_of("MD"), "modal verb");
        assert_eq!(cefr::tags::kind_of("XX"), "other");
    }
}
