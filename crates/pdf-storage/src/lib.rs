//! Key-value persistence abstraction.
//!
//! The app talks to [`PdfStorage`], never to a concrete backend. Today the
//! only live impl is [`local::LocalStorage`] (what the app always used,
//! reached through `window.localStorage`). A SQLite impl exists behind the
//! `sqlite` feature; switching backends is a one-line change at startup.

pub mod local;

#[cfg(feature = "sqlite")]
pub mod sqlite;

pub use local::LocalStorage;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStorage;

/// Minimal key-value store for persisted app state.
///
/// `Send + Sync` so a backend can live behind a reference from any thread;
/// the wasm impls are trivially safe.
pub trait PdfStorage: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn remove(&self, key: &str);
}
