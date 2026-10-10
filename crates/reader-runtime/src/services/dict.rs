//! The dictionary's frontend: pack rows and ranked lookups.

use std::cell::OnceCell;

use leptos::prelude::*;
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
    pub eta_secs: Option<u64>,
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

thread_local! {
    /// This realm's pack rows, bound before any component reads.
    static PACKS: OnceCell<RwSignal<Vec<PackMirror>>> = const { OnceCell::new() };
    /// The backend tap is installed once per realm.
    static TAPPED: OnceCell<()> = const { OnceCell::new() };
}

/// The pack rows to read this realm.
pub fn packs() -> RwSignal<Vec<PackMirror>> {
    PACKS.with(|slot| *slot.get_or_init(|| RwSignal::new(Vec::new())))
}

/// The built pack that answers a card: the asked language's, or the
/// first one built.
pub fn answer_pack<'a>(rows: &'a [PackMirror], lang: Option<&str>) -> Option<&'a PackMirror> {
    rows.iter()
        .find(|pack| pack.built && lang.is_some_and(|want| pack.target == want))
        .or_else(|| rows.iter().find(|pack| pack.built))
}

/// Every built pack's id, in row order.
pub fn built_packs(rows: &[PackMirror]) -> Vec<String> {
    rows.iter()
        .filter(|pack| pack.built)
        .map(|pack| pack.id.clone())
        .collect()
}

/// The built packs `langs` names: an id, or a `source-target` pair.
/// Empty asks every pack.
pub fn named_packs(rows: &[PackMirror], langs: &[String]) -> Vec<String> {
    if langs.is_empty() {
        return built_packs(rows);
    }
    rows.iter()
        .filter(|pack| {
            pack.built
                && (langs.contains(&pack.id)
                    || langs.contains(&format!("{}-{}", pack.source, pack.target)))
        })
        .map(|pack| pack.id.clone())
        .collect()
}

/// Bind this realm's rows and tap the backend's progress.
pub fn install_dict_bridge() {
    // Realm-global: mint it in this owner, not the first transient reader's.
    let _ = packs();
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("dict_packs_status", JsValue::UNDEFINED).await
            && let Ok(rows) = serde_wasm_bindgen::from_value::<Vec<PackMirror>>(value)
        {
            packs().set(rows);
        }
    });
    if TAPPED.with(|slot| slot.set(())).is_err() {
        return;
    }
    crate::services::tauri_listen("dict-packs", |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<Vec<PackMirror>>(payload) {
            Ok(rows) => packs().set(rows),
            Err(e) => web_sys::console::warn_1(&format!("[dict] bad payload: {e:?}").into()),
        }
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
        done(parsed);
    });
}

/// The route's ask: a word on either side of a built
/// pack.
pub fn search(
    ask: String,
    pack_ids: Option<Vec<String>>,
    done: impl FnOnce(Vec<EntryMirror>) + 'static,
) {
    #[derive(Serialize)]
    struct SearchArgs {
        ask: String,
        #[serde(rename = "packIds")]
        pack_ids: Option<Vec<String>>,
    }
    if !tauri_bridge::has_tauri() {
        done(Vec::new());
        return;
    }
    spawn_local(async move {
        let args = serde_wasm_bindgen::to_value(&SearchArgs { ask, pack_ids })
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

    /// One row: an English-sourced pack, its target, and whether built.
    fn pack(id: &str, target: &str, built: bool) -> PackMirror {
        PackMirror {
            id: id.to_string(),
            source: "en".to_string(),
            target: target.to_string(),
            built,
            ..PackMirror::default()
        }
    }

    #[test]
    fn the_asked_language_answers_before_the_row_order() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let got = answer_pack(&rows, Some("my"));
        assert_eq!(got.map(|pack| pack.id.as_str()), Some("b-en-my"));
    }

    #[test]
    fn no_asked_language_answers_the_first_built_pack() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let got = answer_pack(&rows, None);
        assert_eq!(got.map(|pack| pack.id.as_str()), Some("a-en-jp"));
    }

    #[test]
    fn a_language_nobody_built_falls_to_one_that_is() {
        let rows = vec![pack("a-en-fr", "fr", false), pack("b-en-my", "my", true)];
        let got = answer_pack(&rows, Some("fr"));
        assert_eq!(got.map(|pack| pack.id.as_str()), Some("b-en-my"));
    }

    #[test]
    fn a_pack_still_building_never_answers() {
        let rows = vec![pack("a-en-fr", "fr", false)];
        assert_eq!(answer_pack(&rows, Some("fr")), None);
    }

    #[test]
    fn a_named_pack_is_reachable_by_its_pair_too() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-my", "my", true)];
        let langs = vec!["en-my".to_string()];
        assert_eq!(named_packs(&rows, &langs), vec!["b-en-my"]);
    }

    #[test]
    fn an_empty_list_asks_every_built_pack() {
        let rows = vec![pack("a-en-jp", "jp", true), pack("b-en-fr", "fr", false)];
        assert_eq!(named_packs(&rows, &[]), vec!["a-en-jp"]);
    }
}
