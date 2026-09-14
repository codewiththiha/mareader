//! The one question the library asks about a name: does the level this is going to
//! already hold a book called that?
//!
//! Nothing else about duplicates is a dialog. A second copy of one file is a naming
//! problem with a naming answer, and a reader who wants a pointer rather than a copy
//! gets a row that points ([`crate::book::Row::Link`]).

use std::collections::HashSet;

use crate::book::{Row, duplicate_title, stem_of};
use crate::scan::FoundFile;
use crate::shelf::Shelf;

/// What is arriving, and where it is going. One value for both routes in, so an
/// import and a drag cannot disagree about what a collision is.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    /// The name being placed, as the shelf would show it: a file's stem for an import,
    /// the moved row's own name for a drag.
    pub name: String,
    pub moving: Option<String>,
    /// The file being imported: the address and the measurement the row is minted from.
    pub file: Option<FoundFile>,
    pub shelf_id: String,
    /// The level the arrival LEAVES, when it leaves one: a drag's source shelf, or the
    /// shelf a lift-out takes a book off. `None` for an import, for a filing, and for a
    /// drag that began at the root.
    pub from: Option<String>,
    /// The slot the drop pointed at; `None` appends. Carried rather than re-derived, because the level it was aimed at may have moved.
    pub index: Option<usize>,
}

impl Arrival {
    pub fn import(file: FoundFile, shelf_id: impl Into<String>, index: Option<usize>) -> Self {
        let name = stem_of(&file.path);
        Self {
            name,
            moving: None,
            file: Some(file),
            shelf_id: shelf_id.into(),
            from: None,
            index,
        }
    }

    /// A row being moved or filed onto a level. `name` is the row's own
    /// [`Row::display_name`], read by the caller because the caller holds the row list.
    pub fn moved(
        row_id: impl Into<String>,
        name: impl Into<String>,
        shelf_id: impl Into<String>,
        index: Option<usize>,
    ) -> Self {
        Self {
            name: name.into(),
            moving: Some(row_id.into()),
            file: None,
            shelf_id: shelf_id.into(),
            from: None,
            index,
        }
    }

    /// Name the level this arrival leaves, which tells an answer that dissolves the
    /// moving row that the survivor does not inherit it.
    pub fn leaving(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    /// A FOLDER arriving under a name its level already holds. Neither of the shapes
    /// above: nothing has been measured, and nothing is being moved. The answers are
    /// about a whole import run rather than about one placement.
    pub fn folder(name: impl Into<String>, shelf_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            moving: None,
            file: None,
            shelf_id: shelf_id.into(),
            from: None,
            index: None,
        }
    }

    pub fn is_import(&self) -> bool {
        self.file.is_some()
    }
}

/// Whether two names are the same name. Case-insensitive and nothing else: `Report`
/// beside `report` is two rows of one name however the filesystem spells them.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The row already on the target level whose name this arrival carries, when there
/// is one.
///
/// Only BOOK rows are compared — a link is not a book and never collides — and only
/// rows on the TARGET level, because the collision is the level's.
pub fn collide(rows: &[Row], shelves: &[Shelf], at: &Arrival) -> Option<String> {
    let index = crate::book::index_by_id(rows);
    crate::shelf::members_of(rows, shelves, &at.shelf_id)
        .into_iter()
        .find_map(|member| {
            let row = *index.get(member)?;
            if row.is_link() || Some(row.id()) == at.moving.as_deref() {
                return None;
            }
            same_name(&row.display_name(), &at.name).then(|| row.id().to_string())
        })
}

/// The next free name for `name` on one level, or `name` itself when the level does
/// not hold it.
///
/// Only a level that DOES hold it gets the counter a file manager appends:
/// `1` → `1_1` → `1_2`, counted against the names that level already shows rather
/// than against the whole library.
pub fn next_name(rows: &[Row], shelves: &[Shelf], shelf_id: &str, name: &str) -> String {
    let index = crate::book::index_by_id(rows);
    let in_use: Vec<String> = crate::shelf::members_of(rows, shelves, shelf_id)
        .iter()
        .filter_map(|member| index.get(*member).copied())
        .map(Row::display_name)
        .collect();
    free_name(name, &in_use)
}

/// The shelf already at one level whose name an arriving folder carries, when there
/// is one. The shelf half of [`collide`], and the question a folder import asks before
/// it mints its root shelf. Both kinds count.
pub fn collide_shelf(shelves: &[Shelf], parent: Option<&str>, name: &str) -> Option<String> {
    crate::shelf::children_of(shelves, parent)
        .into_iter()
        .find(|shelf| same_name(&shelf.name, name))
        .map(|shelf| shelf.id.clone())
}

