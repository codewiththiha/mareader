//! The one question the library asks about a name: does the level this is
//! going to already hold a book called that?
//!
//! Nothing else about duplicates is a dialog. A second copy of one file is a
//! naming problem with a naming answer — the counter a file manager appends —
//! and a reader who wants a pointer rather than a copy gets a row that points
//! ([`crate::book::Row::Link`]). So this module is one predicate
//! ([`collide`]), one namer ([`next_name`]) and the answers
//! ([`Placement`]) the sheet offers when the predicate says yes.
//!
//! ## Why names and not content
//!
//! The question used to be asked of a file's fingerprint, which made it a
//! question about bytes: two rows of one file shared an address, and with it
//! their highlights, their resume point and their removal, so keeping both
//! meant two books that could not be told apart by anything the reader could
//! see. Names are what a shelf is: a reader looking at two rows called `1` and
//! `1_1` knows they are two books, and a reader looking at two rows both
//! called `1` does not. So the collision is a name collision, on the level the
//! arrival is going to, and everything the fingerprint used to decide is now
//! either a naming decision ([`next_name`]) or a pointer
//! ([`Placement::LinkOnly`]).
//!
//! Fingerprints are not gone from the library — a watched folder's rescan
//! ([`crate::ledger`]) and a path check ([`crate::book::apply_check`]) still
//! need one, because "is this the file I already placed" is a question about
//! bytes and only bytes can answer it. They are gone from this module, which
//! asks a different question.
//!
//! ## What never asks
//!
//! A link, on either side: it is not a book, so it is never a collision and
//! never the thing collided with. The row being moved itself, because a
//! reorder is not an arrival. A level that holds no book of that name, which
//! includes an empty folder and includes the shelf holding `1_1` when `1`
//! arrives — different names are different books, and a counter a previous
//! answer minted is not a reason to ask again.
//!
//! Pure, and host-tested: no state, no signals, no storage, so the four rules
//! above are assertions rather than behaviours.

use std::collections::HashSet;

use crate::book::{Row, duplicate_title, stem_of};
use crate::scan::FoundFile;
use crate::shelf::Shelf;

/// What is arriving, and where it is going.
///
/// One value for both routes in, which is the reason an import and a drag can
/// no longer disagree about what a collision is: they build the same arrival
/// and hand it to the same [`collide`].
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    /// The name being placed, as the shelf would show it: a file's stem for an
    /// import, the moved row's own name for a drag. This is what [`collide`]
    /// compares, so it is never a file name with an extension on it — a title
    /// that looks like a file name is a title [`crate::book::sanitize`] drops.
    pub name: String,
    /// The row being moved, when this is a move. It cannot collide with
    /// itself, and a link row's id is as valid here as a book's.
    pub moving: Option<String>,
    /// The file being imported, when this is an import: the address and the
    /// measurement the row is minted from. A move has none — its row already
    /// exists.
    pub file: Option<FoundFile>,
    /// The level the arrival is going to; [`ALL_SHELF`] for the root.
    pub shelf_id: String,
    /// The level the arrival LEAVES, when it leaves one: a drag's source
    /// shelf, or the shelf a lift-out takes a book off. `None` for an import
    /// (no level is left), for a filing (a second membership is no departure
    /// — the row stays where it was) and for a drag that began at the root,
    /// which has no member list to leave. The answers that fold or dissolve
    /// the moving row read it: the shelves the survivor takes over are the
    /// ones the row KEEPS, and the level a move is leaving is not one of
    /// them — a merge that filed the survivor back on the source would leave
    /// the book the reader just moved away still sitting where they moved it
    /// from, and the move would only look done after a second drag.
    pub from: Option<String>,
    /// The slot the drop pointed at; `None` appends. Carried rather than
    /// re-derived because an arrival that had to ask lands later, when the
    /// level it was aimed at may have moved.
    pub index: Option<usize>,
}

impl Arrival {
    /// A file being imported onto a level. An import leaves no level, so it
    /// carries no [`Arrival::from`].
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
    /// [`Row::display_name`], read by the caller because the caller is the one
    /// holding the row list.
    ///
    /// No [`Arrival::from`] yet: a filing leaves the row where it was, and a
    /// move names the level it lifts off with [`Arrival::leaving`].
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

