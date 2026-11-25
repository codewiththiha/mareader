//! What the dictionary page asks for: a pack's downloads and its card.

pub mod service;

pub use service::Service;

/// A download's buttons: Start also resumes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Download {
    Start,
    Pause,
    Cancel,
    Remove,
}

/// The reader's floating dictionary window, as a command moves it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Window {
    /// Shown if closed or folded, else closed.
    Toggle,
    /// Shown, its field given the keys; the shortcut on an idle window.
    Focus,
    Close,
    Minimize(bool),
    /// Let go after a drag or resize: x, y, width, height; saved.
    Place([f32; 4]),
    /// Closed, and the Dictionary page in the panes' place.
    Expand,
}

/// Packs to download or remove, and the lookup card's asks.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Download(&'static str),
    Pause(&'static str),
    Cancel(&'static str),
    Remove(&'static str),
    CloseCard,
    /// The card's word, carried on into the window or the page.
    Pursue(String),
    /// The reader's floating window, which the app keeps.
    Window(Window),
    /// Drops an AI dictionary entry.
    Forget(Box<mareader_dict::Saved>),
    /// Writes an edited AI dictionary entry over its old self.
    Edit {
        old: Box<mareader_dict::Saved>,
        new: Box<mareader_dict::Saved>,
        /// Overwrites an entry already under the new key.
        replace: bool,
    },
    /// Gives up an edit that found its new key taken, or failed.
    DropEdit,
}

/// Where an AI dictionary entry's edit or removal stands.
#[derive(Clone, Debug, PartialEq)]
pub enum Edited {
    Waiting,
    /// Another entry holds the new word, class and language.
    Taken(Box<mareader_dict::Saved>),
    Failed(String),
}

/// The selection on one line, edge punctuation trimmed; none past `max`.
pub fn lookup_text(selected: &str, max: usize) -> Option<String> {
    let joined = selected.split_whitespace().collect::<Vec<_>>().join(" ");
    let edge = |c: char| !c.is_alphanumeric() && c != '%';
    let word = joined.trim_matches(edge);
    let fits = !word.is_empty() && word.chars().count() <= max;
    fits.then(|| word.to_owned())
}

#[cfg(test)]
mod tests {
    use super::lookup_text;

    #[test]
    fn a_selection_becomes_a_tidy_lookup() {
        assert_eq!(lookup_text("  “Light,”\n", 40), Some("Light".into()));
        assert_eq!(lookup_text("100% sure.", 40), Some("100% sure".into()));
        assert_eq!(lookup_text("it’s", 40), Some("it’s".into()));
        assert_eq!(lookup_text("…", 40), None);
        assert_eq!(lookup_text("a long, long phrase", 6), None);
    }
}
