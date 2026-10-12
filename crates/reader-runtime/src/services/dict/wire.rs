//! The wire's shapes and its asks: pack lifecycle, lookups, searches.

use std::cell::RefCell;

use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// One download's middle, as download-core draws it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProgressMirror {
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    pub speed: Option<f64>,
    pub eta_secs: Option<f64>,
    pub message: Option<String>,
}

/// A pack's row: what it is and where it stands.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PackMirror {
    pub id: String,
    pub label: String,
    pub source: String,
    pub target: String,
    pub rows: u64,
    pub built: bool,
    pub progress: Option<ProgressMirror>,
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    /// Why a phase is what it is: a failure's own words.
    pub message: Option<String>,
}

impl PackMirror {
    /// The wire phase, with the fields as a fallback for old rows.
    pub fn phase(&self) -> &str {
        if !self.phase.is_empty() {
            return &self.phase;
        }
        if self.built {
            return "ready";
        }
        match self.progress.as_ref().map(|p| p.phase.as_str()) {
            Some("downloading") | Some("preparing") | Some("retrying") | Some("verifying") => {
                "downloading"
            }
            Some("paused") => "paused",
            Some("failed") => "failed",
            _ => "absent",
        }
    }

    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let progress = self.progress.as_ref()?;
        let total = progress.total?;
        (total > 0).then_some(((progress.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }
}

/// One ranked entry on the wire to a card or a search list.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryMirror {
    pub word: String,
    pub pos_raw: Option<String>,
    /// Canon tags as snake_case words: `verb`, `noun`, ...
    pub tags: Vec<String>,
    pub definition: String,
    pub romanization: Option<String>,
    pub sense: Option<String>,
    pub pack: String,
    /// The bridge word, when two packs carried the reader here.
    pub via: Option<String>,
    /// `exact` | `prefix` | `substring` | `fuzzy`.
    pub word_match: String,
}

/// One ask's key: the word, its shores, the rank, the tightness.
#[derive(PartialEq, Clone)]
struct AnswerKey {
    word: String,
    from: String,
    to: String,
    pos: Option<String>,
    tight: bool,
}

thread_local! {
    /// Answered asks, newest first; the cap bounds the realm's memory.
    static ANSWERED: RefCell<Vec<(AnswerKey, Vec<EntryMirror>)>> =
        const { RefCell::new(Vec::new()) };
    /// The built packs the answers were asked against.
    static ANSWERED_FOR: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Asks one realm remembers; a re-hover pays no wire.
const ANSWERED_CAP: usize = 64;

fn answered_take(key: &AnswerKey) -> Option<Vec<EntryMirror>> {
    ANSWERED.with(|slot| {
        let mut slot = slot.borrow_mut();
        let at = slot.iter().position(|(have, _)| have == key)?;
        let found = slot.remove(at);
        slot.insert(0, found);
        slot.first().map(|(_, rows)| rows.clone())
    })
}

fn answered_put(key: AnswerKey, rows: &[EntryMirror]) {
    ANSWERED.with(|slot| {
        let mut slot = slot.borrow_mut();
        slot.retain(|(have, _)| *have != key);
        slot.insert(0, (key, rows.to_vec()));
        slot.truncate(ANSWERED_CAP);
    });
}

/// A shelf that moved voids its answers.
pub(super) fn note_built(rows: &[PackMirror]) {
    let built: Vec<String> = rows
        .iter()
        .filter(|pack| pack.built)
        .map(|pack| pack.id.clone())
        .collect();
    let moved = ANSWERED_FOR.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot == built {
            false
        } else {
            *slot = built;
            true
        }
    });
    if moved {
        ANSWERED.with(|slot| slot.borrow_mut().clear());
    }
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
    tight: bool,
    done: impl FnOnce(Vec<EntryMirror>) + 'static,
) {
    #[derive(Serialize)]
    struct LookupArgs {
        word: String,
        from: String,
        to: String,
        pos: Option<String>,
        tight: bool,
    }
    if !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    let key = AnswerKey {
        word: word.clone(),
        from: from.clone(),
        to: to.clone(),
        pos: pos.clone(),
        tight,
    };
    if let Some(found) = answered_take(&key) {
        done(found);
        return;
    }
    spawn_local(async move {
        let args = serde_wasm_bindgen::to_value(&LookupArgs {
            word,
            from,
            to,
            pos,
            tight,
        })
        .unwrap_or(JsValue::UNDEFINED);
        let parsed = match tauri_bridge::invoke("dict_lookup", args).await {
            Ok(value) => {
                serde_wasm_bindgen::from_value::<Vec<EntryMirror>>(value).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        };
        answered_put(key, &parsed);
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
        done(parsed);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_says_where_it_stands_in_words() {
        let mut row = PackMirror {
            built: true,
            ..PackMirror::default()
        };
        // An old row with no phase reads it off the fields.
        assert_eq!(row.phase(), "ready");
        row.built = false;
        row.progress = Some(ProgressMirror {
            phase: "downloading".into(),
            ..ProgressMirror::default()
        });
        assert_eq!(row.phase(), "downloading");
        row.progress = None;
        assert_eq!(row.phase(), "absent");
        row.phase = "converting".into();
        assert_eq!(row.phase(), "converting");
    }

    #[test]
    fn the_percent_needs_a_size() {
        let row = PackMirror {
            progress: Some(ProgressMirror {
                received: 30,
                total: Some(120),
                ..ProgressMirror::default()
            }),
            ..PackMirror::default()
        };
        assert_eq!(row.percent(), Some(25));
        let sized = PackMirror {
            progress: Some(ProgressMirror::default()),
            ..PackMirror::default()
        };
        assert_eq!(sized.percent(), None);
    }

    #[test]
    fn the_answers_keep_their_cap_and_their_shelf() {
        let key = |word: &str| AnswerKey {
            word: word.to_string(),
            from: "en".into(),
            to: "my".into(),
            pos: None,
            tight: true,
        };
        let one = EntryMirror {
            word: "light".into(),
            ..EntryMirror::default()
        };
        let pack = |id: &str, built: bool| PackMirror {
            id: id.into(),
            built,
            ..PackMirror::default()
        };
        // Seed the shelf the answers are asked against.
        note_built(&[pack("a-en-my", true)]);
        for index in 0..(ANSWERED_CAP + 4) {
            answered_put(key(&format!("w{index}")), std::slice::from_ref(&one));
        }
        ANSWERED.with(|slot| assert_eq!(slot.borrow().len(), ANSWERED_CAP));
        // The newest is answered from memory; the oldest has fallen.
        let newest = format!("w{}", ANSWERED_CAP + 3);
        assert_eq!(answered_take(&key(&newest)).map(|rows| rows.len()), Some(1));
        assert!(answered_take(&key("w0")).is_none());
        // The same shelf keeps its answers...
        note_built(&[pack("a-en-my", true)]);
        assert_eq!(answered_take(&key(&newest)).map(|rows| rows.len()), Some(1));
        // ...and a shelf that moved voids them.
        note_built(&[pack("a-en-my", false)]);
        assert!(answered_take(&key(&newest)).is_none());
        ANSWERED.with(|slot| slot.borrow_mut().clear());
        ANSWERED_FOR.with(|slot| slot.borrow_mut().clear());
    }
}
