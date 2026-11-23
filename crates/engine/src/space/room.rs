//! Space's rooms: the hub of its tools, and each tool's own page.

/// The page Space shows: its hub, or one tool opened from it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Room {
    #[default]
    Hub,
    /// Every studied word, filtered and exported.
    Words,
    /// Every highlight, note and bookmark.
    Notes,
}

impl Room {
    pub const ALL: [Room; 3] = [Room::Hub, Room::Words, Room::Notes];

    /// Its name in commands and shot keys.
    pub fn name(self) -> &'static str {
        match self {
            Room::Hub => "hub",
            Room::Words => "words",
            Room::Notes => "notes",
        }
    }

    pub fn parse(name: &str) -> Option<Room> {
        Room::ALL.into_iter().find(|r| r.name() == name)
    }

    /// Its title in the bar.
    pub fn title(self) -> &'static str {
        match self {
            Room::Hub => "Space",
            Room::Words => "Vocabs",
            Room::Notes => "Notes",
        }
    }
}