    /// Name the level this arrival leaves, which is what tells an answer that
    /// dissolves the moving row — a merge, the sheet's one-book answer — that
    /// the survivor does not inherit it. A drag names the shelf the hand
    /// lifted off; a filing names none, because a second membership is not a
    /// departure.
    pub fn leaving(mut self, from: impl Into<String>) -> Self {
        self.from = Some(from.into());
        self
    }

    /// A FOLDER arriving under a name its level already holds.
    ///
    /// Neither of the two shapes above: nothing has been measured yet, so there
    /// is no [`FoundFile`], and nothing is being moved, so there is no row id.
    /// The name is what collided — the last segment of the path the reader
    /// picked — and the answers are about a whole import run rather than about
    /// one placement, which is why the run's own root and options ride the
    /// app's ask beside this rather than inside it.
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

    /// Whether this arrival is a file with no row of its own yet.
    pub fn is_import(&self) -> bool {
        self.file.is_some()
    }
}

/// Whether two names are the same name. Case-insensitive and nothing else: a
/// shelf is read by a person, and `Report` beside `report` is two rows of one
/// name however the filesystem would have spelled them.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// The row already on the target level whose name this arrival carries, when
/// there is one.
///
/// The whole rule, and all of it:
///
///   * only BOOK rows are compared — a link is not a book and has no name of
///     its own to defend, so it never collides and never blocks;
///   * only rows on the TARGET level, which is a shelf's own member list, and
///     at the root the rows nobody has filed (the list "All" renders — the
///     root is a level like any other and a drop on Home beside an unfiled row
///     of one name is the same question as a drop on a shelf beside a filed
///     one);
///   * the row being moved is never its own collision;
///   * names compare as [`same_name`], so a counter a previous answer minted
///     (`1_1`) is a different name from the one that arrives (`1`).
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

/// The next free name for `name` on one level — and a level that does not hold
/// the name has it free outright, so the answer is `name` itself. Only a level
/// that DOES hold it gets the counter a file manager appends: `1` → `1_1` →
/// `1_2`, counted against the names that level already shows rather than
/// against the whole library.
///
/// A level's own names are the right pool because the collision was a level's:
/// `1` on "Fiction" and `1` on "Sci-Fi" are two rows a reader never sees
/// together, and renaming the second of them would be an answer to a question
/// nobody asked. The two halves of the rule are the two halves of a file
/// manager's copy: a name nobody there wears lands as it is, and a name
/// somebody wears gets the next number nobody does. A library-wide pool was
/// neither — it numbered an arrival against a name held on a level the reader
/// was not looking at, so importing the same file into a second folder arrived
/// as `dune_2` in a folder that had never heard of `dune`.
///
/// "Holds it" is [`same_name`]'s question, the one [`collide`] asks, so the
/// namer and the collision cannot disagree about whether there was something to
/// number around. The counter itself is [`duplicate_title`]'s — it steps rather
/// than stacks (`1_1` becomes `1_2`, not `1_1_1`), it fills gaps, and the name
/// it mints survives [`crate::book::sanitize`]'s rule about titles that look
/// like file names.
pub fn next_name(rows: &[Row], shelves: &[Shelf], shelf_id: &str, name: &str) -> String {
    let index = crate::book::index_by_id(rows);
    let in_use: Vec<String> = crate::shelf::members_of(rows, shelves, shelf_id)
        .iter()
        .filter_map(|member| index.get(*member).copied())
        .map(Row::display_name)
        .collect();
    free_name(name, &in_use)
}

/// The shelf already at one level whose name an arriving folder carries, when
/// there is one.
///
/// The shelf half of [`collide`], and the question a folder import asks before
/// it mints its root shelf: two shelves of one name on one level are two doors
/// a reader cannot tell apart, exactly as two books of one name are. Both kinds
/// count — a virtual shelf the reader made defends its name against an arriving
/// folder exactly as a folder shelf does — and the level is
/// [`crate::shelf::children_of`]'s answer, the root being the shelves with no
/// parent.
pub fn collide_shelf(shelves: &[Shelf], parent: Option<&str>, name: &str) -> Option<String> {
    crate::shelf::children_of(shelves, parent)
        .into_iter()
        .find(|shelf| same_name(&shelf.name, name))
        .map(|shelf| shelf.id.clone())
}

