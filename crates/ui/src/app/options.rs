use super::*;

mod appearance;
mod library;
mod page_ai;
mod panes;
mod read_aloud;
mod study;

impl MareaderApp {
    /// Headless knobs for `examples/shot.rs` and tests.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.reconcile_texts();
        if let Some(result) = appearance::set(self, key, value) {
            return result;
        }
        if let Some(result) = library::set(self, key, value) {
            return result;
        }
        if let Some(result) = study::set(self, key, value) {
            return result;
        }
        if let Some(result) = page_ai::set(self, key, value) {
            return result;
        }
        if let Some(result) = read_aloud::set(self, key, value) {
            return result;
        }
        if let Some(result) = panes::set(self, key, value) {
            return result;
        }
        Err(format!("unknown option {key}"))
    }
}

/// A toast from a shot's words: `_` for spaces, a prefix for its tone.
pub(super) fn shot_toast(value: &str) -> Toast {
    let words = value.replace('_', " ");
    let (kind, message) = words.split_once(':').unwrap_or(("", &words));
    match kind {
        "error" => Toast::error(message),
        "warning" => Toast::warning(message),
        "player" => Toast::info(message).player(),
        "undo" => Toast::info(message).undo(Command::ClearToasts),
        "action" => Toast::info(message).action("Resume", Command::ClearToasts),
        _ => Toast::info(words.as_str()),
    }
}
