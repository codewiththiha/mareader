//! Reading aloud as rules: which words are lit, how long is left, who speaks.

mod active;
pub mod booth;
pub mod countdown;
pub mod feed;
pub mod lit;
mod narrated;
pub mod parts;
pub mod place;
pub mod player;
pub mod spoken;
pub mod state;
pub mod voices;
pub mod words;

use std::time::Duration;

use mareader_session::Anchor;
use mareader_settings::ReadAloudSettings;
use mareader_speech::{Spot, Tone};

/// How often the lit phrase is checked while it plays.
pub const TICK: Duration = Duration::from_millis(50);

/// The tone `settings` pick, as a voice hears it.
pub fn tone(settings: &ReadAloudSettings) -> Tone {
    Tone {
        rate: settings.rate,
        pitch: settings.pitch,
        volume: settings.volume,
    }
}

/// Where reading starts: the place or character under the pointer, or the page.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ReadFrom {
    pub anchor: Option<Anchor>,
    pub at: Option<Spot>,
    /// Where it stops: a selection's end, read alone by the voice.
    pub until: Option<Spot>,
}

impl ReadFrom {
    pub fn here(&self) -> bool {
        self.anchor.is_some() || self.at.is_some()
    }
}
