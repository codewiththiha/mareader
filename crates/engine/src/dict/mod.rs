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
}