/// The next free shelf name on one level: `Books` → `Books_1` → `Books_2`, counted
/// against the SHELF names that level holds rather than against its rows.
pub fn next_shelf_name(shelves: &[Shelf], parent: Option<&str>, name: &str) -> String {
    let in_use: Vec<String> = crate::shelf::children_of(shelves, parent)
        .into_iter()
        .map(|shelf| shelf.name.clone())
        .collect();
    free_name(name, &in_use)
}

/// One spelling for the book level and the shelf level: the two are the file
/// manager's one rule about a copy landing in a directory, and differ only about
/// whose names the directory holds.
fn free_name(name: &str, in_use: &[String]) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() && !in_use.iter().any(|held| same_name(held, trimmed)) {
        return trimmed.to_string();
    }
    let pool: HashSet<String> = in_use.iter().cloned().collect();
    duplicate_title(name, &pool)
}


/// What the reader decided to do about a thing the library already holds.
///
/// Five answers, and they are the whole of it: every sheet the library raises about
/// an arrival that met something already there offers some subset of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Place nothing: take the reader to the thing that is already there.
    Open,
    /// Land it beside what is there, under the next free name.
    KeepBoth,
    /// Fold the arrival into the thing that is there: the further reading place wins,
    /// the arrival's shelves and marks join the survivor, and the arrival goes
    /// ([`crate::book::fold_books`]).
    Merge,
    /// The thing that is there goes and the arrival takes its place.
    Replace,
    /// Put a pointer at the thing that is there instead of a second instance.
    LinkOnly,
}

impl Placement {
    /// The sentence a button wearing this answer says — one spelling per answer, so five sheets cannot drift.
    pub fn label(self) -> &'static str {
        match self {
            Placement::Open => "Already imported",
            Placement::KeepBoth => "Add as new",
            Placement::Merge => "Merge",
            Placement::Replace => "Replace",
            Placement::LinkOnly => "Make link",
        }
    }

    /// Whether this answer destroys anything, which is the reason *replace* is the only one of the five that reads as a warning.
    pub fn is_destructive(self) -> bool {
        matches!(self, Placement::Replace)
    }

    /// The three an import of a FILE is offered: no row to fold and no row to displace.
    pub const FILE: &'static [Placement] =
        &[Placement::Open, Placement::KeepBoth, Placement::LinkOnly];

    /// The three a ROW moved onto a row is offered: the reader is holding the arrival.
    pub const MOVE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// The same three with *link* standing in for the destructive *replace*.
    pub const MOVE_KEEPING_BOTH: &'static [Placement] =
        &[Placement::Merge, Placement::LinkOnly, Placement::KeepBoth];

    /// The two a covered file is offered: the library's own copy on this level, or the book the folder already holds.
    pub const COVERED: &'static [Placement] = &[Placement::Open, Placement::KeepBoth];

    /// The three one file of a merging folder is offered on the compact sheet.
    pub const FOLDER_MERGE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// The three a STORED folder arrival is offered — copies the library owns, so the question is the level's own.
    pub const SHELF_STORED: &'static [Placement] =
        &[Placement::Open, Placement::Replace, Placement::KeepBoth];

    /// The two a READ-AT-PLACE folder arrival of a DIFFERENT folder's name is offered.
    pub const SHELF_READ_IN_PLACE: &'static [Placement] =
        &[Placement::LinkOnly, Placement::Merge];

    pub const ALL: &'static [Placement] = &[
        Placement::Open,
        Placement::KeepBoth,
        Placement::Merge,
        Placement::Replace,
        Placement::LinkOnly,
    ];
}

/// Which thing the reader's answer is about: the row already there, or the shelf
/// already there. The two halves differ only in what a "membership" is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Book { row_id: String },
    Shelf { shelf_id: String },
}

impl Scope {
    pub fn id(&self) -> &str {
        match self {
            Scope::Book { row_id } => row_id,
            Scope::Shelf { shelf_id } => shelf_id,
        }
    }

    pub fn is_shelf(&self) -> bool {
        matches!(self, Scope::Shelf { .. })
    }
}

/// One question about one arrival that met something the library already holds: the
/// arrival, the thing it met, that thing's name, and the subset of [`Placement`] this
/// question offers. A sheet renders `offers` and does not need to know why.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementAsk {
    /// Kept whole: an answer places it, and a placement needs the file or the row and the level it was going to.
    pub arrival: Arrival,
    /// The thing already there, which *open* reveals, *merge* folds into, *replace* purges and *link* points at.
    pub existing: Scope,
    /// Read once: a sheet prints it in a heading and on buttons, and three derivations of one string can disagree.
    pub existing_name: String,
    pub offers: &'static [Placement],
}

