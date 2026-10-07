//! Name collisions on a level: does the shelf already hold that name?

use std::collections::HashSet;

use crate::book::{Row, duplicate_title, stem_of};
use crate::scan::FoundFile;
use crate::shelf::Shelf;

/// What is arriving, and where: one value for both routes in.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    /// The name being placed, as the shelf shows it.
    pub name: String,
    pub moving: Option<String>,
    /// The file being imported, `None` for a move.
    pub file: Option<FoundFile>,
    pub shelf_id: String,
    /// The level the arrival leaves, if it leaves one.
    pub from: Option<String>,
    /// The slot the drop pointed at; `None` appends.
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

    /// A row being moved or filed onto a level.
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

    /// Name the level this arrival leaves, for the survivor's sake.
    pub fn leaving(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    /// A folder arriving under a name its level already holds.
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

/// Whether two names are the same: case-insensitive only.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The row on the target level whose name this arrival carries.
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

/// The next free name on one level, or `name` itself.
pub fn next_name(rows: &[Row], shelves: &[Shelf], shelf_id: &str, name: &str) -> String {
    let index = crate::book::index_by_id(rows);
    let in_use: Vec<String> = crate::shelf::members_of(rows, shelves, shelf_id)
        .iter()
        .filter_map(|member| index.get(*member).copied())
        .map(Row::display_name)
        .collect();
    free_name(name, &in_use)
}

/// The shelf at one level whose name an arriving folder carries.
pub fn collide_shelf(shelves: &[Shelf], parent: Option<&str>, name: &str) -> Option<String> {
    crate::shelf::children_of(shelves, parent)
        .into_iter()
        .find(|shelf| same_name(&shelf.name, name))
        .map(|shelf| shelf.id.clone())
}

/// The next free shelf name on one level.
pub fn next_shelf_name(shelves: &[Shelf], parent: Option<&str>, name: &str) -> String {
    let in_use: Vec<String> = crate::shelf::children_of(shelves, parent)
        .into_iter()
        .map(|shelf| shelf.name.clone())
        .collect();
    free_name(name, &in_use)
}

/// The file manager's copy-into-directory rule, once for both sides.
fn free_name(name: &str, in_use: &[String]) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() && !in_use.iter().any(|held| same_name(held, trimmed)) {
        return trimmed.to_string();
    }
    let pool: HashSet<String> = in_use.iter().cloned().collect();
    duplicate_title(name, &pool)
}

/// What the reader decided about a thing the library already holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Place nothing: take the reader to the thing that is already there.
    Open,
    /// Land it beside what is there, under the next free name.
    KeepBoth,
    /// Fold the arrival into what is there: the further read point wins.
    Merge,
    /// The thing that is there goes and the arrival takes its place.
    Replace,
    /// Put a pointer at the thing that is there instead of a second instance.
    LinkOnly,
}

impl Placement {
    /// Offered for an import of a file: no row to fold and none to displace.
    pub const FILE: &'static [Placement] =
        &[Placement::Open, Placement::KeepBoth, Placement::LinkOnly];

    /// Offered when a row is moved onto a row: the reader is holding the
    /// arrival.
    pub const MOVE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// [`Placement::MOVE`] with *link* standing in for the destructive
    /// *replace*.
    pub const MOVE_KEEPING_BOTH: &'static [Placement] =
        &[Placement::Merge, Placement::LinkOnly, Placement::KeepBoth];

    /// Offered for a covered file.
    pub const COVERED: &'static [Placement] = &[Placement::Open, Placement::KeepBoth];

    /// Offered per file of a merging folder on the compact sheet.
    pub const FOLDER_MERGE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// Offered for a stored folder arrival.
    pub const SHELF_STORED: &'static [Placement] =
        &[Placement::Open, Placement::Replace, Placement::KeepBoth];

    /// Offered for a read-at-place folder arrival under a different folder's
    /// name.
    pub const SHELF_READ_IN_PLACE: &'static [Placement] = &[Placement::LinkOnly, Placement::Merge];
}

/// Which thing the answer is about: the row, or the shelf.
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
}

