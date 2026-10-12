//! The vocabulary feature's asks: batched levels and one word's role.

use std::cell::RefCell;

use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// The backend's dataset POS answer for one clicked word.
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PosAnswer {
    pub pos: String,
    pub kind: String,
    pub sense: Option<String>,
    pub level: Option<f64>,
    pub senses: Vec<String>,
}

/// A tagged ask's key: the word and the sentence it was read in.
type PosKey = (String, String);

thread_local! {
    /// Tagged words, newest first; the cap bounds the realm's memory.
    static TAGGED: RefCell<Vec<(PosKey, PosAnswer)>> = const { RefCell::new(Vec::new()) };
}

/// Tags one realm remembers; the model costs seconds of CPU.
const TAGGED_CAP: usize = 256;

pub(super) fn tagged_take(key: &PosKey) -> Option<PosAnswer> {
    TAGGED.with(|slot| {
        let mut slot = slot.borrow_mut();
        let at = slot.iter().position(|(have, _)| have == key)?;
        let found = slot.remove(at);
        slot.insert(0, found);
        slot.first().map(|(_, answer)| answer.clone())
    })
}

/// One answer the model gave, remembered newest-first.
pub(super) fn remember(key: PosKey, answer: PosAnswer) {
    TAGGED.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.retain(|(have, _)| *have != key);
        slot.insert(0, (key, answer));
        slot.truncate(TAGGED_CAP);
    });
}

/// A dataset that moved voids its tags.
pub(super) fn clear_tagged() {
    TAGGED.with(|slot| slot.borrow_mut().clear());
}

/// The lookup command's wire shape: a named `words` field.
#[derive(Serialize)]
struct LevelsArgs {
    words: Vec<String>,
}

/// Levels for `keys`, aligned with the input.
///
/// `done` always runs; a failure releases its keys.
pub fn fetch_levels(keys: Vec<String>, done: impl FnOnce(Vec<Option<f64>>) + 'static) {
    if keys.is_empty() || !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    // Tauri binds command parameters by name: the array rides NAMED.
    let args = match serde_wasm_bindgen::to_value(&LevelsArgs { words: keys }) {
        Ok(args) => args,
        Err(_) => {
            done(Vec::new());
            return;
        }
    };
    spawn_local(async move {
        match tauri_bridge::invoke("cefr_levels", args).await {
            Ok(value) => match serde_wasm_bindgen::from_value::<Vec<Option<f64>>>(value) {
                Ok(levels) => done(levels),
                Err(_) => done(Vec::new()),
            },
            Err(e) => {
                if super::lookup_warned() {
                    web_sys::console::warn_1(&format!("[cefr] lookup failed: {e:?}").into());
                }
                done(Vec::new());
            }
        }
    });
}

/// The dataset's verdict for `word` in `sentence`; `None` if
/// unequipped. A model's answer is remembered.
pub fn fetch_pos(word: String, context: String, done: impl FnOnce(Option<PosAnswer>) + 'static) {
    #[derive(Serialize)]
    struct PosArgs {
        word: String,
        context: String,
    }
    if !tauri_bridge::has_tauri() {
        done(None);
        return;
    }
    let key = (word.clone(), context.clone());
    if let Some(found) = tagged_take(&key) {
        done(Some(found));
        return;
    }
    spawn_local(async move {
        let args =
            serde_wasm_bindgen::to_value(&PosArgs { word, context }).unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("cefr_pos_of", args).await {
            Ok(value) => serde_wasm_bindgen::from_value::<Option<PosAnswer>>(value)
                .ok()
                .flatten(),
            Err(_) => None,
        };
        if let Some(answer) = &parsed {
            remember(key, answer.clone());
        }
        done(parsed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pos_answer_uses_the_camel_case_wire() {
        let json = r#"{"pos":"VB","kind":"verb","sense":null,"level":4.2,"senses":[]}"#;
        let parsed: PosAnswer = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.pos, "VB");
        assert_eq!(parsed.level, Some(4.2));
    }

    #[test]
    fn a_tag_is_remembered_and_the_shelf_holds_its_cap() {
        let key = |word: &str| (word.to_string(), "a sentence".to_string());
        let answer = PosAnswer {
            pos: "NN".into(),
            kind: "noun".into(),
            ..PosAnswer::default()
        };
        for index in 0..(TAGGED_CAP + 4) {
            remember(key(&format!("w{index}")), answer.clone());
        }
        TAGGED.with(|slot| assert_eq!(slot.borrow().len(), TAGGED_CAP));
        let newest = format!("w{}", TAGGED_CAP + 3);
        assert_eq!(tagged_take(&key(&newest)).map(|a| a.pos), Some("NN".into()));
        assert!(tagged_take(&key("w0")).is_none());
        clear_tagged();
        TAGGED.with(|slot| assert!(slot.borrow().is_empty()));
    }
}
