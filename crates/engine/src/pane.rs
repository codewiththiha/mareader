//! Panes as commands see them: how a page is laid out, and its zoom.

/// How a pane's document is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ViewMode {
    /// One page at a time, turned by keys.
    Single,
    /// Two pages side by side: 1 and 2, 3 and 4.
    Spread,
    #[default]
    Vertical,
    Horizontal,
}

impl ViewMode {
    pub const ALL: [Self; 4] = [Self::Single, Self::Spread, Self::Vertical, Self::Horizontal];

    pub fn paged(self) -> bool {
        matches!(self, Self::Single | Self::Spread)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Single page",
            Self::Spread => "Two pages",
            Self::Vertical => "Vertical scroll",
            Self::Horizontal => "Horizontal scroll",
        }
    }

    /// A word for under its icon, inside its full label.
    pub fn short(self) -> &'static str {
        match self {
            Self::Single => "Single",
            Self::Spread => "Two pages",
            Self::Vertical => "Vertical",
            Self::Horizontal => "Horizontal",
        }
    }

    /// What shows with `page`: first page and count; `lead` pages alone.
    pub fn span(self, page: usize, lead: usize) -> (usize, usize) {
        match self {
            Self::Spread if page >= lead => (lead + (page - lead) / 2 * 2, 2),
            _ => (page, 1),
        }
    }

    /// The first page of what shows with `page` in it.
    pub fn start(self, page: usize, lead: usize) -> usize {
        self.span(page, lead).0
    }

    /// The page one turn from `page`, clamped to the document.
    pub fn turn(self, page: usize, forward: bool, (count, lead): (usize, usize)) -> usize {
        let last = self.start(count.saturating_sub(1), lead);
        let (from, shown) = self.span(page, lead);
        match (forward, from.checked_sub(1)) {
            (true, _) => (from + shown).min(last),
            (false, Some(back)) => self.start(back, lead),
            (false, None) => 0,
        }
    }
}

/// An arrow key over a document: a turn, or the scroll's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

/// A pane's zoom, as a command asks for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomCommand {
    Step(Step),
    Fit(Fit),
}

/// The ladder `+` and `-` climb.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    In,
    Out,
}

/// A zoom that follows the pane's size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    Width,
    Page,
}

impl Arrow {
    /// One step forward, right or down; minus one back.
    pub fn sign(self) -> f32 {
        if matches!(self, Self::Right | Self::Down) {
            1.0
        } else {
            -1.0
        }
    }

    /// mareader's rule: arrows turn, except along a strip's own axis.
    pub fn turns(self, mode: ViewMode, rtl: bool) -> Option<bool> {
        // A right-to-left book's pages turn leftward.
        let ahead = if rtl && mode.paged() {
            Self::Left
        } else {
            Self::Right
        };
        let forward = self == ahead || self == Self::Down;
        let along = match self {
            Self::Left | Self::Right => mode == ViewMode::Horizontal,
            Self::Up | Self::Down => !mode.paged(),
        };
        (!along).then_some(forward)
    }
}
