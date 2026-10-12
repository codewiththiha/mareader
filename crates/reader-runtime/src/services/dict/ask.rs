//! The dictionary's asks: a word's senses, a route's rows, a
//! pack's lifecycle verb.

use std::cell::RefCell;
use std::collections::VecDeque;

use leptos::task::spawn_local;
use serde::Serialize;
use wasm_bindgen::JsValue;

use super::rows::EntryMirror;

/// Answers the card keeps; the oldest goes first at the cap.
const LOOKUP_CAP: usize = 96;
/// Search results the panel keeps; evicted oldest-first at the cap.
const SEARCH_CAP: usize = 32;

thread_local! {
    /// Newest-first answers, keyed by the whole ask. Plain data only.
    static LOOKUPS: RefCell<VecDeque<(LookupKey, Vec<EntryMirror>)>> =
        const { RefCell::new(VecDeque::new()) };
    /// Newest-first search answers, invalidated whenever packs change.
    static SEARCHES: RefCell<VecDeque<(SearchKey, Vec<EntryMirror>)>> =
        const { RefCell::new(VecDeque::new()) };
}

/// What one answer depends on: the word, both shores, the role.
type LookupKey = (String, String, String, Option<String>);
/// What one search depends on: the query, pack filter, and shores.
type SearchKey = (String, Option<Vec<String>>, Option<String>, Option<String>);

/// Answers the cache holds, for the diagnostics gauge.
pub(super) fn entries() -> usize {
    LOOKUPS.with(|cache| cache.borrow().len())
}

/// Drop every answer: a pack that landed or left changes them all.
pub(super) fn forget() {
    LOOKUPS.with(|cache| cache.borrow_mut().clear());
    SEARCHES.with(|cache| cache.borrow_mut().clear());
}

/// The answer a key already has, pushed back to the front.
fn remembered(key: &LookupKey) -> Option<Vec<EntryMirror>> {
    LOOKUPS.with(|cache| {
        let mut cache = cache.borrow_mut();
        let at = cache.iter().position(|(have, _)| have == key)?;
        let (_, entries) = cache.remove(at)?;
        cache.push_front((key.clone(), entries.clone()));
        Some(entries)
    })
}

/// Keep one answer, oldest first at the cap.
fn remember(key: LookupKey, entries: Vec<EntryMirror>) {
    LOOKUPS.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|(have, _)| have != &key);
        cache.push_front((key, entries));
        cache.truncate(LOOKUP_CAP);
    });
}

fn remembered_search(key: &SearchKey) -> Option<Vec<EntryMirror>> {
    SEARCHES.with(|cache| {
        let mut cache = cache.borrow_mut();
        let at = cache.iter().position(|(have, _)| have == key)?;
        let (_, entries) = cache.remove(at)?;
        cache.push_front((key.clone(), entries.clone()));
        Some(entries)
    })
}

fn remember_search(key: SearchKey, entries: Vec<EntryMirror>) {
    SEARCHES.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.retain(|(have, _)| have != &key);
        cache.push_front((key, entries));
        cache.truncate(SEARCH_CAP);
    });
}

/// One lifecycle verb per pack: download | pause | resume | cancel | remove.
pub fn request_pack(pack_id: &str, verb: &str) {
    #[derive(Serialize)]
    struct PackArgs {
        #[serde(rename = "packId")]
        pack_id: String,
    }
    let name = format!("dict_pack_{verb}");
    let verb = verb.to_string();
    let args = serde_wasm_bindgen::to_value(&PackArgs {
        pack_id: pack_id.to_string(),
    })
    .unwrap_or(JsValue::UNDEFINED);
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke(&name, args).await {
            web_sys::console::warn_1(&format!("[dict] {verb} failed: {e:?}").into());
        }
    });
}

/// The hover's ask: `word` between two languages.
/// POS ranks senses; never gates them.
pub fn lookup(
    word: String,
    from: String,
    to: String,
    pos: Option<String>,
    done: impl FnOnce(Vec<EntryMirror>) + 'static,
) {
    #[derive(Serialize)]
    struct LookupArgs {
        word: String,
        from: String,
        to: String,
        pos: Option<String>,
    }
    if !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    // A word already hovered answers from memory: one ask per
    // word, not one per glance.
    let key = (word.clone(), from.clone(), to.clone(), pos.clone());
    if let Some(entries) = remembered(&key) {
        done(entries);
        return;
    }
    spawn_local(async move {
        let args = serde_wasm_bindgen::to_value(&LookupArgs {
            word,
            from,
            to,
            pos,
        })
        .unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("dict_lookup", args).await {
            Ok(value) => {
                serde_wasm_bindgen::from_value::<Vec<EntryMirror>>(value).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        };
        remember(key, parsed.clone());
        done(parsed);
    });
}

/// The route's ask: a word on either side, or between two shores.
pub fn search(
    ask: String,
    pack_ids: Option<Vec<String>>,
    from: Option<String>,
    to: Option<String>,
    done: impl FnOnce(Vec<EntryMirror>) + 'static,
) {
    #[derive(Serialize)]
    struct SearchArgs {
        ask: String,
        #[serde(rename = "packIds")]
        pack_ids: Option<Vec<String>>,
        from: Option<String>,
        to: Option<String>,
    }
    if !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    let key = (ask.clone(), pack_ids.clone(), from.clone(), to.clone());
    if let Some(entries) = remembered_search(&key) {
        done(entries);
        return;
    }
    spawn_local(async move {
        let args = serde_wasm_bindgen::to_value(&SearchArgs {
            ask,
            pack_ids,
            from,
            to,
        })
        .unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("dict_search", args).await {
            Ok(value) => {
                serde_wasm_bindgen::from_value::<Vec<EntryMirror>>(value).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        };
        remember_search(key, parsed.clone());
        done(parsed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(word: &str) -> LookupKey {
        (word.to_string(), "en".to_string(), "my".to_string(), None)
    }

    fn answer(word: &str) -> Vec<EntryMirror> {
        vec![EntryMirror {
            word: word.to_string(),
            ..EntryMirror::default()
        }]
    }

    #[test]
    fn an_answer_kept_comes_back() {
        forget();
        remember(key("light"), answer("light"));
        assert_eq!(remembered(&key("light")).map(|got| got.len()), Some(1));
    }

    #[test]
    fn another_shore_is_another_ask() {
        forget();
        remember(key("light"), answer("light"));
        let other = (
            "light".to_string(),
            "en".to_string(),
            "jp".to_string(),
            None,
        );
        assert!(remembered(&other).is_none());
    }

    #[test]
    fn the_cap_drops_the_oldest_answer() {
        forget();
        for index in 0..(LOOKUP_CAP + 4) {
            remember(key(&index.to_string()), answer("x"));
        }
        assert!(remembered(&key("0")).is_none(), "the oldest is gone");
        assert!(remembered(&key(&LOOKUP_CAP.to_string())).is_some());
    }

    #[test]
    fn a_remembered_answer_moves_to_the_front() {
        forget();
        for index in 0..LOOKUP_CAP {
            remember(key(&index.to_string()), answer("x"));
        }
        // Asking for the oldest saves it from the next eviction.
        assert!(remembered(&key("0")).is_some());
        remember(key("fresh"), answer("x"));
        assert!(remembered(&key("0")).is_some());
        assert!(remembered(&key("1")).is_none());
    }

    #[test]
    fn forgetting_drops_every_answer() {
        forget();
        remember(key("light"), answer("light"));
        forget();
        assert!(remembered(&key("light")).is_none());
    }
}
