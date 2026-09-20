//! The reading data a removal kept, waiting for the file it came from.
//!
//! Removing a book takes the app's own copy of its file with it, but the
//! marks a reader wrote, the place they stopped at and any name they gave it
//! are theirs: the removal sheet asks, and an answer of *keep* is written
//! down here.
//!
//! A record cannot be keyed by the row it came from — the row is what went —
//! so it is keyed by the file, the one thing a later import still has in
//! common with it. [`claim`] hands the best record for a file to the import
//! that brings it back.

use ai_core::gloss::GlossMark;
use library_core::book::{Book, Fingerprint, stem_of};
use library_core::scan::FoundFile;
use reader_core::format::Format;
use serde::{Deserialize, Serialize};

use super::{StorageError, encode, load_keyed, set};

const KEPT_KEY: &str = "mareader.kept.v1";

/// [`KEPT_KEY`] before the app was renamed: read as a fallback, never written.
const RETIRED_KEPT_KEY: &str = "pdfreader.kept.v1";

/// How many removals' worth of reading data the app holds. A ceiling,
/// because every removal adds and only an import of the same file takes away:
/// a store that only grows ends in a quota error with the reader's library in
/// it.
const KEPT_CAP: usize = 100;

/// What a removal kept about one book, in enough of its own words to be
/// recognised by the file that comes back.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptBook {
    /// The stem a later import is recognised by, derived the way every other
    /// name in the app is.
    ///
    /// `None` when the library never knew which file the bytes came from (a
    /// copy whose provenance was lost). The copy's own name is `source`,
    /// which belongs to the store and would answer for a reader's file that
    /// merely shares it, so such a record is matched by its bytes alone.
    #[serde(default)]
    name: Option<String>,
    /// The address the file was at: for a copied book, its source — the
    /// copy's own address is the store's, and the import that follows is of
    /// the source file.
    address: String,
    /// `None` for a book the library never measured: a placeholder is derived
    /// from the address, and this store would read the address's length as a
    /// file's size.
    #[serde(default)]
    fp: Option<Fingerprint>,
    #[serde(default)]
    format: Format,
    /// The name the reader gave the book, and only that one: a title the app
    /// captured from the document is captured again on the next open; a name a
    /// person typed is not.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub page: u32,
    #[serde(default)]
    pub num_pages: u32,
    #[serde(default)]
    pub fraction: Option<f64>,
    #[serde(default)]
    pub last_read_ms: u64,
    #[serde(default)]
    pub marks: Vec<GlossMark>,
}

/// How much of itself a file shares with a kept record, weakest first — the
/// order an import takes them in: same bytes is the strongest thing two
/// files can share, a name alone the weakest answer the store accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Confidence {
    /// The name and the kind agree and nothing else does.
    Name,
    /// The address agrees and the bytes do not: the file was rewritten where it stood.
    Rewritten,
    /// The bytes agree under a different address: the file moved.
    Moved,
    /// The address and the bytes both agree: the file itself.
    Same,
}

impl KeptBook {
    /// What the library held about `book`, as it leaves it.
    pub fn of(book: &Book, marks: Vec<GlossMark>) -> Self {
        let source = book.origin.source();
        Self {
            name: source.map(stem_of),
            address: source.unwrap_or_else(|| book.path()).to_string(),
            fp: (!book.fp_pending).then_some(book.fp),
            format: book.format,
            title: book.title.clone().filter(|_| book.title_locked),
            page: book.page,
            num_pages: book.num_pages,
            fraction: book.fraction,
            last_read_ms: book.last_read_ms,
            marks,
        }
    }

    /// How well `file` answers for this record, `None` when the two have nothing in common.
    ///
    /// `size` and `head_hash` are what "the same bytes" means: they are the two fields that
    /// identify content, and a copy the app made carries its OWN stamp, so comparing whole
    /// fingerprints would refuse to recognise every stored book's source.
    fn confidence(&self, file: &FoundFile, name: &str) -> Option<Confidence> {
        let same_bytes = self
            .fp
            .is_some_and(|fp| fp.size == file.fp.size && fp.head_hash == file.fp.head_hash);
        match (same_bytes, self.address == file.path) {
            (true, true) => Some(Confidence::Same),
            (true, false) => Some(Confidence::Moved),
            (false, true) => Some(Confidence::Rewritten),
            (false, false) => (self.name.as_deref() == Some(name)
                && file.format() == Some(self.format))
            .then_some(Confidence::Name),
        }
    }
}

/// Every record the app holds, oldest first.
fn load() -> Vec<KeptBook> {
    load_keyed("kept", KEPT_KEY, RETIRED_KEPT_KEY)
}

fn save(all: &[KeptBook]) -> Result<(), StorageError> {
    set(KEPT_KEY, &encode("save_kept", all)?)
}

