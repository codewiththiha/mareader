//! The machine a browser runs on: a clipboard, and no files at all.

use std::path::{Path, PathBuf};

use eframe::egui::Context;
use mareader_engine::host::Host;

/// The page's own table: what a browser can answer, and what it cannot.
pub struct Table {
    ctx: Context,
}

impl Table {
    /// The machine for a page drawn in `ctx`.
    pub fn new(ctx: Context) -> Self {
        Self { ctx }
    }
}

impl Host for Table {
    /// No files at all: the page's own store keeps the engine's two.
    fn state_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Nothing here can be thrown away, because nothing here is kept.
    fn cache_dir(&self) -> Option<PathBuf> {
        None
    }

    fn set_clipboard(&self, text: String) {
        self.ctx.copy_text(text);
    }

    /// A page has no file manager to show a path in.
    fn reveal(&self, _path: &Path) -> bool {
        false
    }

    /// A clip copied out has no path a page could hand to a player.
    fn open_external(&self, _path: &Path) -> bool {
        false
    }
}