/// One collision question: the arrival, the thing it met, and the offers.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementAsk {
    /// Kept whole: an answer places it.
    pub arrival: Arrival,
    /// The thing already there: reveal, fold, purge, or point at it.
    pub existing: Scope,
    /// Derived once: a sheet prints it in headings and buttons.
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

    /// A button the ask did not offer would be an answer with nothing to
    /// apply.
    pub fn offers_placement(&self, choice: Placement) -> bool {
        self.offers.contains(&choice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{Book, Fingerprint, Origin};
    use crate::shelf::ALL_SHELF;

    const ALL: [Placement; 5] = [
        Placement::Open,
        Placement::KeepBoth,
        Placement::Merge,
        Placement::Replace,
        Placement::LinkOnly,
    ];

    #[test]
    fn each_ask_offers_only_the_answers_it_can_apply() {
        assert!(!Placement::FILE.contains(&Placement::Merge));
        assert!(!Placement::FILE.contains(&Placement::Replace));
        assert!(Placement::FILE.contains(&Placement::Open));
        // A move has no "go and look at it": the reader holds the arrival.
        assert!(!Placement::MOVE.contains(&Placement::Open));
        assert!(Placement::MOVE.contains(&Placement::Replace));
        // The keeping-both shape swaps the destructive answer for a pointer.
        assert!(!Placement::MOVE_KEEPING_BOTH.contains(&Placement::Replace));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::LinkOnly));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::Merge));
        // A covered file is two answers.
        assert_eq!(Placement::COVERED, &[Placement::Open, Placement::KeepBoth]);
        // A folder's offers depend on its mode.
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
            assert!(list.iter().all(|p| ALL.contains(p)));
        }
    }

    #[test]
    fn an_ask_answers_for_the_thing_it_met_and_refuses_an_answer_it_did_not_offer() {
        let book = PlacementAsk::book(
            import("dune", "s1"),
            "b1".into(),
            "Dune".into(),
            Placement::FILE,
        );
        assert_eq!(book.existing.id(), "b1");
        assert!(matches!(book.existing, Scope::Book { .. }));
        assert!(book.offers_placement(Placement::KeepBoth));
        assert!(
            !book.offers_placement(Placement::Merge),
            "an import cannot fold a row it does not have"
        );

        let shelf = PlacementAsk::shelf(
            import("dune", "s1"),
            "s2".into(),
            "Sci-fi".into(),
            Placement::SHELF_STORED,
        );
        assert_eq!(shelf.existing.id(), "s2");
        assert!(matches!(shelf.existing, Scope::Shelf { .. }));
        assert!(shelf.offers_placement(Placement::Replace));
    }

    fn book(id: &str, path: &str) -> Row {
        Row::Book(Book {
            fp: Fingerprint {
                size: 10,
                mtime_ms: 5,
                head_hash: 9,
            },
            added_ms: 10,
            origin: Origin::Linked {
                src: path.to_string(),
            },
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
            fp: Fingerprint {
                size: 10,
                mtime_ms: 5,
                head_hash: 9,
            },
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
        // A minted name is not a reason to ask again.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
        ];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &drag("b2", "1_1", "s")), None);
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn a_link_neither_asks_nor_blocks() {
        // A link carries its target's name but is never what a collision is
        // found against.
        let rows = vec![titled("b1", "/books/1.pdf", "1"), link("l1", "1", "b1")];
        let shelves = vec![shelf("s", &["l1"]), shelf("t", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")),
            None,
            "the only row on this shelf is a pointer"
        );
        assert_eq!(
            collide(&rows, &shelves, &drag("l1", "1", "t")).as_deref(),
            Some("b1")
        );
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
        // A collision is a name on one level.
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
        // `1` and `1_1` are taken on s, so the next free counter is `1_2`.
        assert_eq!(next_name(&rows, &shelves, "s", "1"), "1_2");
        assert_eq!(next_name(&rows, &shelves, "t", "1"), "1_1");
        // An empty level holds no name.
        assert_eq!(next_name(&rows, &shelves, "empty", "1"), "1");
        assert_eq!(next_name(&rows, &shelves, "t", " 1 "), "1_1");
        assert_eq!(next_name(&rows, &shelves, "s", "1_1"), "1_2");
    }

    #[test]
    fn an_arrival_carries_its_own_name_and_its_file() {
        let at = Arrival::import(file("1.pdf"), "s", Some(2));
        assert_eq!(
            at.name, "1",
            "the name is the stem: a title is not a file name"
        );
        assert!(at.is_import());
        assert_eq!(at.moving, None);
        assert_eq!(at.index, Some(2));
        assert_eq!(
            at.file.as_ref().map(|f| f.path.as_str()),
            Some("/books/1.pdf")
        );
        let moved = Arrival::moved("b1", "Dune", ALL_SHELF, None);
        assert!(!moved.is_import());
        assert_eq!(moved.moving.as_deref(), Some("b1"));
        assert_eq!(moved.name, "Dune");
    }

    #[test]
    fn a_move_names_the_level_it_leaves_and_nothing_else_does() {
        // A merge keeps every shelf but the one it lifts off.
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
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/books/2.pdf", "2"),
        ];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &["b2"])];
        assert_eq!(
            collide(&rows, &shelves, &drag("b2", "1", "s").leaving("t")).as_deref(),
            Some("b1")
        );
    }

    #[test]
    fn a_shelf_name_the_level_already_holds_asks() {
        let shelves = vec![plain_shelf("s1", "Books")];
        assert_eq!(
            collide_shelf(&shelves, None, "Books").as_deref(),
            Some("s1")
        );
        assert_eq!(
            collide_shelf(&shelves, None, "books").as_deref(),
            Some("s1")
        );
        assert_eq!(collide_shelf(&shelves, None, "Comics"), None);
        assert_eq!(collide_shelf(&shelves, None, "Books_1"), None);
    }

    #[test]
    fn a_shelf_collision_is_the_level_it_lands_on() {
        let shelves = vec![
            plain_shelf("s1", "Fiction"),
            Shelf {
                parent: Some("s1".to_string()),
                ..plain_shelf("s2", "Deep")
            },
        ];
        assert_eq!(
            collide_shelf(&shelves, None, "Fiction").as_deref(),
            Some("s1")
        );
        assert_eq!(collide_shelf(&shelves, Some("s1"), "Fiction"), None);
        assert_eq!(
            collide_shelf(&shelves, Some("s1"), "Deep").as_deref(),
            Some("s2")
        );
    }

    #[test]
    fn the_shelf_counter_counts_the_level_it_lands_on() {
        let shelves = vec![
            plain_shelf("s1", "Books"),
            plain_shelf("s2", "Books_1"),
            Shelf {
                parent: Some("s1".to_string()),
                ..plain_shelf("s3", "Books")
            },
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