/// Keep what the library held about `book`, under the file it came from.
pub fn remember(book: &Book, marks: Vec<GlossMark>) {
    let all = remembered(load(), KeptBook::of(book, marks));
    report(save(&all));
}

/// Drop what waits for this book's file. The sheet's answer was to delete the reader's data, and a
/// record left behind would put it back on the next import — the answer before the question.
pub fn forget(book: &Book) {
    let address = book.origin.source().unwrap_or_else(|| book.path());
    let mut all = load();
    let before = all.len();
    all.retain(|each| each.address != address);
    if all.len() < before {
        report(save(&all));
    }
}

/// The best record `file` answers for, spent: the marks and the place it kept belong to the row
/// this import is landing, and a second import of the same file is a second book rather than a
/// second helping of the same reading data.
pub fn claim(file: &FoundFile) -> Option<KeptBook> {
    let mut all = load();
    let at = best_for(&all, file)?;
    let kept = all.remove(at);
    report(save(&all));
    Some(kept)
}

/// The store after one more removal: the answer the reader just gave replaces an older record of
/// the same address rather than stacking on it, and the newest [`KEPT_CAP`] are what it holds.
fn remembered(mut all: Vec<KeptBook>, kept: KeptBook) -> Vec<KeptBook> {
    all.retain(|each| each.address != kept.address);
    all.push(kept);
    let excess = all.len().saturating_sub(KEPT_CAP);
    all.drain(..excess);
    all
}

/// Which record a file answers for best, `None` when none of them shares anything with it. A tie
/// goes to the newer record, which is why the list's own order decides it: the last removal that
/// kept this file is the answer the reader would expect to come back.
fn best_for(all: &[KeptBook], file: &FoundFile) -> Option<usize> {
    let name = stem_of(&file.path);
    let mut best: Option<(Confidence, usize)> = None;
    for (at, kept) in all.iter().enumerate() {
        let Some(confidence) = kept.confidence(file, &name) else {
            continue;
        };
        if best.is_none_or(|(found, _)| confidence >= found) {
            best = Some((confidence, at));
        }
    }
    best.map(|(_, at)| at)
}

