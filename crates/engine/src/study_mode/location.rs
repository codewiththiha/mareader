//! Convert the pure deck's locations to the library's saved marks.

use mareader_library::Spot;
use mareader_study::mode::Location;

pub(super) fn from_spot(spot: Spot) -> Option<Location> {
    match spot {
        Spot::Chars { from, to } => Some(Location::Chars { from, to }),
        Spot::Blocks { from, to } => Some(Location::Blocks { from, to }),
        Spot::Page { .. } | Spot::Text { .. } => None,
    }
}

pub(crate) fn to_spot(location: Location) -> Spot {
    match location {
        Location::Chars { from, to } => Spot::Chars { from, to },
        Location::Blocks { from, to } => Spot::Blocks { from, to },
    }
}