/// The next free shelf name on one level: the name itself when that level
/// holds no shelf wearing it, and `Books` → `Books_1` → `Books_2` when it does
/// — the counter [`duplicate_title`] mints, counted against the SHELF names
/// that level holds rather than against its rows, because a shelf and a book of
/// one name are two different doors and only a second door of the same kind is
/// a second door too many.
pub fn next_shelf_name(shelves: &[Shelf], parent: Option<&str>, name: &str) -> String {
    let in_use: Vec<String> = crate::shelf::children_of(shelves, parent)
        .into_iter()
        .map(|shelf| shelf.name.clone())
        .collect();
    free_name(name, &in_use)
}

/// The name a level has free: `name` itself when nothing there wears it, and
/// the first counter nobody wears when something does.
///
/// One spelling for the book level and the shelf level, because the two are the
/// file manager's one rule about a copy landing in a directory and only differ
/// about whose names the directory holds. The emptiness guard is
/// [`duplicate_title`]'s own — a blank base falls back to a word there, and a
/// blank name is never "free" here.
fn free_name(name: &str, in_use: &[String]) -> String {
    let trimmed = name.trim();
    if !trimmed.is_empty() && !in_use.iter().any(|held| same_name(held, trimmed)) {
        return trimmed.to_string();
    }
    let pool: HashSet<String> = in_use.iter().cloned().collect();
    duplicate_title(name, &pool)
}

// ---------------------------------------------------------------------------
// One placement vocabulary.
// ---------------------------------------------------------------------------

/// What the reader decided to do about a thing the library already holds.
///
/// Five answers, and they are the whole of it: every sheet the library raises
/// about an arrival that met something already there — a name on the level, a
/// folder merging into a shelf, a file an in-place tree holds, a shelf arriving
/// under a name its level has, a row dragged onto a row — is offering some
/// subset of these five. They used to be five separate enums — an import's
/// three, a move's four, and the app's own folder-merge, covered and shelf
/// answers — each with its own apply function, and each of those
/// re-derived "how do I purge the loser", "how do I fold the reading progress"
/// and "how do I re-seat the shelf membership" for itself — which is why every
/// one of them had its own bugs and needed its own fix.
///
/// WHICH subset a sheet offers is the ask's own fact, not the answer's:
/// [`PlacementAsk::offers`]. An import has no row to fold and offers no *merge*;
/// a move has no "go and look at it" and offers no *open*. One enum and a list
/// of the buttons that make sense, rather than one enum per combination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Placement {
    /// Place nothing: take the reader to the thing that is already there — the
    /// import sheet's *already imported*, and the covered sheet's *go to the
    /// book the folder holds*.
    Open,
    /// Land it beside what is there, under the next free name — both are books
    /// of their own. *Add as new* on either sheet.
    KeepBoth,
    /// Fold the arrival into the thing that is there: the further reading place
    /// wins, a name or an author fills a gap, the arrival's shelves and marks
    /// join the survivor, and the arrival goes ([`crate::book::fold_books`]).
    /// The move sheet's *merge*, and a folder merging into the shelf it found.
    Merge,
    /// The thing that is there goes and the arrival takes its place — its slot,
    /// its name and every other shelf it was filed on.
    Replace,
    /// Put a pointer at the thing that is there instead of a second instance: a
    /// [`crate::book::Row::Link`] on this level. *Make link* on either sheet.
    LinkOnly,
}