impl PlacementAsk {
    pub fn book(
        arrival: Arrival,
        row_id: String,
        existing_name: String,
        offers: &'static [Placement],
    ) -> Self {
        Self {
            arrival,
            existing: Scope::Book { row_id },
            existing_name,
            offers,
        }
    }

    pub fn shelf(
        arrival: Arrival,
        shelf_id: String,
        existing_name: String,
        offers: &'static [Placement],
    ) -> Self {
        Self {
            arrival,
            existing: Scope::Shelf { shelf_id },
            existing_name,
            offers,
        }
    }

    /// A sheet that rendered a button the ask did not offer would be an answer with nothing to apply.
    pub fn offers_placement(&self, choice: Placement) -> bool {
        self.offers.contains(&choice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Fingerprint, Origin};
    use crate::shelf::ALL_SHELF;

    #[test]
    fn every_answer_a_sheet_can_offer_is_one_of_five() {
        let all = Placement::ALL;
        for offer in [
            Placement::Open,
            Placement::KeepBoth,
            Placement::Merge,
            Placement::Replace,
            Placement::LinkOnly,
        ] {
            assert!(all.contains(&offer));
        }
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn each_ask_offers_only_the_answers_it_can_apply() {
        assert!(!Placement::FILE.contains(&Placement::Merge));
        assert!(!Placement::FILE.contains(&Placement::Replace));
        assert!(Placement::FILE.contains(&Placement::Open));
        // A move has no "go and look at it": the reader is holding the arrival.
        assert!(!Placement::MOVE.contains(&Placement::Open));
        assert!(Placement::MOVE.contains(&Placement::Replace));
        // The shape that keeps both sides swaps the destructive answer for a pointer.
        assert!(!Placement::MOVE_KEEPING_BOTH.contains(&Placement::Replace));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::LinkOnly));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::Merge));
        // A covered file is two answers: a second linked row of one read-at-place file is the one thing that rule can never make.
        assert_eq!(Placement::COVERED, &[Placement::Open, Placement::KeepBoth]);
        // A folder's question is the arrival's MODE: a stored arrival gets the level's own
        // three, a read-at-place one of another folder's name gets the pointer and the merge.
        assert_eq!(
            Placement::SHELF_STORED,
            &[Placement::Open, Placement::Replace, Placement::KeepBoth]
        );
        assert_eq!(
            Placement::SHELF_READ_IN_PLACE,
            &[Placement::LinkOnly, Placement::Merge]
        );
        assert!(
            !Placement::SHELF_READ_IN_PLACE.contains(&Placement::KeepBoth),
            "a second instance of one ground is the answer the gate withholds"
        );
        for list in [
            Placement::FILE,
            Placement::MOVE,
            Placement::MOVE_KEEPING_BOTH,
            Placement::COVERED,
            Placement::FOLDER_MERGE,
            Placement::SHELF_STORED,
            Placement::SHELF_READ_IN_PLACE,
        ] {
            assert!(!list.is_empty());
            assert!(list.iter().all(|p| Placement::ALL.contains(p)));
        }
    }

    #[test]
    fn only_replace_destroys_the_thing_that_is_already_there() {
        for offer in Placement::ALL {
            assert_eq!(offer.is_destructive(), *offer == Placement::Replace);
        }
    }

    #[test]
    fn an_ask_answers_for_the_thing_it_met_and_refuses_an_answer_it_did_not_offer() {
        let book =
            PlacementAsk::book(import("dune", "s1"), "b1".into(), "Dune".into(), Placement::FILE);
        assert_eq!(book.existing.id(), "b1");
        assert!(!book.existing.is_shelf());
        assert!(book.offers_placement(Placement::KeepBoth));
        assert!(
            !book.offers_placement(Placement::Merge),
            "an import cannot fold a row it does not have"
        );

        let shelf = PlacementAsk::shelf(
            import("dune", "s1"),
            "s2".into(),
            "Sci-fi".into(),
            Placement::ALL,
        );
        assert_eq!(shelf.existing.id(), "s2");
        assert!(shelf.existing.is_shelf());
        assert!(shelf.offers_placement(Placement::Replace));
    }

    #[test]
    fn every_button_has_one_sentence_whichever_sheet_wears_it() {
        let labels: Vec<&str> = Placement::ALL.iter().map(|p| p.label()).collect();
        let mut deduped = labels.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(labels.len(), deduped.len(), "no two answers share a label");
        assert!(labels.iter().all(|l| !l.is_empty()));
    }

    fn book(id: &str, path: &str) -> Row {
        Row::Book(Book {
            fp: Fingerprint {
                size: 10,
                mtime_ms: 5,
                head_hash: 9,
            },
            added_ms: 10,
            origin: Origin::Linked { src: path.to_string() },
            ..crate::testkit::book(id)
        })
    }

    fn titled(id: &str, path: &str, title: &str) -> Row {
        let mut row = book(id, path);
        row.as_book_mut().unwrap().title = Some(title.to_string());
        row
    }

    fn link(id: &str, name: &str, target: &str) -> Row {
        crate::testkit::link(id, name, target)
    }

    fn shelf(id: &str, members: &[&str]) -> Shelf {
        crate::testkit::plain_shelf(id, members)
    }

    fn plain_shelf(id: &str, name: &str) -> Shelf {
        Shelf {
            name: name.to_string(),
            ..shelf(id, &[])
        }
    }

    fn file(name: &str) -> FoundFile {
        FoundFile {
            rel: name.to_string(),
            path: format!("/books/{name}"),
            ext: "pdf".to_string(),
            size: 10,
            fp: Fingerprint { size: 10, mtime_ms: 5, head_hash: 9 },
        }
    }

    fn import(name: &str, shelf_id: &str) -> Arrival {
        Arrival::import(file(&format!("{name}.pdf")), shelf_id, None)
    }

    fn drag(row_id: &str, name: &str, shelf_id: &str) -> Arrival {
        Arrival::moved(row_id, name, shelf_id, None)
    }


    #[test]
    fn a_name_already_on_the_shelf_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")).as_deref(),
            Some("b1")
        );
        let named = vec![titled("b1", "/books/report.pdf", "Report")];
        assert_eq!(
            collide(&named, &shelves, &import("report", "s")).as_deref(),
            Some("b1"),
            "a shelf is read by a person, not by a byte comparison"
        );
    }

    #[test]
    fn an_empty_shelf_never_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &[]), shelf("t", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1", "s")), None);
        let empty: Vec<Row> = Vec::new();
        assert_eq!(collide(&empty, &shelves, &import("1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &import("1", "gone")), None);
    }

    #[test]
    fn a_counter_copy_beside_its_original_never_asks() {
        // A name that was minted is not a reason to ask again: `1_1` arriving beside `1` is a second book.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
        ];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &drag("b2", "1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &import("1", "s")).as_deref(), Some("b1"));
    }

    #[test]
    fn a_link_neither_asks_nor_blocks() {
        // A link is not a book: it carries the name of the book it points at, and is never the thing a collision is found against.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            link("l1", "1", "b1"),
        ];
        let shelves = vec![shelf("s", &["l1"]), shelf("t", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")),
            None,
            "the only row on this shelf is a pointer"
        );
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "t")).as_deref(), Some("b1"));
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "s")), None);
    }


    #[test]
    fn a_row_never_collides_with_itself() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &[])];
        assert_eq!(collide(&rows, &shelves, &drag("b1", "1", "s")), None);
        let mut reorder = drag("b1", "1", "s");
        reorder.index = Some(0);
        assert_eq!(collide(&rows, &shelves, &reorder), None);
    }

    #[test]
    fn a_twin_on_another_shelf_is_not_this_shelfs_question() {
        // The collision is a NAME on a LEVEL: a `1` on Fiction says nothing about a `1` arriving on Sci-Fi.
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("fiction", &["b1"]), shelf("scifi", &[])];
        assert_eq!(collide(&rows, &shelves, &import("1", "scifi")), None);
    }

    #[test]
    fn the_root_is_a_level_and_its_list_is_the_unfiled_rows() {
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/books/2.pdf", "2"),
        ];
        let shelves = vec![shelf("s", &["b2"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", ALL_SHELF)).as_deref(),
            Some("b1")
        );
        assert_eq!(collide(&rows, &shelves, &import("2", ALL_SHELF)), None);
    }

    #[test]
    fn the_counter_counts_against_the_level_it_lands_on() {
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
            titled("b3", "/elsewhere/1.pdf", "1"),
        ];
        let shelves = vec![shelf("s", &["b1", "b2"]), shelf("t", &["b3"])];
        // On s, `1` and `1_1` are taken, so the next free counter is `1_2`.
        assert_eq!(next_name(&rows, &shelves, "s", "1"), "1_2");
        assert_eq!(next_name(&rows, &shelves, "t", "1"), "1_1");
        // A level that holds nothing holds no NAME either: a free name lands as itself, not as a counter of a collision that never happened.
        assert_eq!(next_name(&rows, &shelves, "empty", "1"), "1");
        assert_eq!(next_name(&rows, &shelves, "t", " 1 "), "1_1");
        assert_eq!(next_name(&rows, &shelves, "s", "1_1"), "1_2");
    }

    #[test]
    fn an_arrival_carries_its_own_name_and_its_file() {
        let at = Arrival::import(file("1.pdf"), "s", Some(2));
        assert_eq!(at.name, "1", "the name is the stem: a title is not a file name");
        assert!(at.is_import());
        assert_eq!(at.moving, None);
        assert_eq!(at.index, Some(2));
        assert_eq!(at.file.as_ref().map(|f| f.path.as_str()), Some("/books/1.pdf"));
        let moved = Arrival::moved("b1", "Dune", ALL_SHELF, None);
        assert!(!moved.is_import());
        assert_eq!(moved.moving.as_deref(), Some("b1"));
        assert_eq!(moved.name, "Dune");
    }

    #[test]
    fn a_move_names_the_level_it_leaves_and_nothing_else_does() {
        // The departure is a fact the answers read: a merge inherits the shelves the moved
        // row KEEPS, and the level it lifts off is the one shelf it does not keep.
        let leaving = Arrival::moved("b1", "Dune", "s", None).leaving("t");
        assert_eq!(leaving.from.as_deref(), Some("t"));
        assert_eq!(
            Arrival::moved("b1", "Dune", "s", None).from,
            None,
            "a filing names no departure: the row stays where it was"
        );
        assert_eq!(
            Arrival::import(file("1.pdf"), "s", None).from,
            None,
            "and an import leaves no level at all"
        );
        let rows = vec![titled("b1", "/books/1.pdf", "1"), titled("b2", "/books/2.pdf", "2")];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &["b2"])];
        assert_eq!(
            collide(&rows, &shelves, &drag("b2", "1", "s").leaving("t")).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn a_shelf_name_the_level_already_holds_asks() {
        let shelves = vec![plain_shelf("s1", "Books")];
        assert_eq!(collide_shelf(&shelves, None, "Books").as_deref(), Some("s1"));
        assert_eq!(collide_shelf(&shelves, None, "books").as_deref(), Some("s1"));
        assert_eq!(collide_shelf(&shelves, None, "Comics"), None);
        assert_eq!(collide_shelf(&shelves, None, "Books_1"), None);
    }

    #[test]
    fn a_shelf_collision_is_the_level_it_lands_on() {
        let shelves = vec![
            plain_shelf("s1", "Fiction"),
            Shelf { parent: Some("s1".to_string()), ..plain_shelf("s2", "Deep") },
        ];
        assert_eq!(collide_shelf(&shelves, None, "Fiction").as_deref(), Some("s1"));
        assert_eq!(collide_shelf(&shelves, Some("s1"), "Fiction"), None);
        assert_eq!(collide_shelf(&shelves, Some("s1"), "Deep").as_deref(), Some("s2"));
    }

    #[test]
    fn the_shelf_counter_counts_the_level_it_lands_on() {
        let shelves = vec![
            plain_shelf("s1", "Books"),
            plain_shelf("s2", "Books_1"),
            Shelf { parent: Some("s1".to_string()), ..plain_shelf("s3", "Books") },
        ];
        assert_eq!(next_shelf_name(&shelves, None, "Books"), "Books_2");
        assert_eq!(next_shelf_name(&shelves, Some("s1"), "Books"), "Books_1");
        assert_eq!(next_shelf_name(&shelves, Some("s2"), "Books"), "Books");
    }

    #[test]
    fn a_link_row_is_not_a_book_and_says_so() {
        let rows = [book("b1", "/books/1.pdf"), link("l1", "1", "b1")];
        assert!(rows[0].book().is_some() && !rows[0].is_link());
        assert!(rows[1].is_link() && rows[1].book().is_none());
        assert_eq!(rows[1].book(), None);
        assert_eq!(rows[1].fp(), None, "a pointer has no content identity");
        assert_eq!(rows[1].target(), Some("b1"));
        assert_eq!(rows[0].target(), None);
        assert_eq!(rows[1].display_name(), "1");
        assert_eq!(rows[1].id(), "l1");
        assert_eq!(rows[0].id(), "b1");
    }
}
