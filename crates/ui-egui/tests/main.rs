//! UI integration suites sharing one test binary and common drivers.

mod a11y;
mod arrange;
mod auto_scroll;
mod blend;
mod chrome;
mod common;
mod dial_deck;
#[cfg(not(target_arch = "wasm32"))]
mod dictionary;
mod find;
mod focus;
mod gpu;
mod keys;
mod library;
mod modes;
mod navigation;
mod option_keys;
mod organize;
mod page_ai;
mod page_groups;
mod pages;
mod panel;
mod pronunciation;
mod read_aloud;
mod reading;
mod search;
mod selection;
mod shell;
mod study;
mod thumb_picks;
#[cfg(not(target_arch = "wasm32"))]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod vocabulary;
mod voice_models;
