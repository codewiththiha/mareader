//! localStorage-backed [`PdfStorage`]. Identical behaviour to the original
//! `window.PDFReader.storageGet/storageSet` wrappers, minus the indirection.

use super::PdfStorage;

#[derive(Debug, Clone, Copy, Default)]
pub struct LocalStorage;

impl PdfStorage for LocalStorage {
    fn get(&self, key: &str) -> Option<String> {
        web_sys::window()?
            .local_storage()
            .ok()??
            .get_item(key)
            .ok()?
    }

    fn set(&self, key: &str, value: &str) {
        if let Some(Some(storage)) = web_sys::window().and_then(|w| w.local_storage().ok()) {
            let _ = storage.set_item(key, value);
        }
    }

    fn remove(&self, key: &str) {
        if let Some(Some(storage)) = web_sys::window().and_then(|w| w.local_storage().ok()) {
            let _ = storage.remove_item(key);
        }
    }
}