impl Placement {
    /// The sentence a button wearing this answer says. One spelling per answer
    /// rather than one per sheet, so five sheets cannot drift about what the
    /// same decision is called.
    pub fn label(self) -> &'static str {
        match self {
            Placement::Open => "Already imported",
            Placement::KeepBoth => "Add as new",
            Placement::Merge => "Merge",
            Placement::Replace => "Replace",
            Placement::LinkOnly => "Make link",
        }
    }

    /// Whether this answer destroys anything — the row that is already there.
    /// What a sheet uses to decide whether the button owes a promise about what
    /// goes with it, and the reason *replace* is the only one of the five that
    /// reads as a warning.
    pub fn is_destructive(self) -> bool {
        matches!(self, Placement::Replace)
    }

    /// The three an import of a FILE is offered: there is no row to fold and no
    /// row to take the place of, so *merge* and *replace* are not answers a
    /// drop can give.
    pub const FILE: &'static [Placement] =
        &[Placement::Open, Placement::KeepBoth, Placement::LinkOnly];

    /// The three a ROW being moved onto a row is offered: the reader is holding
    /// the arrival, so "go and look at the other one" is not a question they
    /// need answered.
    pub const MOVE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// The same three, with *link* standing in for the destructive *replace* —
    /// offered when the row being moved reads a file at its place and the row on
    /// the level is one of the library's own copies, where neither side is the
    /// reader's to destroy.
    pub const MOVE_KEEPING_BOTH: &'static [Placement] =
        &[Placement::Merge, Placement::LinkOnly, Placement::KeepBoth];

    /// The two a covered file is offered: the library's own copy on this level,
    /// or the book the folder already holds. A second linked row of one
    /// read-at-place file is the one thing that rule can never make, so
    /// *keep both* lands a stored copy rather than a link.
    pub const COVERED: &'static [Placement] = &[Placement::Open, Placement::KeepBoth];

    /// The three one file of a merging folder is offered on the compact sheet:
    /// the row the shelf already holds takes the arriving file's measurement,
    /// the arriving file takes the row's place, or both stand under two names.
    /// *Open* is not one of them — the reader is importing the folder, so
    /// "go and look at the shelf" is not an answer to a file inside it.
    pub const FOLDER_MERGE: &'static [Placement] =
        &[Placement::Merge, Placement::Replace, Placement::KeepBoth];

    /// The three a STORED folder arrival is offered — copies the library owns,
    /// unrelated to any tree, so the question is the level's own: go and look at
    /// the shelf that is here, replace it, or a shelf of the next free name.
    /// *Merge* and *link* are not among them, because there is no tree to fold
    /// into and no read-at-place file to point at.
    pub const SHELF_STORED: &'static [Placement] =
        &[Placement::Open, Placement::Replace, Placement::KeepBoth];

    /// The two a READ-AT-PLACE folder arrival of a DIFFERENT folder's name is
    /// offered: a pointer at the shelf that is here, or the arriving folder IS
    /// that shelf and its books join it. *Keep both* is withheld as the second
    /// instance of one ground the family gate exists to prevent, and *replace*
    /// with it — neither side of a read-at-place collision is the reader's to
    /// destroy.
    pub const SHELF_READ_IN_PLACE: &'static [Placement] =
        &[Placement::LinkOnly, Placement::Merge];

    /// Every answer, for a test that asks what the vocabulary holds.
    pub const ALL: &'static [Placement] = &[
        Placement::Open,
        Placement::KeepBoth,
        Placement::Merge,
        Placement::Replace,
        Placement::LinkOnly,
    ];
}

/// Which thing the reader's answer is about: the row already there, or the shelf
/// already there.
///
/// The one branch a unified apply has to make. Everything else about an answer
/// is the same question asked of a book or of a shelf — purge it, fold into it,
/// seat beside it, point at it — and the two halves differ only in what a
/// "membership" is: a row's is the shelves it is filed on, a shelf's is the
/// books it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// A row of the library's list: a book or a link.
    Book { row_id: String },
    /// A shelf, which is membership and a name and nothing else.
    Shelf { shelf_id: String },
}

