//! What a toast says, and what its button sends: the data, not the drawing.

use crate::command::Command;

/// How long a note or a result stays up.
pub const SHOW_FOR: f64 = 3.5;
/// A failure stays longer: it may need reading twice.
pub const ERROR_FOR: f64 = 6.0;
/// A toast with a button stays as long as an Undo is offered.
pub const UNDO_FOR: f64 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// A result or a note: a check mark.
    Info,
    /// Something failed: an alert in the danger colour.
    Error,
    /// It worked, but not wholly: an alert in amber.
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    pub message: String,
    pub tone: Tone,
    /// A button, Undo unless said, and what it sends; it lasts longer.
    pub undo: Option<Box<Command>>,
    pub label: &'static str,
    pub player: bool,
}

impl Toast {
    pub fn info(message: impl Into<String>) -> Self {
        Self::new(message, Tone::Info)
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, Tone::Error)
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, Tone::Warning)
    }

    fn new(message: impl Into<String>, tone: Tone) -> Self {
        Self {
            message: message.into(),
            tone,
            undo: None,
            label: "Undo",
            player: false,
        }
    }

    pub fn player(self) -> Self {
        Self {
            player: true,
            ..self
        }
    }

    /// With an Undo button that sends `command`.
    pub fn undo(self, intent: Command) -> Self {
        Self {
            undo: Some(Box::new(intent)),
            ..self
        }
    }

    /// With a button `label` that sends `command`, as Resume.
    pub fn action(self, label: &'static str, intent: Command) -> Self {
        Self {
            undo: Some(Box::new(intent)),
            label,
            ..self
        }
    }

    pub fn lasts(&self) -> f64 {
        match (&self.undo, self.tone) {
            (Some(_), _) => UNDO_FOR,
            (None, Tone::Error) => ERROR_FOR,
            (None, _) => SHOW_FOR,
        }
    }
}
