//! The vocabulary feature's frontend: the mirror and lifecycle
//! here, the asks in [`query`].

pub mod query;

use std::cell::{Cell, OnceCell};

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

pub use query::{PosAnswer, fetch_levels, fetch_pos};

/// The dataset's lifecycle, as the backend reports it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DatasetMirror {
    /// `absent` | `downloading` | `paused` | `converting` | `ready` |
    /// `failed`.
    pub phase: String,
    pub received: u64,
    pub total: Option<u64>,
    /// Bytes per second, smoothed; the backend stops sending it when idle.
    pub speed: Option<f64>,
    /// Whole seconds left, when the size and the speed are both known.
    pub eta_secs: Option<u64>,
    pub words: Option<u64>,
    pub message: Option<String>,
    /// The click-time tagger model: `absent` | `downloading` | `ready` |
    /// `failed`.
    pub tagger: String,
    /// The tagger model's own percent, while it is inbound.
    pub tagger_percent: Option<u32>,
}

impl DatasetMirror {
    /// The fraction downloaded, when the size is known.
    pub fn percent(&self) -> Option<u32> {
        let total = self.total?;
        (total > 0).then_some(((self.received.min(total) as f64 / total as f64) * 100.0) as u32)
    }

    pub fn is_ready(&self) -> bool {
        self.phase == "ready"
    }

    /// The readout under the bar: a speed, and what is left at it.
    pub fn rate(&self) -> Option<String> {
        let speed = self.speed.filter(|speed| *speed > 1.0)?;
        let per_sec = if speed >= 1_048_576.0 {
            format!("{:.1} MB/s", speed / 1_048_576.0)
        } else {
            format!("{:.0} kB/s", speed / 1024.0)
        };
        match self.eta_secs {
            Some(secs) if secs > 0 => Some(format!("{per_sec} — {secs}s left")),
            _ => Some(per_sec),
        }
    }
}

thread_local! {
    /// This realm's one mirror handle, bound before any component reads.
    static MIRROR: OnceCell<RwSignal<Option<DatasetMirror>>> = const { OnceCell::new() };
    /// The backend tap is installed once per realm.
    static TAPPED: OnceCell<()> = const { OnceCell::new() };
    /// One warn per realm: a dead backend must not flood the console.
    static LOOKUP_WARNED: OnceCell<()> = const { OnceCell::new() };
    /// The dataset the mirror last named, by its word count.
    static DATASET_WORDS: Cell<Option<u64>> = const { Cell::new(None) };
}

/// The dataset mirror to read this realm; `None` phases in from the bridge.
pub fn dataset() -> RwSignal<Option<DatasetMirror>> {
    MIRROR.with(|slot| *slot.get_or_init(|| RwSignal::new(None)))
}

/// One warn per realm for a dead backend.
pub(super) fn lookup_warned() -> bool {
    LOOKUP_WARNED.with(|slot| slot.set(())).is_ok()
}

/// Bind this realm's mirror and tap the backend's progress.
pub fn install_cefr_bridge() {
    // Realm-global: mint it in this owner, not the first transient reader's.
    let mirror = dataset();
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        if let Ok(value) = tauri_bridge::invoke("cefr_dataset_status", JsValue::UNDEFINED).await
            && let Ok(status) = serde_wasm_bindgen::from_value::<DatasetMirror>(value)
        {
            mirror.try_set(Some(status));
        }
    });
    if TAPPED.with(|slot| slot.set(())).is_err() {
        return;
    }
    crate::services::tauri_listen("cefr-dataset-progress", |ev: web_sys::Event| {
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);
        match serde_wasm_bindgen::from_value::<DatasetMirror>(payload) {
            Ok(status) => publish(status),
            Err(e) => web_sys::console::warn_1(&format!("[cefr] bad payload: {e:?}").into()),
        }
    });
}

/// Hand a status snapshot to this realm's mirror.
fn publish(status: DatasetMirror) {
    // A dataset that moved voids the tagger's answers over the old.
    let moved = DATASET_WORDS.with(|slot| slot.replace(status.words) != status.words);
    if moved {
        query::clear_tagged();
    }
    dataset().try_set(Some(status));
}