impl Scope {
    /// The id this scope names, whichever kind it is.
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

/// One question about one arrival that met something the library already holds.
///
/// Generalizes the app's five ask types into one: the arrival (which was always
/// general), the thing it met, that thing's name, and the subset of
/// [`Placement`] this particular question offers. A sheet renders `offers` and
/// does not need to know WHY those are the options — which is what lets one
/// component serve a drop, a drag, a folder merge, a covered file and a shelf
/// collision instead of five.
// `PartialEq` and not `Eq`: the ask carries an [`Arrival`], which is `PartialEq`
// alone, and an ask is never a map key or a set member — it is a question on
// screen, compared only by the tests that build one.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacementAsk {
    /// The arrival that met something. Kept whole: an answer places it, and a
    /// placement needs the file it measured or the row it was moving, the level
    /// it was going to and the slot the drop pointed at.
    pub arrival: Arrival,
    /// The row or shelf already there, which *open* reveals, *merge* folds into,
    /// *replace* purges and *link* points at.
    pub existing: Scope,
    /// That thing's name, read once: a sheet prints it in a heading and on
    /// buttons, and three derivations of one string is three chances to disagree
    /// about which book the question is about.
    pub existing_name: String,
    /// Which answers this question offers, in the order the sheet shows them.
    pub offers: &'static [Placement],
}

impl PlacementAsk {
    /// A question about a ROW already on the level.
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

    /// A question about a SHELF already on the level.
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