/// A store that will not write is a store the reader can still remove books from: the write is
/// reported and the app carries on, the way every other save in this module does.
fn report(result: Result<(), StorageError>) {
    if let Err(e) = result {
        e.report();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_core::gloss::{GlossBox, PageAnchor};
    use library_core::book::Origin;
    use library_core::testkit::{book, fp_n};

    fn found(path: &str, fp: Fingerprint) -> FoundFile {
        let ext = path.rsplit_once('.').map(|(_, ext)| ext).unwrap_or_default();
        FoundFile {
            path: path.to_string(),
            rel: String::new(),
            ext: ext.to_lowercase(),
            size: fp.size,
            fp,
        }
    }

    fn mark(word: &str) -> GlossMark {
        GlossMark {
            id: word.to_string(),
            word: word.to_string(),
            context: String::new(),
            anchor: PageAnchor {
                page: 1,
                rect: GlossBox {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                    r: 0.0,
                },
            },
        }
    }

    /// A book the app copied: the reader's own file still exists somewhere, and the bytes the
    /// library holds are its own copy's.
    fn stored(id: &str, source: &str) -> Book {
        Book {
            origin: Origin::Stored {
                src: Some(source.to_string()),
                store: format!("/store/{id}/source.pdf"),
            },
            ..book(id)
        }
    }

    #[test]
    fn a_record_is_keyed_by_the_file_and_not_by_the_copy_the_app_made() {
        let kept = KeptBook::of(&stored("b1", "/downloads/dune.pdf"), vec![mark("sietch")]);
        assert_eq!(
            kept.name.as_deref(),
            Some("dune"),
            "the stem a later import is recognised by"
        );
        assert_eq!(
            kept.address, "/downloads/dune.pdf",
            "the source, because the store's own address goes with the copy"
        );
        assert_eq!(kept.marks.len(), 1, "the reader's marks travel with it");
    }

    #[test]
    fn a_copy_whose_provenance_was_lost_is_matched_by_its_bytes_and_never_by_the_store_s_name() {
        let orphan = Book {
            origin: Origin::Stored {
                src: None,
                store: "/store/b1/source.pdf".to_string(),
            },
            ..book("b1")
        };
        let kept = KeptBook::of(&orphan, Vec::new());
        assert!(kept.name.is_none(), "the store's own name is not the file's");
        assert_eq!(
            kept.confidence(&found("/downloads/source.pdf", fp_n(9)), "source"),
            None,
            "a reader's file that happens to be called `source` is not the book"
        );
        assert_eq!(
            kept.confidence(&found("/downloads/anything.pdf", orphan.fp), "anything"),
            Some(Confidence::Moved),
            "the bytes are what is left to know it by"
        );
    }

    #[test]
    fn a_book_the_library_never_measured_is_kept_without_bytes_to_match() {
        let never_measured = Book {
            fp_pending: true,
            ..book("b1")
        };
        let kept = KeptBook::of(&never_measured, Vec::new());
        assert!(kept.fp.is_none(), "a placeholder is not an identity");
        assert_eq!(
            kept.confidence(&found("/books/b1.pdf", never_measured.fp), "b1"),
            Some(Confidence::Rewritten),
            "so it answers for a file by address or by name and never by bytes"
        );
    }

    #[test]
    fn the_same_bytes_at_the_same_address_is_the_file_itself() {
        let kept = KeptBook::of(&book("b1"), Vec::new());
        let there = found("/books/b1.pdf", book("b1").fp);
        assert_eq!(kept.confidence(&there, "b1"), Some(Confidence::Same));

        let restamped = Fingerprint {
            mtime_ms: 99,
            ..book("b1").fp
        };
        assert_eq!(
            kept.confidence(&found("/books/b1.pdf", restamped), "b1"),
            Some(Confidence::Same),
            "a copy's own stamp is not a difference in content"
        );

        let other_head = Fingerprint {
            head_hash: 9,
            ..book("b1").fp
        };
        assert_eq!(
            kept.confidence(&found("/books/b1.pdf", other_head), "b1"),
            Some(Confidence::Rewritten),
            "two books of one length are told apart by their heads"
        );
    }

    #[test]
    fn bytes_under_a_new_address_outrank_the_address_alone() {
        let kept = KeptBook::of(&book("b1"), Vec::new());
        assert_eq!(
            kept.confidence(&found("/moved/b1.pdf", book("b1").fp), "b1"),
            Some(Confidence::Moved)
        );
        assert_eq!(
            kept.confidence(&found("/books/b1.pdf", fp_n(9)), "b1"),
            Some(Confidence::Rewritten)
        );
        assert!(Confidence::Moved > Confidence::Rewritten);
        assert!(Confidence::Rewritten > Confidence::Name);
    }

    #[test]
    fn a_name_alone_answers_only_when_it_is_the_same_name_and_kind() {
        let kept = KeptBook::of(&book("b1"), Vec::new());
        assert_eq!(
            kept.confidence(&found("/elsewhere/b1.pdf", fp_n(9)), "b1"),
            Some(Confidence::Name)
        );
        assert_eq!(
            kept.confidence(&found("/elsewhere/b1.md", fp_n(9)), "b1"),
            None,
            "a different kind is a different book however it is named"
        );
        assert_eq!(kept.confidence(&found("/elsewhere/b2.pdf", fp_n(9)), "b2"), None);
    }

    #[test]
    fn a_tie_goes_to_the_removal_that_kept_the_file_last() {
        let elsewhere = found("/elsewhere/b1.pdf", fp_n(9));
        let older = KeptBook::of(&book("b1"), vec![mark("first")]);
        let newer = KeptBook::of(&book("b1"), vec![mark("second")]);
        assert_eq!(
            best_for(&[older, newer], &elsewhere),
            Some(1),
            "both answer by name, and the later removal is the answer"
        );
    }

    #[test]
    fn a_record_that_shares_more_with_the_file_beats_a_newer_one_that_shares_less() {
        let elsewhere = found("/elsewhere/b1.pdf", fp_n(9));
        let by_name = KeptBook::of(&book("b1"), Vec::new());
        let mut moved = KeptBook::of(&book("b2"), Vec::new());
        moved.fp = Some(fp_n(9));
        moved.address = "/moved/b2.pdf".to_string();
        let records = vec![moved, by_name];
        assert_eq!(
            best_for(&records, &elsewhere),
            Some(0),
            "the moved file's bytes are this file's, however the name agrees"
        );
        assert_eq!(
            best_for(&records, &found("/moved/b2.pdf", fp_n(9))),
            Some(0),
            "and the move itself is the strongest answer either record can give"
        );
    }

    #[test]
    fn a_removal_replaces_the_record_of_its_own_file_and_the_oldest_falls_off_the_end() {
        let first = KeptBook::of(&stored("b1", "/downloads/dune.pdf"), vec![mark("first")]);
        let all = remembered(Vec::new(), first);
        assert_eq!(all.len(), 1);

        let again = KeptBook::of(&stored("b1", "/downloads/dune.pdf"), vec![mark("second")]);
        let mut all = remembered(all, again);
        assert_eq!(all.len(), 1, "a second removal of one file is one record");
        assert_eq!(all[0].marks[0].word, "second", "wearing the newer answer");

        for n in 1..=KEPT_CAP {
            let source = format!("/downloads/{n}.pdf");
            all = remembered(all, KeptBook::of(&stored("b", &source), Vec::new()));
        }
        assert_eq!(all.len(), KEPT_CAP);
        assert_eq!(
            all[0].name.as_deref(),
            Some("1"),
            "the oldest removal is the one that goes"
        );
        assert_eq!(all[KEPT_CAP - 1].name.as_deref(), Some(KEPT_CAP.to_string().as_str()));
    }
}
