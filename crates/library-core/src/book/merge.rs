//! How two books that turn out to be one fold back into one: the fold keeps
//! whichever of the two the reader got further in, whichever name is real, and
//! both shelves' memberships.

use super::{Book, ReadPoint};

/// Fold one book into another: `gone` dissolves and `survivor` keeps its id, its
/// name and its address, taking everything the survivor does not already know.
///
/// The rule per field is the one a reader means by "these are the same book":
/// the placement, the read point and the measurement each travel from the row
/// that knows more, and a name the survivor already has wins.
pub fn fold_books(survivor: &mut Book, gone: &Book) {
    // The resume point travels as one unit: a page without its count is a
    // position the progress bar cannot draw.
    let mine = ReadPoint {
        page: survivor.page,
        num_pages: survivor.num_pages,
        fraction: survivor.fraction,
    };
    let theirs = ReadPoint {
        page: gone.page,
        num_pages: gone.num_pages,
        fraction: gone.fraction,
    };
    let point = further_point(mine, theirs).settled();
    survivor.page = point.page;
    survivor.fraction = point.fraction;
    survivor.num_pages = point.num_pages.max(survivor.num_pages).max(gone.num_pages);
    if crate::text::non_blank(survivor.title.as_deref()).is_none()
        && let Some(title) = crate::text::non_blank(gone.title.as_deref())
    {
        survivor.title = Some(title.to_string());
    }
    if survivor.author.is_none() {
        survivor.author = crate::text::non_blank(gone.author.as_deref()).map(str::to_string);
    }
    survivor.added_ms = earliest_known(survivor.added_ms, gone.added_ms);
    survivor.last_read_ms = survivor.last_read_ms.max(gone.last_read_ms);
    survivor.missing = survivor.missing && gone.missing;
    // The pending flag is the survivor's OWN before it is folded: two placeholders
    // stay one, but a placeholder yields to a row that has been weighed.
    let was_pending = survivor.fp_pending;
    survivor.fp_pending = was_pending && gone.fp_pending;
    if was_pending && !gone.fp_pending {
        survivor.fp = gone.fp;
    }
}

/// The point that got FURTHER: the higher page, and on a page tie the deeper
/// stream fraction. A full tie keeps the survivor's own, which is what makes
/// "the point came from the other row" a fact rather than a coin toss.
pub fn further_point(mine: ReadPoint, theirs: ReadPoint) -> ReadPoint {
    use std::cmp::Ordering;
    match theirs.page.cmp(&mine.page) {
        Ordering::Greater => theirs,
        Ordering::Less => mine,
        Ordering::Equal => match (mine.fraction, theirs.fraction) {
            (_, None) => mine,
            (None, Some(_)) => theirs,
            (Some(a), Some(b)) => {
                if b > a {
                    theirs
                } else {
                    mine
                }
            }
        },
    }
}

/// The earlier of two stamps, with `0` as the gap it is: a migrated row carries
/// no join stamp, and a fold that treated zero as the epoch would date every
/// book it touched to 1970.
fn earliest_known(mine: u64, theirs: u64) -> u64 {
    match (mine, theirs) {
        (0, other) | (other, 0) => other,
        _ => mine.min(theirs),
    }
}