    /// Whether this ask offers `choice` at all. A sheet that rendered a button
    /// the ask did not offer would be an answer with nothing to apply, so the
    /// check is the ask's and not the view's.
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
        // The point of the vocabulary: five sheets used to carry five enums
        // between them, and the union of what they offered is these five.
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
        // An import has no row to fold and no row to displace.
        assert!(!Placement::FILE.contains(&Placement::Merge));
        assert!(!Placement::FILE.contains(&Placement::Replace));
        assert!(Placement::FILE.contains(&Placement::Open));
        // A move has no "go and look at it" — the reader is holding the arrival.
        assert!(!Placement::MOVE.contains(&Placement::Open));
        assert!(Placement::MOVE.contains(&Placement::Replace));
        // The shape that keeps both sides swaps the destructive answer for a
        // pointer, and changes nothing else.
        assert!(!Placement::MOVE_KEEPING_BOTH.contains(&Placement::Replace));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::LinkOnly));
        assert!(Placement::MOVE_KEEPING_BOTH.contains(&Placement::Merge));
        // A covered file is two answers: a second linked row of one read-at-place
        // file is the one thing that rule can never make.
        assert_eq!(Placement::COVERED, &[Placement::Open, Placement::KeepBoth]);
        // A folder's question is the arrival's MODE: a stored arrival gets the
        // level's own three, a read-at-place one of another folder's name gets the
        // pointer and the merge, with *keep both* withheld as the second instance
        // the family gate exists to prevent.
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
        // Every offer list is a subset of the vocabulary, so no sheet can render
        // a button the apply has never heard of.
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
        // One label per answer rather than one per sheet, so five sheets cannot
        // drift about what the same decision is called.
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

    /// A book row that shows `title`, which is what a collision compares.
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

    /// A shelf with a name of its own: the shelf collision asks about NAMES,
    /// and the row builder above names every shelf by its id.
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

    /// An import of `name.pdf` onto `shelf_id`.
    fn import(name: &str, shelf_id: &str) -> Arrival {
        Arrival::import(file(&format!("{name}.pdf")), shelf_id, None)
    }

    /// A drag of one row onto `shelf_id`.
    fn drag(row_id: &str, name: &str, shelf_id: &str) -> Arrival {
        Arrival::moved(row_id, name, shelf_id, None)
    }

    // -------------------------------------------------------------------
    // The four rules the question is made of.
    // -------------------------------------------------------------------

    #[test]
    fn a_name_already_on_the_shelf_asks() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(
            collide(&rows, &shelves, &import("1", "s")).as_deref(),
            Some("b1")
        );
        // And the same name spelled the way the other filesystem would.
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
        // Nothing in the library at all: the first file in is never a
        // duplicate of itself.
        let empty: Vec<Row> = Vec::new();
        assert_eq!(collide(&empty, &shelves, &import("1", "s")), None);
        // A shelf id that names no shelf is not a level with members in it.
        assert_eq!(collide(&rows, &shelves, &import("1", "gone")), None);
    }

    #[test]
    fn a_counter_copy_beside_its_original_never_asks() {
        // The answer to a first collision is a name, and a name that was
        // minted is not a reason to ask again: `1_1` arriving beside `1` is a
        // second book, and dragging it in beside the row it was named from is
        // the move the reader meant.
        let rows = vec![
            titled("b1", "/books/1.pdf", "1"),
            titled("b2", "/copies/1.pdf", "1_1"),
        ];
        let shelves = vec![shelf("s", &["b1"])];
        assert_eq!(collide(&rows, &shelves, &import("1_1", "s")), None);
        assert_eq!(collide(&rows, &shelves, &drag("b2", "1_1", "s")), None);
        // And the original still asks, which is the pair of facts that makes
        // the counter mean something.
        assert_eq!(collide(&rows, &shelves, &import("1", "s")).as_deref(), Some("b1"));
    }

    #[test]
    fn a_link_neither_asks_nor_blocks() {
        // A link is not a book: it carries the name of the book it points at,
        // and a second row of that name arriving is not a collision with a
        // pointer. Nor is a pointer the thing a collision is found against.
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
        // A link never collides with itself either, and a link moved onto the
        // shelf holding its own book is the quiet case the row exists for.
        assert_eq!(collide(&rows, &shelves, &drag("l1", "1", "s")), None);
    }

    // -------------------------------------------------------------------
    // The rest of the rule.
    // -------------------------------------------------------------------

    #[test]
    fn a_row_never_collides_with_itself() {
        let rows = vec![titled("b1", "/books/1.pdf", "1")];
        let shelves = vec![shelf("s", &["b1"]), shelf("t", &[])];
        assert_eq!(collide(&rows, &shelves, &drag("b1", "1", "s")), None);
        // A reorder inside its own shelf is the same fact with an index.
        let mut reorder = drag("b1", "1", "s");
        reorder.index = Some(0);
        assert_eq!(collide(&rows, &shelves, &reorder), None);
    }

    #[test]
    fn a_twin_on_another_shelf_is_not_this_shelfs_question() {
        // The collision is a NAME on a LEVEL. A book called `1` on Fiction says
        // nothing about a `1` arriving on Sci-Fi: the two are never on screen
        // together, and the reader who filed them there filed two books.
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
        // b1 is unfiled, so it is on the root's list and a drop on Home asks.
        assert_eq!(
            collide(&rows, &shelves, &import("1", ALL_SHELF)).as_deref(),
            Some("b1")
        );
        // b2 is filed, so it is not on that list, and Home is where a second
        // row of it lands as its own book.
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
        // On t only `1` is, so the same arrival is `1_1` there: the pool is the
        // level's, because the collision was.
        assert_eq!(next_name(&rows, &shelves, "t", "1"), "1_1");
        // A level that holds nothing holds no NAME either, and the file
        // manager's answer to a free name is the name: an arrival the level has
        // never seen lands as itself rather than as a counter of a collision
        // that never happened.
        assert_eq!(next_name(&rows, &shelves, "empty", "1"), "1");
        // "Holds it" is the collision's own case-insensitive question.
        assert_eq!(next_name(&rows, &shelves, "t", " 1 "), "1_1");
        // And a counter does not stack on a counter.
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
        // The departure is a fact the answers read: a merge inherits the
        // shelves the moved row KEEPS, and the level the move lifts off is
        // the one shelf it does not keep — a survivor filed back on it would
        // be a book that never visibly moved.
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
        // The departure changes nothing about the collision itself: the
        // question is the name on the level the arrival is GOING to.
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
        // A counter a previous answer minted is a different name, as with rows.
        assert_eq!(collide_shelf(&shelves, None, "Books_1"), None);
    }

    #[test]
    fn a_shelf_collision_is_the_level_it_lands_on() {
        // "Fiction" inside s1 does not defend the root's name, and the root's
        // "Fiction" does not defend the level inside s1.
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
        // The root's own names are the pool: the nested "Books" is another
        // level's business.
        assert_eq!(next_shelf_name(&shelves, None, "Books"), "Books_2");
        assert_eq!(next_shelf_name(&shelves, Some("s1"), "Books"), "Books_1");
        // A level with no shelves in it holds no name, and a free name lands
        // as itself — the shelf half of the book namer's rule.
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