/// Ask the backend to start the resumable download.
pub fn request_download() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_dataset_download", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] download kickoff failed: {e:?}").into());
        }
    });
}

/// Ask a running download to stop; the partial stays for a resume.
pub fn request_cancel() {
    spawn_local(async move {
        let _ = tauri_bridge::invoke("cefr_dataset_cancel", JsValue::UNDEFINED).await;
    });
}

/// Stop reading the stream; the partial stays for a resume.
pub fn request_pause() {
    spawn_local(async move {
        let _ = tauri_bridge::invoke("cefr_dataset_pause", JsValue::UNDEFINED).await;
    });
}

/// Continue a paused download from the bytes already on disk.
pub fn request_resume() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_dataset_resume", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] resume failed: {e:?}").into());
        }
    });
}

/// Delete the dataset and its partials.
pub fn request_remove() {
    spawn_local(async move {
        if let Err(e) = tauri_bridge::invoke("cefr_dataset_remove", JsValue::UNDEFINED).await {
            web_sys::console::warn_1(&format!("[cefr] remove failed: {e:?}").into());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mirror(phase: &str, received: u64, total: Option<u64>) -> DatasetMirror {
        DatasetMirror {
            phase: phase.into(),
            received,
            total,
            ..Default::default()
        }
    }

    #[test]
    fn percent_needs_a_size() {
        assert_eq!(mirror("downloading", 10, None).percent(), None);
        assert_eq!(mirror("downloading", 10, Some(0)).percent(), None);
        assert_eq!(mirror("downloading", 10, Some(40)).percent(), Some(25));
    }

    #[test]
    fn a_paused_mirror_keeps_the_bytes_it_had() {
        let paused = mirror("downloading", 30, Some(100));
        assert_eq!(paused.percent(), Some(30));
        assert!(!paused.is_ready());
        assert!(mirror("ready", 0, None).is_ready());
    }

    #[test]
    fn the_rate_readout_needs_a_real_speed() {
        assert_eq!(mirror("downloading", 0, None).rate(), None);
        let mut slow = mirror("downloading", 100, Some(200));
        slow.speed = Some(0.5);
        assert_eq!(slow.rate(), None, "a crawl is not an estimate");
        slow.speed = Some(512_000.0);
        assert_eq!(slow.rate().as_deref(), Some("500 kB/s"));
        slow.speed = Some(2_097_152.0);
        slow.eta_secs = Some(3);
        assert_eq!(slow.rate().as_deref(), Some("2.0 MB/s — 3s left"));
    }

    #[test]
    fn the_wire_shape_tolerates_a_backend_that_sends_less() {
        let parsed: DatasetMirror =
            serde_json::from_str(r#"{"phase":"ready","received":0,"words":1234}"#).unwrap();
        assert!(parsed.is_ready());
        assert_eq!(parsed.words, Some(1234));
        assert_eq!(parsed.total, None);
        assert_eq!(parsed.speed, None);
    }

    #[test]
    fn a_dataset_that_moves_voids_its_tags() {
        let keyed = ("light".to_string(), "a lamp".to_string());
        let base = mirror("ready", 0, Some(10));
        let with_words = |words: u64| DatasetMirror {
            words: Some(words),
            ..base.clone()
        };
        DATASET_WORDS.with(|slot| slot.set(None));
        query::clear_tagged();
        publish(with_words(1234));
        query::remember(
            keyed.clone(),
            PosAnswer {
                pos: "NN".into(),
                ..PosAnswer::default()
            },
        );
        assert!(query::tagged_take(&keyed).is_some());
        // The same dataset keeps its tags...
        publish(with_words(1234));
        assert!(query::tagged_take(&keyed).is_some());
        // ...and a replaced one voids them.
        publish(with_words(4321));
        assert!(query::tagged_take(&keyed).is_none());
        DATASET_WORDS.with(|slot| slot.set(None));
    }
}
