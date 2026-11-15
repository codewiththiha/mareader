//! The parts reading may pass over whole: tables, lists, figures, asides.

use mareader_session::{Passage, TextLayer};
use mareader_settings::{Pass, Skips};

/// What kind of part a stretch of the book is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Table,
    List,
    Figure,
    Aside,
}

impl Kind {
    /// Its name as a title, as in "List ahead".
    pub fn name(self) -> &'static str {
        match self {
            Self::Table => "Table",
            Self::List => "List",
            Self::Figure => "Figure",
            Self::Aside => "Aside",
        }
    }

    pub fn bit(self) -> u8 {
        match self {
            Self::Table => 1,
            Self::List => 2,
            Self::Figure => 4,
            Self::Aside => 8,
        }
    }

    pub const ALL: [Self; 4] = [Self::Table, Self::List, Self::Figure, Self::Aside];

    pub fn set_skip(self, skips: &mut Skips) {
        let skip = match self {
            Self::Table => &mut skips.tables,
            Self::List => &mut skips.lists,
            Self::Figure => &mut skips.figures,
            Self::Aside => &mut skips.asides,
        };
        skip.on = true;
        skip.ask = false;
    }

    /// What reading does on reaching one, as the settings say.
    pub fn pass(self, skips: &Skips) -> Pass {
        let skip = match self {
            Self::Table => skips.tables,
            Self::List => skips.lists,
            Self::Figure => skips.figures,
            Self::Aside => skips.asides,
        };
        skip.pass()
    }
}

/// One part of the book: its kind, and which one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub kind: Kind,
    pub id: usize,
}

/// The parts character `char` of a page is in, outermost first.
pub fn at(layer: &TextLayer, char: usize) -> Vec<Part> {
    let kind = |p: Passage| match p {
        Passage::List => Kind::List,
        Passage::Figure => Kind::Figure,
        Passage::Aside => Kind::Aside,
    };
    let passages = layer.passages(char).iter();
    let mut parts: Vec<Part> = passages
        .map(|&(p, id)| Part { kind: kind(p), id })
        .collect();
    // A cell's lists are flattened into it, so a table is innermost.
    if let Some(id) = layer.table(char) {
        parts.push(Part {
            kind: Kind::Table,
            id,
        });
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use mareader_settings::Skip;

    #[test]
    fn a_table_sits_inside_the_list_it_is_in() {
        let layer = TextLayer {
            passages: vec![(2..9, vec![(Passage::List, 7)])],
            tables: vec![(4..6, 3)],
            ..TextLayer::default()
        };
        let list = Part {
            kind: Kind::List,
            id: 7,
        };
        let table = Part {
            kind: Kind::Table,
            id: 3,
        };
        assert_eq!(at(&layer, 0), vec![]);
        assert_eq!(at(&layer, 2), vec![list]);
        assert_eq!(at(&layer, 5), vec![list, table]);
    }

    #[test]
    fn each_kind_follows_its_own_setting() {
        let skips = Skips {
            figures: Skip {
                on: true,
                ask: false,
            },
            ..Skips::default()
        };
        assert_eq!(Kind::Figure.pass(&skips), Pass::Skip);
        assert_eq!(Kind::List.pass(&skips), Pass::Read);
    }
}
